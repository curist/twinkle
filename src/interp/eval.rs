use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use crate::ir::core::{
    CoreExpr, CoreExprKind, CorePattern, FuncId, FunctionDef, LocalId, MatchArm,
};
use crate::ir::CoreModule;
use crate::syntax::ast::BinOp;
use crate::syntax::ast::UnOp as AstUnOp;
use crate::types::ty::{OPTION_TYPE_ID, RANGE_TYPE_ID, ITER_ITEM_TYPE_ID, UNFOLD_STEP_TYPE_ID};
use super::value::IteratorState;

use super::value::Value;

// ---------------------------------------------------------------------------
// Frame type alias
// ---------------------------------------------------------------------------

type Frame = HashMap<LocalId, Value>;

// ---------------------------------------------------------------------------
// Control-flow signals
// ---------------------------------------------------------------------------

/// Non-local exits that propagate through the call stack.
enum Signal {
    Break(Option<Value>),
    Continue,
    Return(Option<Value>),
}

type EvalResult = Result<Value, Signal>;

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

pub struct Interpreter<W: Write = Box<dyn Write>> {
    module: CoreModule,
    func_index: HashMap<FuncId, usize>,
    output: W,
}

impl<W: Write> Interpreter<W> {
    pub fn new(module: CoreModule, output: W) -> Self {
        let func_index = module.functions.iter().enumerate()
            .map(|(i, f)| (f.func_id, i))
            .collect();
        Self { module, func_index, output }
    }

    /// Consume the interpreter and return the underlying output sink.
    /// Useful in tests to inspect captured bytes.
    pub fn into_output(self) -> W {
        self.output
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        if let Some(id) = self.module.init_func_id {
            self.call_func(id, vec![], HashMap::new())
                .map_err(|_| anyhow::anyhow!("top-level execution failed with unhandled signal"))?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Function calls
    // -----------------------------------------------------------------------

    fn call_func(
        &mut self,
        func_id: FuncId,
        args: Vec<Value>,
        captured: Frame,
    ) -> EvalResult {
        // Prelude / built-in functions (FuncId 1–22)
        if func_id.0 < crate::ir::lower::prelude::USER_FUNC_START {
            return self.call_builtin(func_id, args);
        }

        let idx = match self.func_index.get(&func_id) {
            Some(&i) => i,
            None => {
                return Err(Signal::Return(Some(Value::Void)));
            }
        };

        // Clone body to avoid borrow issues
        let (params, body): (Vec<LocalId>, CoreExpr) = {
            let def: &FunctionDef = &self.module.functions[idx];
            (def.params.clone(), def.body.clone())
        };

        // Build frame: start with captured free vars, then bind params
        let mut frame = captured;
        for (param, arg) in params.iter().zip(args) {
            frame.insert(*param, arg);
        }

        match self.eval(&body, &mut frame) {
            Ok(v) => Ok(v),
            Err(Signal::Return(Some(v))) => Ok(v),
            Err(Signal::Return(None)) => Ok(Value::Void),
            Err(sig) => Err(sig), // Break/Continue propagate (shouldn't reach here)
        }
    }

    // -----------------------------------------------------------------------
    // Eval dispatch
    // -----------------------------------------------------------------------

    fn eval(&mut self, expr: &CoreExpr, frame: &mut Frame) -> EvalResult {
        use CoreExprKind::*;
        match &expr.kind.clone() {
            LitInt(n) => Ok(Value::Int(*n)),
            LitFloat(f) => Ok(Value::Float(*f)),
            LitBool(b) => Ok(Value::Bool(*b)),
            LitStr(s) => Ok(Value::Str(s.clone())),
            LitVoid => Ok(Value::Void),

            Local(id) => {
                match frame.get(id) {
                    Some(v) => Ok(v.clone()),
                    None => panic!("interpreter bug: undefined local {:?}", id),
                }
            }

            GlobalFunc(func_id) => {
                Ok(Value::Closure(*func_id, HashMap::new()))
            }

            MakeClosure { func_id, free_vars } => {
                let mut captured = HashMap::new();
                for &local_id in free_vars {
                    if let Some(v) = frame.get(&local_id) {
                        captured.insert(local_id, v.clone());
                    }
                }
                Ok(Value::Closure(*func_id, captured))
            }

            Let { local, value, body } => {
                let v = self.eval(value, frame)?;
                frame.insert(*local, v);
                self.eval(body, frame)
            }

            Assign { local, value } => {
                let v = self.eval(value, frame)?;
                frame.insert(*local, v);
                Ok(Value::Void)
            }

            BinOp { op, left, right } => {
                self.eval_binop(*op, left, right, frame)
            }

            CoreExprKind::UnOp { op, expr: inner } => {
                let v = self.eval(inner, frame)?;
                Ok(match op {
                    AstUnOp::Neg => match v {
                        Value::Int(n) => Value::Int(-n),
                        Value::Float(f) => Value::Float(-f),
                        _ => panic!("type error: neg on non-numeric"),
                    },
                    AstUnOp::Not => match v {
                        Value::Bool(b) => Value::Bool(!b),
                        _ => panic!("type error: not on non-bool"),
                    },
                })
            }

            Call { callee, args } => {
                let callee_val = self.eval(callee, frame)?;
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval(a, frame)?);
                }
                match callee_val {
                    Value::Closure(func_id, captured) => {
                        self.call_func(func_id, arg_vals, captured)
                    }
                    _ => panic!("type error: call on non-closure {:?}", callee_val),
                }
            }

            If { cond, then_branch, else_branch } => {
                let cond_val = self.eval(cond, frame)?;
                match cond_val {
                    Value::Bool(true) => self.eval(then_branch, frame),
                    Value::Bool(false) => self.eval(else_branch, frame),
                    _ => panic!("type error: if condition is not bool"),
                }
            }

            Match { scrutinee, arms } => {
                let scrut_val = self.eval(scrutinee, frame)?;
                for arm in arms {
                    let mut arm_frame = frame.clone();
                    if match_pattern(&arm.pattern, &scrut_val, &mut arm_frame) {
                        // Apply bindings from match to outer frame
                        *frame = arm_frame.clone();
                        return self.eval(&arm.body.clone(), frame);
                    }
                }
                panic!("non-exhaustive match — no arm matched {:?}", scrut_val)
            }

            Loop { body } => {
                loop {
                    let body_clone = body.clone();
                    match self.eval(&body_clone, frame) {
                        Ok(_) | Err(Signal::Continue) => { /* continue loop */ }
                        Err(Signal::Break(v)) => return Ok(v.unwrap_or(Value::Void)),
                        Err(Signal::Return(v)) => return Err(Signal::Return(v)),
                    }
                }
            }

            Break { value } => {
                let v = match value {
                    Some(e) => Some(self.eval(e, frame)?),
                    None => None,
                };
                Err(Signal::Break(v))
            }

            Continue => Err(Signal::Continue),

            Return { value } => {
                let v = match value {
                    Some(e) => Some(self.eval(e, frame)?),
                    None => None,
                };
                Err(Signal::Return(v))
            }

            Record { type_id, fields } => {
                // Fields are already sorted by FieldId; eval in order
                let mut vals = vec![Value::Void; fields.len()];
                for (field_id, field_expr) in fields {
                    vals[field_id.0] = self.eval(field_expr, frame)?;
                }
                Ok(Value::Record(*type_id, vals))
            }

            RecordGet { target, field } => {
                let record_val = self.eval(target, frame)?;
                match record_val {
                    Value::Record(_, fields) => Ok(fields[field.0].clone()),
                    _ => panic!("type error: record-get on non-record"),
                }
            }

            RecordUpdate { base, field, value } => {
                let record_val = self.eval(base, frame)?;
                let new_val = self.eval(value, frame)?;
                match record_val {
                    Value::Record(type_id, mut fields) => {
                        fields[field.0] = new_val;
                        Ok(Value::Record(type_id, fields))
                    }
                    _ => panic!("type error: record-update on non-record"),
                }
            }

            Variant { type_id, variant, args } => {
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.eval(a, frame)?);
                }
                Ok(Value::Variant(*type_id, variant.0, arg_vals))
            }

            ArrayLit { elements } => {
                let mut vals = Vec::new();
                for e in elements {
                    vals.push(self.eval(e, frame)?);
                }
                Ok(Value::Arr(vals))
            }

            Index { base, index } => {
                let base_val = self.eval(base, frame)?;
                let idx_val = self.eval(index, frame)?;
                match (base_val, idx_val) {
                    (Value::Arr(elems), Value::Int(i)) => {
                        let i = i as usize;
                        if i >= elems.len() {
                            panic!("array index out of bounds: {} >= {}", i, elems.len());
                        }
                        Ok(elems[i].clone())
                    }
                    (Value::Dict(_), _) => {
                        unreachable!("dict[key] should be rewritten to DICT_GET by the lowerer")
                    }
                    (base, idx) => panic!("type error: index on {:?} with {:?}", base, idx),
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Binary operators
    // -----------------------------------------------------------------------

    fn eval_binop(&mut self, op: BinOp, left: &CoreExpr, right: &CoreExpr, frame: &mut Frame) -> EvalResult {
        // Short-circuit logical operators
        if op == BinOp::And {
            let lv = self.eval(left, frame)?;
            return match lv {
                Value::Bool(false) => Ok(Value::Bool(false)),
                Value::Bool(true) => self.eval(right, frame),
                _ => panic!("type error: && on non-bool"),
            };
        }
        if op == BinOp::Or {
            let lv = self.eval(left, frame)?;
            return match lv {
                Value::Bool(true) => Ok(Value::Bool(true)),
                Value::Bool(false) => self.eval(right, frame),
                _ => panic!("type error: || on non-bool"),
            };
        }

        let lv = self.eval(left, frame)?;
        let rv = self.eval(right, frame)?;
        Ok(match (op, lv, rv) {
            // Int arithmetic
            (BinOp::Add, Value::Int(a), Value::Int(b)) => Value::Int(a + b),
            (BinOp::Sub, Value::Int(a), Value::Int(b)) => Value::Int(a - b),
            (BinOp::Mul, Value::Int(a), Value::Int(b)) => Value::Int(a * b),
            (BinOp::Div, Value::Int(a), Value::Int(b)) => {
                if b == 0 { panic!("division by zero"); }
                Value::Int(a / b)
            }
            (BinOp::Mod, Value::Int(a), Value::Int(b)) => {
                if b == 0 { panic!("modulo by zero"); }
                Value::Int(a % b)
            }

            // Float arithmetic
            (BinOp::Add, Value::Float(a), Value::Float(b)) => Value::Float(a + b),
            (BinOp::Sub, Value::Float(a), Value::Float(b)) => Value::Float(a - b),
            (BinOp::Mul, Value::Float(a), Value::Float(b)) => Value::Float(a * b),
            (BinOp::Div, Value::Float(a), Value::Float(b)) => Value::Float(a / b),
            (BinOp::Mod, Value::Float(a), Value::Float(b)) => Value::Float(a % b),

            // Comparison
            (BinOp::Eq, a, b) => Value::Bool(a == b),
            (BinOp::Ne, a, b) => Value::Bool(a != b),
            (BinOp::Lt, Value::Int(a), Value::Int(b)) => Value::Bool(a < b),
            (BinOp::Le, Value::Int(a), Value::Int(b)) => Value::Bool(a <= b),
            (BinOp::Gt, Value::Int(a), Value::Int(b)) => Value::Bool(a > b),
            (BinOp::Ge, Value::Int(a), Value::Int(b)) => Value::Bool(a >= b),
            (BinOp::Lt, Value::Float(a), Value::Float(b)) => Value::Bool(a < b),
            (BinOp::Le, Value::Float(a), Value::Float(b)) => Value::Bool(a <= b),
            (BinOp::Gt, Value::Float(a), Value::Float(b)) => Value::Bool(a > b),
            (BinOp::Ge, Value::Float(a), Value::Float(b)) => Value::Bool(a >= b),

            (op, a, b) => panic!("type error: {:?} on {:?} and {:?}", op, a, b),
        })
    }

    // -----------------------------------------------------------------------
    // Built-in functions (FuncId 1–35; USER_FUNC_START=36)
    // -----------------------------------------------------------------------

    fn call_builtin(&mut self, func_id: FuncId, args: Vec<Value>) -> EvalResult {
        match func_id.0 {
            1 => {
                // print(s: String)
                let s = args_to_string(&args, 0);
                write!(self.output, "{}", s).ok();
                Ok(Value::Void)
            }
            2 => {
                // println(s: String)
                let s = args_to_string(&args, 0);
                writeln!(self.output, "{}", s).ok();
                Ok(Value::Void)
            }
            3 => {
                // error(s: String)
                let s = args_to_string(&args, 0);
                panic!("error: {}", s);
            }
            4 => {
                // int_to_string(n: Int) String
                match &args[0] {
                    Value::Int(n) => Ok(Value::Str(n.to_string())),
                    _ => panic!("int_to_string: expected Int"),
                }
            }
            5 => {
                // float_to_string(f: Float) String
                match &args[0] {
                    Value::Float(f) => Ok(Value::Str(format_float(*f))),
                    _ => panic!("float_to_string: expected Float"),
                }
            }
            6 => {
                // bool_to_string(b: Bool) String
                match &args[0] {
                    Value::Bool(b) => Ok(Value::Str(b.to_string())),
                    _ => panic!("bool_to_string: expected Bool"),
                }
            }
            7 => {
                // string_to_string(s: String) String  — identity
                match args.into_iter().next() {
                    Some(v @ Value::Str(_)) => Ok(v),
                    _ => panic!("string_to_string: expected String"),
                }
            }
            8 => {
                // string_len(s: String) Int
                match &args[0] {
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    _ => panic!("string_len: expected String"),
                }
            }
            9 => {
                // string_concat(a: String, b: String) String
                match (&args[0], &args[1]) {
                    (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                    _ => panic!("string_concat: expected two Strings"),
                }
            }
            10 => {
                // array_len(arr: array<T>) Int
                match &args[0] {
                    Value::Arr(elems) => Ok(Value::Int(elems.len() as i64)),
                    _ => panic!("array_len: expected Array"),
                }
            }
            11 => {
                // array_append(arr: array<T>, elem: T) array<T>
                match args[0].clone() {
                    Value::Arr(mut elems) => {
                        elems.push(args[1].clone());
                        Ok(Value::Arr(elems))
                    }
                    _ => panic!("array_append: expected Array"),
                }
            }
            12 => {
                // array_set(arr: array<T>, idx: Int, val: T) array<T>
                match (args[0].clone(), &args[1], args[2].clone()) {
                    (Value::Arr(mut elems), Value::Int(i), val) => {
                        let i = *i as usize;
                        if i >= elems.len() {
                            panic!("array_set: index {} out of bounds (len {})", i, elems.len());
                        }
                        elems[i] = val;
                        Ok(Value::Arr(elems))
                    }
                    _ => panic!("array_set: wrong argument types"),
                }
            }
            13 => {
                // dict_set(m: dict<K,V>, k: K, v: V) dict<K,V>
                match args[0].clone() {
                    Value::Dict(mut pairs) => {
                        let k = args[1].clone();
                        let v = args[2].clone();
                        // Update existing or insert
                        let mut found = false;
                        for (ek, ev) in &mut pairs {
                            if ek == &k {
                                *ev = v.clone();
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            pairs.push((k, v));
                        }
                        Ok(Value::Dict(pairs))
                    }
                    _ => panic!("dict_set: expected Dict"),
                }
            }
            14 => {
                // dict_keys(m: dict<K,V>) array<K>
                match &args[0] {
                    Value::Dict(pairs) => {
                        Ok(Value::Arr(pairs.iter().map(|(k, _)| k.clone()).collect()))
                    }
                    _ => panic!("dict_keys: expected Dict"),
                }
            }
            15 => {
                // range_from(start: Int, end: Int) Range
                match (&args[0], &args[1]) {
                    (Value::Int(start), Value::Int(end)) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(*start), Value::Int(*end), Value::Int(1)],
                    )),
                    _ => panic!("range_from: expected two Ints"),
                }
            }
            16 => {
                // range(n: Int) Range  — [0, n)
                match &args[0] {
                    Value::Int(n) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(0), Value::Int(*n), Value::Int(1)],
                    )),
                    _ => panic!("range: expected Int"),
                }
            }
            17 => {
                // Cell.new(value: T) Cell<T>
                Ok(Value::Cell(Rc::new(RefCell::new(args.into_iter().next().unwrap_or(Value::Void)))))
            }
            18 => {
                // Cell.get(cell: Cell<T>) T
                match &args[0] {
                    Value::Cell(inner) => Ok(inner.borrow().clone()),
                    _ => panic!("Cell.get: expected Cell"),
                }
            }
            19 => {
                // Cell.set(cell: Cell<T>, value: T) Void
                match &args[0] {
                    Value::Cell(inner) => {
                        *inner.borrow_mut() = args[1].clone();
                        Ok(Value::Void)
                    }
                    _ => panic!("Cell.set: expected Cell"),
                }
            }
            20 => {
                // Cell.update(cell: Cell<T>, f: fn(T) T) Void
                match (&args[0], &args[1]) {
                    (Value::Cell(inner), Value::Closure(func_id, captured)) => {
                        let old_val = inner.borrow().clone();
                        let func_id = *func_id;
                        let captured = captured.clone();
                        let new_val = self.call_func(func_id, vec![old_val], captured)?;
                        *inner.borrow_mut() = new_val;
                        Ok(Value::Void)
                    }
                    _ => panic!("Cell.update: expected Cell and Closure"),
                }
            }
            21 => {
                // dict_get(m: Dict<K,V>, k: K) Option<V>
                match args[0].clone() {
                    Value::Dict(pairs) => {
                        let key = &args[1];
                        for (k, v) in &pairs {
                            if k == key {
                                // Some(v) — variant index 1
                                return Ok(Value::Variant(OPTION_TYPE_ID, 1, vec![v.clone()]));
                            }
                        }
                        // None — variant index 0
                        Ok(Value::Variant(OPTION_TYPE_ID, 0, vec![]))
                    }
                    _ => panic!("dict_get: expected Dict"),
                }
            }
            22 => {
                // Dict.new() Dict<K,V>
                Ok(Value::Dict(vec![]))
            }
            23 => {
                // range_step(start: Int, end: Int, step: Int) Range
                match (&args[0], &args[1], &args[2]) {
                    (Value::Int(start), Value::Int(end), Value::Int(step)) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(*start), Value::Int(*end), Value::Int(*step)],
                    )),
                    _ => panic!("range_step: expected three Ints"),
                }
            }
            24 => {
                // dict_get_unsafe(m: Dict<K,V>, k: K) V  — internal use by for-loop lowering
                match args[0].clone() {
                    Value::Dict(pairs) => {
                        let key = &args[1];
                        for (k, v) in &pairs {
                            if k == key {
                                return Ok(v.clone());
                            }
                        }
                        panic!("dict_get_unsafe: key not found (internal error)");
                    }
                    _ => panic!("dict_get_unsafe: expected Dict"),
                }
            }
            25 => {
                // Array.concat(a, b) -> Array<T>
                match (args[0].clone(), args[1].clone()) {
                    (Value::Arr(mut a), Value::Arr(b)) => {
                        a.extend(b);
                        Ok(Value::Arr(a))
                    }
                    _ => panic!("array_concat: expected two Arrays"),
                }
            }
            26 => {
                // Array.slice(arr, start, end) -> Array<T>
                match (&args[0], &args[1], &args[2]) {
                    (Value::Arr(elems), Value::Int(s), Value::Int(e)) => {
                        let s = (*s as usize).min(elems.len());
                        let e = (*e as usize).min(elems.len()).max(s);
                        Ok(Value::Arr(elems[s..e].to_vec()))
                    }
                    _ => panic!("array_slice: expected Array and two Ints"),
                }
            }
            27 => {
                // Dict.len(m) -> Int
                match &args[0] {
                    Value::Dict(pairs) => Ok(Value::Int(pairs.len() as i64)),
                    _ => panic!("dict_len: expected Dict"),
                }
            }
            28 => {
                // Dict.has(m, k) -> Bool
                match &args[0] {
                    Value::Dict(pairs) => {
                        let found = pairs.iter().any(|(k, _)| k == &args[1]);
                        Ok(Value::Bool(found))
                    }
                    _ => panic!("dict_has: expected Dict"),
                }
            }
            29 => {
                // Dict.remove(m, k) -> Dict<K,V>
                match args[0].clone() {
                    Value::Dict(mut pairs) => {
                        pairs.retain(|(k, _)| k != &args[1]);
                        Ok(Value::Dict(pairs))
                    }
                    _ => panic!("dict_remove: expected Dict"),
                }
            }
            30 => {
                // String.substring(s, start, end) -> String
                match (&args[0], &args[1], &args[2]) {
                    (Value::Str(s), Value::Int(start), Value::Int(end)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let s = (*start as usize).min(chars.len());
                        let e = (*end as usize).min(chars.len()).max(s);
                        Ok(Value::Str(chars[s..e].iter().collect()))
                    }
                    _ => panic!("string_substring: expected String and two Ints"),
                }
            }
            31 => {
                // Iterator.next(it: Iterator<T>) Option<IterItem<T>>
                let iter_state = match &args[0] {
                    Value::Iterator(s) => s.clone(),
                    _ => panic!("Iterator.next: expected Iterator"),
                };
                let step_result = match iter_state.step.as_ref() {
                    Value::Closure(func_id, captured) => {
                        let fid = *func_id;
                        let cap = captured.clone();
                        self.call_func(fid, vec![iter_state.seed.as_ref().clone()], cap)?
                    }
                    _ => panic!("Iterator.next: step must be a Closure"),
                };
                match step_result {
                    // Done (variant 0) → None
                    Value::Variant(tid, 0, _) if tid == UNFOLD_STEP_TYPE_ID => {
                        Ok(Value::Variant(OPTION_TYPE_ID, 0, vec![]))
                    }
                    // Yield(value, next_seed) (variant 1) → Some(IterItem { value, rest })
                    Value::Variant(tid, 1, payload) if tid == UNFOLD_STEP_TYPE_ID => {
                        let yielded   = payload[0].clone();
                        let next_seed = payload[1].clone();
                        let next_iter = Value::Iterator(Rc::new(IteratorState {
                            seed: Box::new(next_seed),
                            step: iter_state.step.clone(),
                        }));
                        let item = Value::Record(ITER_ITEM_TYPE_ID, vec![yielded, next_iter]);
                        Ok(Value::Variant(OPTION_TYPE_ID, 1, vec![item]))
                    }
                    other => panic!("Iterator.next: unexpected step result {:?}", other),
                }
            }
            32 => {
                // Iterator.unfold(seed: S, step: fn(S) UnfoldStep<T,S>) Iterator<T>
                Ok(Value::Iterator(Rc::new(IteratorState {
                    seed: Box::new(args[0].clone()),
                    step: Box::new(args[1].clone()),
                })))
            }
            33 => {
                // ARRAY_BUILDER_NEW() -> Cell<Array<T>>
                Ok(Value::Cell(Rc::new(RefCell::new(Value::Arr(vec![])))))
            }
            34 => {
                // ARRAY_BUILDER_PUSH(builder, elem) -> Void
                let cell = match &args[0] {
                    Value::Cell(c) => c.clone(),
                    _ => panic!("ARRAY_BUILDER_PUSH: expected Cell"),
                };
                let elem = args[1].clone();
                if let Value::Arr(ref mut vec) = *cell.borrow_mut() {
                    vec.push(elem);
                } else {
                    panic!("ARRAY_BUILDER_PUSH: cell does not contain an Array");
                }
                Ok(Value::Void)
            }
            35 => {
                // ARRAY_BUILDER_FREEZE(builder) -> Array<T>
                let cell = match &args[0] {
                    Value::Cell(c) => c.clone(),
                    _ => panic!("ARRAY_BUILDER_FREEZE: expected Cell"),
                };
                Ok(cell.borrow().clone())
            }
            _ => panic!("unknown builtin FuncId({})", func_id.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern matching helper
// ---------------------------------------------------------------------------

fn match_pattern(pattern: &CorePattern, value: &Value, frame: &mut Frame) -> bool {
    match (pattern, value) {
        (CorePattern::Wildcard, _) => true,
        (CorePattern::Var(id), v) => {
            frame.insert(*id, v.clone());
            true
        }
        (CorePattern::LitInt(n), Value::Int(m)) => n == m,
        (CorePattern::LitBool(b), Value::Bool(c)) => b == c,
        (CorePattern::LitStr(s), Value::Str(t)) => s == t,
        (CorePattern::Variant { type_id, variant, fields }, Value::Variant(vt, vi, vargs)) => {
            type_id == vt
                && variant.0 == *vi
                && fields.len() == vargs.len()
                && fields.iter().zip(vargs).all(|(p, v)| match_pattern(p, v, frame))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn args_to_string(args: &[Value], idx: usize) -> String {
    match args.get(idx) {
        Some(Value::Str(s)) => s.clone(),
        Some(v) => format!("{}", v),
        None => String::new(),
    }
}

fn format_float(f: f64) -> String {
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

// ---------------------------------------------------------------------------
// Unused MatchArm import suppression
// ---------------------------------------------------------------------------
const _: fn() = || {
    let _: &MatchArm;
};
