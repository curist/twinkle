use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use super::value::IteratorState;
use crate::ir::CoreModule;
use crate::ir::core::{
    CoreExpr, CoreExprKind, CorePattern, FuncId, FunctionDef, LocalId, MatchArm,
};
use crate::ir::lower::prelude as prelude_ids;
use crate::syntax::ast::BinOp;
use crate::syntax::ast::UnOp as AstUnOp;
use crate::types::ty::{
    ITER_ITEM_TYPE_ID, OPTION_TYPE_ID, RANGE_TYPE_ID, UNFOLD_STEP_TYPE_ID,
};

use super::value::Value;

// ---------------------------------------------------------------------------
// Frame type alias
// ---------------------------------------------------------------------------

type Frame = HashMap<LocalId, Value>;

// ---------------------------------------------------------------------------
// Trap errors (unrecoverable runtime faults)
// ---------------------------------------------------------------------------

/// A user-visible runtime fault that aborts execution.
/// Distinct from interpreter bugs, which remain as `panic!`.
#[derive(Debug)]
pub enum TrapError {
    ArrayIndexOutOfBounds { index: usize, len: usize },
    DivisionByZero,
    ModuloByZero,
    UserError(String),
}

impl std::fmt::Display for TrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrapError::ArrayIndexOutOfBounds { index, len } => {
                write!(f, "array index out of bounds: {} >= {}", index, len)
            }
            TrapError::DivisionByZero => write!(f, "division by zero"),
            TrapError::ModuloByZero => write!(f, "modulo by zero"),
            TrapError::UserError(msg) => write!(f, "{}", msg),
        }
    }
}

// ---------------------------------------------------------------------------
// Control-flow signals
// ---------------------------------------------------------------------------

/// Non-local exits that propagate through the call stack.
enum Signal {
    Break(Option<Value>),
    Continue,
    Return(Option<Value>),
    Trap(TrapError),
}

type EvalResult = Result<Value, Signal>;

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

pub struct Interpreter<W: Write = Box<dyn Write>> {
    module: CoreModule,
    func_index: HashMap<FuncId, usize>,
    output: W,
    error_output: Vec<u8>,
    /// Module-level globals populated during __init__ execution.
    globals: Frame,
    /// True while directly executing the __init__ body (not inside nested calls).
    in_init_frame: bool,
    /// Defer scope stack. Each entry is a scope (function call or loop iteration).
    /// Entries are (deferred_expr, captured_frame_snapshot) pairs in declaration order.
    /// Drained in reverse (LIFO) on scope exit. Traps do NOT drain defers.
    defer_stack: Vec<Vec<(CoreExpr, Frame)>>,
}

impl<W: Write> Interpreter<W> {
    pub fn new(module: CoreModule, output: W) -> Self {
        let func_index = module
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.func_id, i))
            .collect();
        Self {
            module,
            func_index,
            output,
            error_output: Vec::new(),
            globals: HashMap::new(),
            in_init_frame: false,
            defer_stack: Vec::new(),
        }
    }

    /// Consume the interpreter and return the underlying output sink.
    /// Useful in tests to inspect captured bytes.
    pub fn into_output(self) -> W {
        self.output
    }

    /// Return the captured stderr bytes.
    pub fn error_output(&self) -> &[u8] {
        &self.error_output
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        // Run all module __init__ functions in dependency order (imported modules first,
        // entry module last). This ensures cross-module globals are available when needed.
        for id in self.module.all_init_func_ids.clone() {
            match self.run_init(id) {
                Ok(_) => {}
                Err(Signal::Trap(t)) => return Err(anyhow::anyhow!("{}", t)),
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "top-level execution failed with unhandled signal"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Execute the __init__ function with globals tracking enabled.
    fn run_init(&mut self, func_id: FuncId) -> EvalResult {
        let idx = match self.func_index.get(&func_id) {
            Some(&i) => i,
            None => return Ok(Value::Void),
        };
        let body = self.module.functions[idx].body.clone();
        let mut frame: Frame = HashMap::new();
        self.in_init_frame = true;
        self.defer_stack.push(Vec::new());
        let result = match self.eval(&body, &mut frame) {
            Ok(v) => Ok(v),
            Err(Signal::Return(Some(v))) => Ok(v),
            Err(Signal::Return(None)) => Ok(Value::Void),
            Err(sig) => Err(sig),
        };
        self.in_init_frame = false;
        let scope = self.defer_stack.pop().unwrap_or_default();
        if !matches!(result, Err(Signal::Trap(_))) {
            for (deferred_expr, mut cap) in scope.into_iter().rev() {
                let _ = self.eval(&deferred_expr, &mut cap);
            }
        }
        result
    }

    // -----------------------------------------------------------------------
    // Function calls
    // -----------------------------------------------------------------------

    fn call_func(&mut self, func_id: FuncId, args: Vec<Value>, captured: Frame) -> EvalResult {
        // Prefer user-defined functions when present; otherwise dispatch to
        // prelude/host builtins (including high IDs like __host_args).
        let idx = match self.func_index.get(&func_id) {
            Some(&i) => i,
            None => return self.call_builtin(func_id, args),
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

        // Nested calls are never the init frame
        let saved_in_init = std::mem::replace(&mut self.in_init_frame, false);
        self.defer_stack.push(Vec::new());
        let result = match self.eval(&body, &mut frame) {
            Ok(v) => Ok(v),
            Err(Signal::Return(Some(v))) => Ok(v),
            Err(Signal::Return(None)) => Ok(Value::Void),
            Err(sig) => Err(sig),
        };
        self.in_init_frame = saved_in_init;
        let scope = self.defer_stack.pop().unwrap_or_default();
        if !matches!(result, Err(Signal::Trap(_))) {
            for (deferred_expr, mut cap) in scope.into_iter().rev() {
                let _ = self.eval(&deferred_expr, &mut cap);
            }
        }
        result
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
                if let Some(v) = frame.get(id) {
                    Ok(v.clone())
                } else if let Some(v) = self.globals.get(id) {
                    Ok(v.clone())
                } else {
                    panic!("interpreter bug: undefined local {:?}", id)
                }
            }

            GlobalLocal(id) => match self.globals.get(id) {
                Some(v) => Ok(v.clone()),
                None => panic!("interpreter bug: uninitialized module global {:?}", id),
            },

            GlobalFunc(func_id) => Ok(Value::Closure(*func_id, HashMap::new())),

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
                frame.insert(*local, v.clone());
                if self.in_init_frame {
                    self.globals.insert(*local, v);
                }
                self.eval(body, frame)
            }

            Assign { local, value } => {
                let v = self.eval(value, frame)?;
                frame.insert(*local, v.clone());
                if self.in_init_frame {
                    self.globals.insert(*local, v);
                }
                Ok(Value::Void)
            }

            BinOp { op, left, right } => self.eval_binop(*op, left, right, frame),

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

            If {
                cond,
                then_branch,
                else_branch,
            } => {
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
                    // Each iteration gets its own defer scope.
                    self.defer_stack.push(Vec::new());
                    let body_clone = body.clone();
                    let iter_result = self.eval(&body_clone, frame);
                    let scope = self.defer_stack.pop().unwrap_or_default();
                    match iter_result {
                        Ok(_) | Err(Signal::Continue) => {
                            // Normal or continue: drain iteration defers, then keep looping.
                            for (d, mut cap) in scope.into_iter().rev() {
                                let _ = self.eval(&d, &mut cap);
                            }
                        }
                        Err(Signal::Break(v)) => {
                            // Break: drain iteration defers, then exit loop.
                            for (d, mut cap) in scope.into_iter().rev() {
                                let _ = self.eval(&d, &mut cap);
                            }
                            return Ok(v.unwrap_or(Value::Void));
                        }
                        Err(sig @ Signal::Return(_)) => {
                            // Return: drain iteration defers; call_func will drain fn scope.
                            for (d, mut cap) in scope.into_iter().rev() {
                                let _ = self.eval(&d, &mut cap);
                            }
                            return Err(sig);
                        }
                        Err(sig @ Signal::Trap(_)) => {
                            // Trap: do NOT drain defers; just propagate.
                            return Err(sig);
                        }
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

            Defer(inner_expr) => {
                // Register the deferred expression in the innermost scope, capturing
                // the current frame by value (capture-by-value semantics).
                // Deep-clone so that Cell values capture their current contents,
                // not a shared Rc pointer that would reflect later mutations.
                let captured: Frame = frame.iter().map(|(k, v)| (*k, v.deep_clone())).collect();
                self.defer_stack
                    .last_mut()
                    .expect("interpreter bug: Defer evaluated with no active defer scope")
                    .push((*inner_expr.clone(), captured));
                Ok(Value::Void)
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

            Variant {
                type_id,
                variant,
                args,
            } => {
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
                Ok(Value::Vec(vals))
            }

            Index { base, index } => {
                let base_val = self.eval(base, frame)?;
                let idx_val = self.eval(index, frame)?;
                match (base_val, idx_val) {
                    (Value::Vec(elems), Value::Int(i)) => {
                        let i = i as usize;
                        if i >= elems.len() {
                            return Err(Signal::Trap(TrapError::ArrayIndexOutOfBounds {
                                index: i,
                                len: elems.len(),
                            }));
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

    fn eval_binop(
        &mut self,
        op: BinOp,
        left: &CoreExpr,
        right: &CoreExpr,
        frame: &mut Frame,
    ) -> EvalResult {
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
                if b == 0 {
                    return Err(Signal::Trap(TrapError::DivisionByZero));
                }
                Value::Int(a / b)
            }
            (BinOp::Mod, Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    return Err(Signal::Trap(TrapError::ModuloByZero));
                }
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
    // Built-in functions (fixed prelude IDs + extended host bridge IDs)
    // -----------------------------------------------------------------------

    fn call_builtin(&mut self, func_id: FuncId, args: Vec<Value>) -> EvalResult {
        match func_id {
            prelude_ids::PRINT => {
                // print(s: String)
                let s = args_to_string(&args, 0);
                write!(self.output, "{}", s).ok();
                Ok(Value::Void)
            }
            prelude_ids::PRINTLN => {
                // println(s: String)
                let s = args_to_string(&args, 0);
                writeln!(self.output, "{}", s).ok();
                Ok(Value::Void)
            }
            prelude_ids::ERROR => {
                // error(s: String)
                let s = args_to_string(&args, 0);
                return Err(Signal::Trap(TrapError::UserError(s)));
            }
            prelude_ids::EPRINT => {
                // eprint(s: String)
                let s = args_to_string(&args, 0);
                write!(self.error_output, "{}", s).ok();
                Ok(Value::Void)
            }
            prelude_ids::EPRINTLN => {
                // eprintln(s: String)
                let s = args_to_string(&args, 0);
                writeln!(self.error_output, "{}", s).ok();
                Ok(Value::Void)
            }
            prelude_ids::INT_TO_STRING => {
                // int_to_string(n: Int) String
                match &args[0] {
                    Value::Int(n) => Ok(Value::Str(n.to_string())),
                    _ => panic!("int_to_string: expected Int"),
                }
            }
            prelude_ids::FLOAT_TO_STRING => {
                // float_to_string(f: Float) String
                match &args[0] {
                    Value::Float(f) => Ok(Value::Str(format_float(*f))),
                    _ => panic!("float_to_string: expected Float"),
                }
            }
            prelude_ids::BOOL_TO_STRING => {
                // bool_to_string(b: Bool) String
                match &args[0] {
                    Value::Bool(b) => Ok(Value::Str(b.to_string())),
                    _ => panic!("bool_to_string: expected Bool"),
                }
            }
            prelude_ids::STRING_TO_STRING => {
                // string_to_string(s: String) String  — identity
                match args.into_iter().next() {
                    Some(v @ Value::Str(_)) => Ok(v),
                    _ => panic!("string_to_string: expected String"),
                }
            }
            prelude_ids::STRING_LEN => {
                // string_len(s: String) Int
                match &args[0] {
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    _ => panic!("string_len: expected String"),
                }
            }
            prelude_ids::STRING_CONCAT => {
                // string_concat(a: String, b: String) String
                match (&args[0], &args[1]) {
                    (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                    _ => panic!("string_concat: expected two Strings"),
                }
            }
            prelude_ids::VECTOR_LEN => {
                // vector_len(vec: Vector<T>) Int
                match &args[0] {
                    Value::Vec(elems) => Ok(Value::Int(elems.len() as i64)),
                    _ => panic!("vector_len: expected Vector"),
                }
            }
            prelude_ids::VECTOR_PUSH => {
                // vector_push(vec: Vector<T>, elem: T) Vector<T>
                match args[0].clone() {
                    Value::Vec(mut elems) => {
                        elems.push(args[1].clone());
                        Ok(Value::Vec(elems))
                    }
                    _ => panic!("vector_push: expected Vector"),
                }
            }
            prelude_ids::VECTOR_SET_UNSAFE => {
                // vector_set_unsafe(vec: Vector<T>, idx: Int, val: T) Vector<T>
                match (args[0].clone(), &args[1], args[2].clone()) {
                    (Value::Vec(mut elems), Value::Int(i), val) => {
                        let i = *i as usize;
                        if i >= elems.len() {
                            return Err(Signal::Trap(TrapError::ArrayIndexOutOfBounds {
                                index: i,
                                len: elems.len(),
                            }));
                        }
                        elems[i] = val;
                        Ok(Value::Vec(elems))
                    }
                    _ => panic!("vector_set_unsafe: wrong argument types"),
                }
            }
            prelude_ids::DICT_SET => {
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
            prelude_ids::DICT_KEYS => {
                // dict_keys(m: dict<K,V>) Vector<K>
                match &args[0] {
                    Value::Dict(pairs) => {
                        Ok(Value::Vec(pairs.iter().map(|(k, _)| k.clone()).collect()))
                    }
                    _ => panic!("dict_keys: expected Dict"),
                }
            }
            prelude_ids::RANGE_FROM => {
                // range_from(start: Int, end: Int) Range
                match (&args[0], &args[1]) {
                    (Value::Int(start), Value::Int(end)) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(*start), Value::Int(*end), Value::Int(1)],
                    )),
                    _ => panic!("range_from: expected two Ints"),
                }
            }
            prelude_ids::RANGE => {
                // range(n: Int) Range  — [0, n)
                match &args[0] {
                    Value::Int(n) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(0), Value::Int(*n), Value::Int(1)],
                    )),
                    _ => panic!("range: expected Int"),
                }
            }
            prelude_ids::CELL_NEW => {
                // Cell.new(value: T) Cell<T>
                Ok(Value::Cell(Rc::new(RefCell::new(
                    args.into_iter().next().unwrap_or(Value::Void),
                ))))
            }
            prelude_ids::CELL_GET => {
                // Cell.get(cell: Cell<T>) T
                match &args[0] {
                    Value::Cell(inner) => Ok(inner.borrow().clone()),
                    _ => panic!("Cell.get: expected Cell"),
                }
            }
            prelude_ids::CELL_SET => {
                // Cell.set(cell: Cell<T>, value: T) Void
                match &args[0] {
                    Value::Cell(inner) => {
                        *inner.borrow_mut() = args[1].clone();
                        Ok(Value::Void)
                    }
                    _ => panic!("Cell.set: expected Cell"),
                }
            }
            prelude_ids::CELL_UPDATE => {
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
            prelude_ids::DICT_GET => {
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
            prelude_ids::DICT_NEW => {
                // Dict.new() Dict<K,V>
                Ok(Value::Dict(vec![]))
            }
            prelude_ids::RANGE_STEP => {
                // range_step(start: Int, end: Int, step: Int) Range
                match (&args[0], &args[1], &args[2]) {
                    (Value::Int(start), Value::Int(end), Value::Int(step)) => Ok(Value::Record(
                        RANGE_TYPE_ID,
                        vec![Value::Int(*start), Value::Int(*end), Value::Int(*step)],
                    )),
                    _ => panic!("range_step: expected three Ints"),
                }
            }
            prelude_ids::DICT_GET_UNSAFE => {
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
            prelude_ids::VECTOR_CONCAT => {
                // Vector.concat(a, b) -> Vector<T>
                match (args[0].clone(), args[1].clone()) {
                    (Value::Vec(mut a), Value::Vec(b)) => {
                        a.extend(b);
                        Ok(Value::Vec(a))
                    }
                    _ => panic!("vector_concat: expected two Vectors"),
                }
            }
            prelude_ids::VECTOR_SLICE => {
                // Vector.slice(vec, start, end) -> Vector<T>
                match (&args[0], &args[1], &args[2]) {
                    (Value::Vec(elems), Value::Int(s), Value::Int(e)) => {
                        let s = (*s as usize).min(elems.len());
                        let e = (*e as usize).min(elems.len()).max(s);
                        Ok(Value::Vec(elems[s..e].to_vec()))
                    }
                    _ => panic!("vector_slice: expected Vector and two Ints"),
                }
            }
            prelude_ids::DICT_LEN => {
                // Dict.len(m) -> Int
                match &args[0] {
                    Value::Dict(pairs) => Ok(Value::Int(pairs.len() as i64)),
                    _ => panic!("dict_len: expected Dict"),
                }
            }
            prelude_ids::DICT_HAS => {
                // Dict.has(m, k) -> Bool
                match &args[0] {
                    Value::Dict(pairs) => {
                        let found = pairs.iter().any(|(k, _)| k == &args[1]);
                        Ok(Value::Bool(found))
                    }
                    _ => panic!("dict_has: expected Dict"),
                }
            }
            prelude_ids::DICT_REMOVE => {
                // Dict.remove(m, k) -> Dict<K,V>
                match args[0].clone() {
                    Value::Dict(mut pairs) => {
                        pairs.retain(|(k, _)| k != &args[1]);
                        Ok(Value::Dict(pairs))
                    }
                    _ => panic!("dict_remove: expected Dict"),
                }
            }
            prelude_ids::STRING_SUBSTR => {
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
            prelude_ids::ITERATOR_NEXT => {
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
                        let yielded = payload[0].clone();
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
            prelude_ids::ITERATOR_UNFOLD => {
                // Iterator.unfold(seed: S, step: fn(S) UnfoldStep<T,S>) Iterator<T>
                Ok(Value::Iterator(Rc::new(IteratorState {
                    seed: Box::new(args[0].clone()),
                    step: Box::new(args[1].clone()),
                })))
            }
            prelude_ids::VECTOR_BUILDER_NEW => {
                // VECTOR_BUILDER_NEW() -> Cell<Vector<T>>
                Ok(Value::Cell(Rc::new(RefCell::new(Value::Vec(vec![])))))
            }
            prelude_ids::VECTOR_BUILDER_PUSH => {
                // VECTOR_BUILDER_PUSH(builder, elem) -> Void
                let cell = match &args[0] {
                    Value::Cell(c) => c.clone(),
                    _ => panic!("VECTOR_BUILDER_PUSH: expected Cell"),
                };
                let elem = args[1].clone();
                if let Value::Vec(ref mut vec) = *cell.borrow_mut() {
                    vec.push(elem);
                } else {
                    panic!("VECTOR_BUILDER_PUSH: cell does not contain a Vector");
                }
                Ok(Value::Void)
            }
            prelude_ids::VECTOR_BUILDER_FREEZE => {
                // VECTOR_BUILDER_FREEZE(builder) -> Vector<T>
                let cell = match &args[0] {
                    Value::Cell(c) => c.clone(),
                    _ => panic!("VECTOR_BUILDER_FREEZE: expected Cell"),
                };
                Ok(cell.borrow().clone())
            }
            prelude_ids::VECTOR_GET => {
                // VECTOR_GET(vec: Vector<T>, i: Int) -> Option<T>  (safe)
                match (&args[0], &args[1]) {
                    (Value::Vec(elems), Value::Int(i)) => {
                        let idx = *i as usize;
                        if idx < elems.len() {
                            Ok(Value::Variant(OPTION_TYPE_ID, 1, vec![elems[idx].clone()]))
                        } else {
                            Ok(Value::Variant(OPTION_TYPE_ID, 0, vec![]))
                        }
                    }
                    _ => panic!("vector_get: wrong argument types"),
                }
            }
            prelude_ids::VECTOR_SET => {
                // VECTOR_SET(vec: Vector<T>, i: Int, val: T) -> Option<Vector<T>>  (safe)
                match (args[0].clone(), &args[1], args[2].clone()) {
                    (Value::Vec(mut elems), Value::Int(i), val) => {
                        let idx = *i as usize;
                        if idx < elems.len() {
                            elems[idx] = val;
                            Ok(Value::Variant(OPTION_TYPE_ID, 1, vec![Value::Vec(elems)]))
                        } else {
                            Ok(Value::Variant(OPTION_TYPE_ID, 0, vec![]))
                        }
                    }
                    _ => panic!("vector_set: wrong argument types"),
                }
            }
            prelude_ids::VECTOR_MAKE => {
                // VECTOR_MAKE(size: Int, fill: T) -> Vector<T>
                match (&args[0], args[1].clone()) {
                    (Value::Int(size), fill) => {
                        let n = (*size).max(0) as usize;
                        Ok(Value::Vec(vec![fill; n]))
                    }
                    _ => panic!("vector_make: expected Int size"),
                }
            }
            prelude_ids::VECTOR_SET_IN_PLACE => {
                // __vector_set_in_place(vec: Vector<T>, i: Int, val: T) -> Vector<T>
                // Internal collect optimization helper.
                match (args[0].clone(), &args[1], args[2].clone()) {
                    (Value::Vec(mut elems), Value::Int(i), val) => {
                        let idx = *i as usize;
                        if idx >= elems.len() {
                            return Err(Signal::Trap(TrapError::ArrayIndexOutOfBounds {
                                index: idx,
                                len: elems.len(),
                            }));
                        }
                        elems[idx] = val;
                        Ok(Value::Vec(elems))
                    }
                    _ => panic!("__vector_set_in_place: wrong argument types"),
                }
            }
            prelude_ids::VECTOR_BUILDER_FROM => {
                // __vector_builder_from(vec: Vector<T>) -> Cell<Vector<T>>
                // Internal uniqueness loop rewrite helper.
                match args[0].clone() {
                    Value::Vec(elems) => Ok(Value::Cell(Rc::new(RefCell::new(Value::Vec(elems))))),
                    _ => panic!("__vector_builder_from: expected Vector"),
                }
            }
            prelude_ids::DICT_SET_IN_PLACE => {
                // __dict_set_in_place(dict: Dict<K,V>, k: K, v: V) -> Dict<K,V>
                // Internal uniqueness rewrite helper.
                match args[0].clone() {
                    Value::Dict(mut pairs) => {
                        let k = args[1].clone();
                        let v = args[2].clone();
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
                    _ => panic!("__dict_set_in_place: expected Dict"),
                }
            }
            prelude_ids::DICT_REMOVE_IN_PLACE => {
                // __dict_remove_in_place(dict: Dict<K,V>, k: K) -> Dict<K,V>
                // Internal uniqueness rewrite helper.
                match args[0].clone() {
                    Value::Dict(mut pairs) => {
                        pairs.retain(|(k, _)| k != &args[1]);
                        Ok(Value::Dict(pairs))
                    }
                    _ => panic!("__dict_remove_in_place: expected Dict"),
                }
            }
            prelude_ids::HOST_ARGS => {
                // __host_args() -> Vector<String>
                let argv = std::env::args().map(Value::Str).collect::<Vec<_>>();
                Ok(Value::Vec(argv))
            }
            prelude_ids::HOST_ENV => {
                // __host_env(name: String) -> Vector<String> (0 or 1 value)
                let name = match &args[0] {
                    Value::Str(s) => s.clone(),
                    _ => panic!("__host_env: expected String name"),
                };
                let values = std::env::var(&name)
                    .ok()
                    .map(|v| vec![Value::Str(v)])
                    .unwrap_or_default();
                Ok(Value::Vec(values))
            }
            prelude_ids::HOST_CWD => {
                // __host_cwd() -> String
                let cwd = std::env::current_dir().map_err(|e| {
                    Signal::Trap(TrapError::UserError(format!("cwd lookup failed: {e}")))
                })?;
                Ok(Value::Str(path_to_logical(&cwd)))
            }
            prelude_ids::HOST_EXIT => {
                // __host_exit(code: Int) -> Never
                let code = match &args[0] {
                    Value::Int(i) => *i,
                    _ => panic!("__host_exit: expected Int exit code"),
                };
                return Err(Signal::Trap(TrapError::UserError(format!(
                    "host.exit({code})"
                ))));
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
        (
            CorePattern::Variant {
                type_id,
                variant,
                fields,
            },
            Value::Variant(vt, vi, vargs),
        ) => {
            type_id == vt
                && variant.0 == *vi
                && fields.len() == vargs.len()
                && fields
                    .iter()
                    .zip(vargs)
                    .all(|(p, v)| match_pattern(p, v, frame))
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

fn path_to_logical(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '/' {
        s.into_owned()
    } else {
        s.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

// ---------------------------------------------------------------------------
// Unused MatchArm import suppression
// ---------------------------------------------------------------------------
const _: fn() = || {
    let _: &MatchArm;
};
