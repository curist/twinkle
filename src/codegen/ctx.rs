use std::collections::{BTreeMap, HashMap};

use crate::codegen::prelude::{PreludeEntry, PreludeMap};
use crate::ir::FuncId;
use crate::ir::LocalId;
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom, OpKind};
use crate::ir::core::CorePattern;
use crate::runtime::types::{T_ARRAY, T_CLOSURE, T_DICT, T_STRING, T_VARIANT};
use crate::syntax::ast::{BinOp, UnOp};
use crate::types::env::TypeEnv;
use crate::types::ty::{
    CELL_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID, TypeDef, TypeId,
    UNFOLD_STEP_TYPE_ID,
};
use crate::wasm::ir::{FuncSym, HeapType, ImportDef, Label, ValType};

#[derive(Debug, Clone)]
pub struct FuncSigInfo {
    pub params: Vec<ValType>,
    pub result: Option<ValType>,
}

pub struct EmitCtx<'a> {
    pub local_map: HashMap<LocalId, (u32, ValType)>,
    /// Tracks `MonoType::Function` for locals that hold typed closures.
    /// Populated for function-typed params and for `AMakeClosure` let-bindings
    /// with concrete signatures. Used at call sites to select the typed
    /// `call_ref` path instead of the universal anyref boxing path.
    pub local_mono: HashMap<LocalId, MonoType>,
    /// Tracks local bindings created from `AMakeClosure` so direct user calls
    /// can materialize typed closures only at concrete higher-order boundaries.
    pub closure_locals: HashMap<LocalId, (FuncId, Vec<LocalId>)>,
    module_globals: HashMap<LocalId, String>,
    pub label_stack: Vec<(Label, Label)>,
    pub loop_result_stack: Vec<Option<ValType>>,
    next_label_id: u32,
    imports: BTreeMap<FuncSym, ImportDef>,
    pub type_env: &'a TypeEnv,
    pub prelude: &'a PreludeMap,
    user_funcs: &'a HashMap<FuncId, FuncSigInfo>,
    /// Functions with fully-concrete signatures that appear in `AMakeClosure`
    /// nodes.  Maps `func_id → (real_param_types, return_type)`.
    pub concrete_func_sigs: HashMap<FuncId, (Vec<MonoType>, MonoType)>,
}

impl<'a> EmitCtx<'a> {
    pub fn new(
        type_env: &'a TypeEnv,
        prelude: &'a PreludeMap,
        user_funcs: &'a HashMap<FuncId, FuncSigInfo>,
    ) -> Self {
        Self {
            local_map: HashMap::new(),
            local_mono: HashMap::new(),
            closure_locals: HashMap::new(),
            module_globals: HashMap::new(),
            label_stack: Vec::new(),
            loop_result_stack: Vec::new(),
            next_label_id: 0,
            imports: BTreeMap::new(),
            type_env,
            prelude,
            user_funcs,
            concrete_func_sigs: HashMap::new(),
        }
    }

    /// Install the concrete-function-signature map for Stage 9.6 typed
    /// closure emission.  Must be called before any local setup or emission.
    pub fn set_concrete_func_sigs(&mut self, sigs: HashMap<FuncId, (Vec<MonoType>, MonoType)>) {
        self.concrete_func_sigs = sigs;
    }

    /// Return the concrete `(params, ret)` for `func_id` if it has a fully
    /// concrete signature that qualifies for typed closure emission, or `None`
    /// if the universal anyref path should be used.
    pub fn concrete_func_sig(&self, func_id: FuncId) -> Option<&(Vec<MonoType>, MonoType)> {
        self.concrete_func_sigs.get(&func_id)
    }

    pub fn setup_locals(&mut self, func: &AnfFunctionDef) -> Vec<ValType> {
        self.setup_locals_with_extra(func, &[])
    }

    pub fn setup_locals_with_extra(
        &mut self,
        func: &AnfFunctionDef,
        extra_params: &[(LocalId, ValType)],
    ) -> Vec<ValType> {
        self.local_map.clear();
        self.local_mono.clear();
        self.closure_locals.clear();
        self.label_stack.clear();
        self.loop_result_stack.clear();
        self.next_label_id = 0;
        let mut next_idx = 0_u32;

        for (i, local_id) in func.params.iter().enumerate() {
            let mono_ty = func.param_tys.get(i).cloned().unwrap_or(MonoType::Void);
            // Track function-typed params so call sites can use typed call_ref.
            if matches!(&mono_ty, MonoType::Function { .. }) {
                self.local_mono.insert(*local_id, mono_ty.clone());
            }
            let ty = mono_to_valtype_for_param(&mono_ty, self.type_env, &self.concrete_func_sigs);
            self.local_map.insert(*local_id, (next_idx, ty));
            next_idx += 1;
        }
        for (local_id, ty) in extra_params {
            self.local_map.insert(*local_id, (next_idx, ty.clone()));
            next_idx += 1;
        }

        let mut wasm_locals = Vec::new();
        self.assign_expr_locals(&func.body, &mut next_idx, &mut wasm_locals);
        wasm_locals
    }

    pub fn fresh_loop_labels(&mut self) -> (Label, Label) {
        let id = self.next_label_id;
        self.next_label_id += 1;
        (format!("break_{id}"), format!("cont_{id}"))
    }

    pub fn add_runtime_import(&mut self, prelude_entry: &PreludeEntry) {
        let (Some(module), Some(name), Some(sym)) = (
            prelude_entry.runtime_module,
            prelude_entry.runtime_name,
            prelude_entry.runtime_sym.as_ref(),
        ) else {
            return;
        };

        self.imports
            .entry(sym.clone())
            .or_insert_with(|| ImportDef {
                module: module.to_string(),
                name: name.to_string(),
                as_sym: sym.clone(),
                params: prelude_entry.runtime_params.clone(),
                results: prelude_entry.runtime_results.clone(),
            });
    }

    pub fn add_import(&mut self, import: ImportDef) {
        self.imports.insert(import.as_sym.clone(), import);
    }

    pub fn imports(&self) -> Vec<ImportDef> {
        self.imports.values().cloned().collect()
    }

    pub fn local(&self, local_id: LocalId) -> Option<&(u32, ValType)> {
        self.local_map.get(&local_id)
    }

    pub fn set_module_globals(&mut self, module_globals: HashMap<LocalId, String>) {
        self.module_globals = module_globals;
    }

    pub fn module_global_sym(&self, local_id: LocalId) -> Option<&String> {
        self.module_globals.get(&local_id)
    }

    pub fn user_func_sig(&self, func_id: FuncId) -> Option<&FuncSigInfo> {
        self.user_funcs.get(&func_id)
    }

    fn assign_expr_locals(
        &mut self,
        expr: &AnfExpr,
        next_idx: &mut u32,
        wasm_locals: &mut Vec<ValType>,
    ) {
        match expr {
            AnfExpr::Let { local, op, body } => {
                // Assign nested locals in branch/match bodies first so `infer_op_valtype`
                // can see their types when inferring the current let-binding type.
                self.assign_op_locals(op, next_idx, wasm_locals);

                if !self.local_map.contains_key(local) {
                    let local_ty = self.infer_op_valtype(op).unwrap_or(ValType::Anyref);
                    if let AnfOp::AMakeClosure { func_id, free_vars } = op.as_ref() {
                        self.closure_locals
                            .insert(*local, (*func_id, free_vars.clone()));
                        if let Some((params, ret)) = self.concrete_func_sigs.get(func_id) {
                            self.local_mono.insert(
                                *local,
                                MonoType::Function {
                                    params: params.clone(),
                                    ret: Box::new(ret.clone()),
                                },
                            );
                        }
                    }
                    // Propagate local_mono through AInit when the source is a
                    // local with a known Function type (e.g. `f2 := f1`).
                    if let AnfOp::AInit {
                        value: Atom::ALocal(src),
                    } = op.as_ref()
                    {
                        if let Some(mono) = self.local_mono.get(src).cloned() {
                            self.local_mono.insert(*local, mono);
                        }
                    }
                    self.local_map.insert(*local, (*next_idx, local_ty.clone()));
                    wasm_locals.push(local_ty);
                    *next_idx += 1;
                }

                self.assign_expr_locals(body, next_idx, wasm_locals);
            }
            AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) | AnfExpr::Atom(atom) => {
                self.infer_atom_valtype(atom);
            }
            AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
        }
    }

    fn assign_op_locals(&mut self, op: &AnfOp, next_idx: &mut u32, wasm_locals: &mut Vec<ValType>) {
        match op {
            AnfOp::AIf {
                then_branch,
                else_branch,
                ..
            } => {
                self.assign_expr_locals(then_branch, next_idx, wasm_locals);
                self.assign_expr_locals(else_branch, next_idx, wasm_locals);
            }
            AnfOp::AMatch { scrutinee, arms } => {
                // Pre-compute pattern binding types across all arms before visiting
                // arm bodies so local type inference can use concrete binding types.
                let scrutinee_ty = self.infer_atom_valtype(scrutinee);
                let mut pat_types: HashMap<LocalId, ValType> = HashMap::new();
                for AnfMatchArm { pattern, .. } in arms {
                    let mut typed = Vec::new();
                    collect_pattern_locals_typed(
                        pattern,
                        scrutinee_ty.as_ref(),
                        self.type_env,
                        &mut typed,
                    );
                    for (local_id, inferred_ty) in typed {
                        pat_types
                            .entry(local_id)
                            .and_modify(|existing| {
                                if *existing != inferred_ty {
                                    *existing = ValType::Anyref;
                                }
                            })
                            .or_insert(inferred_ty);
                    }
                }
                let mut pat_locals = pat_types.into_iter().collect::<Vec<_>>();
                pat_locals.sort_by_key(|(local_id, _)| local_id.0);
                for (local_id, local_ty) in pat_locals {
                    if !self.local_map.contains_key(&local_id) {
                        self.local_map
                            .insert(local_id, (*next_idx, local_ty.clone()));
                        wasm_locals.push(local_ty);
                        *next_idx += 1;
                    }
                }
                for AnfMatchArm { body, .. } in arms {
                    self.assign_expr_locals(body, next_idx, wasm_locals);
                }
            }
            AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
                self.assign_expr_locals(body, next_idx, wasm_locals);
            }
            _ => {}
        }
    }

    fn infer_op_valtype(&self, op: &AnfOp) -> Option<ValType> {
        match op {
            AnfOp::ACall { callee, .. } => self.infer_call_result_valtype(callee),
            AnfOp::AIf {
                then_branch,
                else_branch,
                ..
            } => {
                let then_ty = self.infer_expr_valtype(then_branch);
                let else_ty = self.infer_expr_valtype(else_branch);
                match (then_ty, else_ty) {
                    (Some(a), Some(b)) if a == b => Some(a),
                    (Some(a), _) if expr_always_diverges(else_branch) => Some(a),
                    (_, Some(b)) if expr_always_diverges(then_branch) => Some(b),
                    _ => None,
                }
            }
            AnfOp::AMatch { arms, .. } => {
                let mut value_ty: Option<ValType> = None;
                for arm in arms {
                    if expr_always_diverges(&arm.body) {
                        continue;
                    }
                    let arm_ty = self.infer_expr_valtype(&arm.body)?;
                    match &value_ty {
                        None => value_ty = Some(arm_ty),
                        Some(expected) if *expected == arm_ty => {}
                        Some(_) => return None,
                    }
                }
                if value_ty.is_some() {
                    return value_ty;
                }
                if !arms.is_empty() && arms.iter().all(|arm| expr_always_diverges(&arm.body)) {
                    // Unreachable expression (all arms diverge): use void-like i32
                    // rather than falling back to anyref.
                    return Some(ValType::I32);
                }
                None
            }
            AnfOp::ABinOp { op, operand_ty, .. } => Some(binop_result_ty(*op, *operand_ty)),
            AnfOp::AUnOp { op, operand_ty, .. } => Some(unop_result_ty(*op, *operand_ty)),
            AnfOp::AMakeClosure { func_id, .. } => {
                if let Some((params, ret)) = self.concrete_func_sigs.get(func_id) {
                    let sym = typed_closure_struct_sym(params, ret);
                    Some(ref_named(true, &sym))
                } else {
                    Some(ref_named(true, T_CLOSURE))
                }
            }
            AnfOp::ARecord { type_id, .. } | AnfOp::ARecordUpdate { type_id, .. } => {
                Some(ref_named(true, &user_record_type_sym(*type_id)))
            }
            AnfOp::AVariant { .. } => Some(ref_named(true, T_VARIANT)),
            AnfOp::AArrayLit(_) => Some(ref_named(true, T_ARRAY)),
            AnfOp::AInit { value } => self.infer_atom_valtype(value),
            AnfOp::AAssign { .. } | AnfOp::ADefer(_) => Some(ValType::I32),
            AnfOp::ALoop { body } => self.infer_loop_result_valtype(body),
            AnfOp::ARecordGet { field, type_id, .. } => {
                self.infer_record_field_valtype(*type_id, *field)
            }
            AnfOp::AIndex { result_ty, .. } => Some(mono_to_valtype(result_ty, self.type_env)),
        }
    }

    fn infer_atom_valtype(&self, atom: &Atom) -> Option<ValType> {
        match atom {
            Atom::ALocal(local_id) => self.local_map.get(local_id).map(|(_, ty)| ty.clone()),
            Atom::AGlobalFunc(_) => Some(ref_named(true, T_CLOSURE)),
            Atom::ALitInt(_) => Some(ValType::I64),
            Atom::ALitFloat(_) => Some(ValType::F64),
            Atom::ALitBool(_) => Some(ValType::I32),
            Atom::ALitStr(_) => Some(ref_named(true, T_STRING)),
            Atom::ALitVoid => Some(ValType::I32),
        }
    }

    fn infer_call_result_valtype(&self, callee: &Atom) -> Option<ValType> {
        match callee {
            Atom::AGlobalFunc(func_id) => {
                if let Some(entry) = self.prelude.get(func_id) {
                    return if entry.is_runtime_call() {
                        runtime_result_valtype(*func_id, entry)
                    } else {
                        intrinsic_result_valtype(*func_id)
                    };
                }
                self.user_funcs
                    .get(func_id)
                    .and_then(|sig| sig.result.clone())
            }
            Atom::ALocal(local_id) => {
                if let Some(MonoType::Function { ret, .. }) = self.local_mono.get(local_id) {
                    if is_concrete_mono_type(ret) {
                        return Some(mono_to_valtype(ret, self.type_env));
                    }
                }
                Some(ValType::Anyref)
            }
            _ => None,
        }
    }

    fn infer_expr_valtype(&self, expr: &AnfExpr) -> Option<ValType> {
        match expr {
            AnfExpr::Let { body, .. } => self.infer_expr_valtype(body),
            AnfExpr::Atom(atom) => self.infer_atom_valtype(atom),
            AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
                self.infer_atom_valtype(atom)
            }
            AnfExpr::Return(None) | AnfExpr::Break(None) => Some(ValType::I32),
            AnfExpr::Continue => None,
        }
    }

    fn infer_loop_result_valtype(&self, body: &AnfExpr) -> Option<ValType> {
        let mut breaks = Vec::new();
        collect_break_types(body, self, 0, &mut breaks);
        let first = breaks.first()?.clone();
        if breaks.iter().all(|ty| *ty == first) {
            Some(first)
        } else {
            None
        }
    }

    fn infer_record_field_valtype(
        &self,
        type_id: TypeId,
        field: crate::ir::FieldId,
    ) -> Option<ValType> {
        let field_ty = record_field_mono(self.type_env, type_id, field.0)?;
        Some(mono_to_valtype(field_ty, self.type_env))
    }
}

fn record_field_mono<'a>(
    type_env: &'a TypeEnv,
    type_id: TypeId,
    field_idx: usize,
) -> Option<&'a MonoType> {
    match type_env.get_def(type_id)? {
        TypeDef::Record { fields, .. } => fields.get(field_idx).map(|f| &f.ty),
        TypeDef::Alias { target, .. } => match target {
            MonoType::Named { type_id, .. } => record_field_mono(type_env, *type_id, field_idx),
            _ => None,
        },
        TypeDef::Sum { .. } => None,
    }
}

fn expr_always_diverges(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => true,
        AnfExpr::Atom(_) => false,
        AnfExpr::Let { op, body, .. } => op_always_diverges(op) || expr_always_diverges(body),
    }
}

fn op_always_diverges(op: &AnfOp) -> bool {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_always_diverges(then_branch) && expr_always_diverges(else_branch),
        AnfOp::AMatch { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|arm| expr_always_diverges(&arm.body))
        }
        _ => false,
    }
}

fn collect_break_types(expr: &AnfExpr, ctx: &EmitCtx<'_>, depth: usize, out: &mut Vec<ValType>) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_break_types_op(op, ctx, depth, out);
            collect_break_types(body, ctx, depth, out);
        }
        AnfExpr::Break(Some(atom)) if depth == 0 => {
            if let Some(ty) = ctx.infer_atom_valtype(atom) {
                out.push(ty);
            }
        }
        AnfExpr::Break(None) if depth == 0 => out.push(ValType::I32),
        AnfExpr::Return(_) | AnfExpr::Continue | AnfExpr::Atom(_) | AnfExpr::Break(_) => {}
    }
}

fn collect_break_types_op(op: &AnfOp, ctx: &EmitCtx<'_>, depth: usize, out: &mut Vec<ValType>) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_break_types(then_branch, ctx, depth, out);
            collect_break_types(else_branch, ctx, depth, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_break_types(&arm.body, ctx, depth, out);
            }
        }
        AnfOp::ALoop { body } => collect_break_types(body, ctx, depth + 1, out),
        AnfOp::ADefer(body) => collect_break_types(body, ctx, depth, out),
        _ => {}
    }
}

fn intrinsic_result_valtype(func_id: FuncId) -> Option<ValType> {
    use crate::ir::lower::prelude as ids;
    use crate::runtime::types::{T_ARRAY, T_STRING};

    let named_ref = |sym: &str| ValType::Ref {
        nullable: true,
        heap: HeapType::Named(sym.to_string()),
    };

    match func_id {
        id if id == ids::STRING_TO_STRING => Some(named_ref(T_STRING)),
        id if id == ids::VECTOR_PUSH => Some(named_ref(T_ARRAY)),
        id if id == ids::VECTOR_SET_IN_PLACE => Some(named_ref(T_ARRAY)),
        id if id == ids::VECTOR_BUILDER_FREEZE => Some(named_ref(T_ARRAY)),
        id if id == ids::RANGE_FROM
            || id == ids::RANGE
            || id == ids::RANGE_STEP
            || id == ids::CELL_NEW
            || id == ids::CELL_GET
            || id == ids::CELL_SET
            || id == ids::CELL_UPDATE
            || id == ids::DICT_GET_UNSAFE
            || id == ids::ITERATOR_NEXT
            || id == ids::ITERATOR_UNFOLD
            || id == ids::VECTOR_BUILDER_NEW
            || id == ids::VECTOR_BUILDER_PUSH
            || id == ids::VECTOR_GET
            || id == ids::VECTOR_SET
            || id == ids::VECTOR_MAKE
            || id == ids::FROM_CHAR_CODE
            || id == ids::INT_FROM_STRING
            || id == ids::FLOAT_FROM_STRING =>
        {
            Some(ValType::Anyref)
        }
        id if id == ids::CHAR_CODE_AT => Some(ValType::I64),
        _ => None,
    }
}

fn runtime_result_valtype(func_id: FuncId, entry: &PreludeEntry) -> Option<ValType> {
    use crate::ir::lower::prelude as ids;

    match func_id {
        // Twinkle `Int` is i64 even though runtime length primitives return i32.
        id if id == ids::VECTOR_LEN || id == ids::STRING_LEN || id == ids::DICT_LEN => {
            Some(ValType::I64)
        }
        _ => match entry.runtime_results.as_slice() {
            [] => Some(ValType::I32),
            [single] => Some(single.clone()),
            _ => None,
        },
    }
}

pub fn mono_to_valtype(ty: &MonoType, type_env: &TypeEnv) -> ValType {
    match ty {
        MonoType::Int => ValType::I64,
        MonoType::Float => ValType::F64,
        MonoType::Bool => ValType::I32,
        MonoType::String => ref_named(true, T_STRING),
        MonoType::Void | MonoType::Never => ValType::I32,
        MonoType::Vector(_) => ref_named(true, T_ARRAY),
        MonoType::Dict(_, _) => ref_named(true, T_DICT),
        MonoType::Function { .. } => ref_named(true, T_CLOSURE),
        MonoType::Var(_) | MonoType::MetaVar(_) => ValType::Anyref,
        MonoType::Named { type_id, .. } => mono_named_to_valtype(*type_id, type_env),
    }
}

fn mono_named_to_valtype(type_id: TypeId, type_env: &TypeEnv) -> ValType {
    if type_id == CELL_TYPE_ID || type_id == ITERATOR_TYPE_ID {
        return ref_named(true, T_ARRAY);
    }
    match type_env.get_def(type_id) {
        Some(TypeDef::Sum { .. }) => ref_named(true, T_VARIANT),
        Some(TypeDef::Record { .. }) => ref_named(true, &user_record_type_sym(type_id)),
        Some(TypeDef::Alias { target, .. }) => mono_to_valtype(target, type_env),
        None => ValType::Anyref,
    }
}

fn binop_result_ty(op: BinOp, operand_ty: OpKind) -> ValType {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => match operand_ty {
            OpKind::Int => ValType::I64,
            OpKind::Float => ValType::F64,
            OpKind::Bool => ValType::I32,
            OpKind::String => ref_named(true, T_STRING),
        },
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ValType::I32,
        BinOp::And | BinOp::Or => ValType::I32,
        BinOp::Assign => ValType::I32,
    }
}

fn unop_result_ty(op: UnOp, operand_ty: OpKind) -> ValType {
    match op {
        UnOp::Neg => match operand_ty {
            OpKind::Int => ValType::I64,
            OpKind::Float => ValType::F64,
            OpKind::Bool => ValType::I32,
            OpKind::String => ref_named(true, T_STRING),
        },
        UnOp::Not => ValType::I32,
    }
}

fn ref_named(nullable: bool, type_sym: &str) -> ValType {
    ValType::Ref {
        nullable,
        heap: HeapType::Named(type_sym.to_string()),
    }
}

pub fn user_record_type_sym(type_id: TypeId) -> String {
    format!("UserRecord_{}", type_id.0)
}

/// Returns true if `ty` has no generic type variables — i.e., it is
/// a fully-instantiated concrete type that can be used in typed closure
/// specialization.
pub fn is_concrete_mono_type(ty: &MonoType) -> bool {
    match ty {
        MonoType::Int
        | MonoType::Float
        | MonoType::Bool
        | MonoType::String
        | MonoType::Void
        | MonoType::Never => true,
        MonoType::Vector(inner) => is_concrete_mono_type(inner),
        MonoType::Dict(k, v) => is_concrete_mono_type(k) && is_concrete_mono_type(v),
        MonoType::Function { params, ret } => {
            params.iter().all(is_concrete_mono_type) && is_concrete_mono_type(ret)
        }
        MonoType::Named { args, .. } => args.iter().all(is_concrete_mono_type),
        MonoType::Var(_) | MonoType::MetaVar(_) => false,
    }
}

/// Map a `MonoType` to a short tag string for use in mangled type symbols.
/// e.g. `Int` → `"i64"`, `String` → `"str"`, `Vector<Int>` → `"arr"`.
pub fn mono_to_type_tag(ty: &MonoType) -> String {
    match ty {
        MonoType::Int => "i64".to_string(),
        MonoType::Float => "f64".to_string(),
        MonoType::Bool => "i32".to_string(),
        MonoType::String => "str".to_string(),
        MonoType::Void | MonoType::Never => "void".to_string(),
        MonoType::Vector(_) => "arr".to_string(),
        MonoType::Dict(_, _) => "dict".to_string(),
        MonoType::Function { .. } => "cls".to_string(),
        MonoType::Named { .. } => "ref".to_string(),
        MonoType::Var(_) | MonoType::MetaVar(_) => "any".to_string(),
    }
}

/// Symbol for a typed closure func type with the given signature.
/// e.g. `[Int, Int] -> Int` → `"closurefunc_i64_i64_i64"`.
/// Zero-param functions use the prefix `"closurefunc_nil__<ret>"`.
pub fn typed_closurefunc_sym(params: &[MonoType], ret: &MonoType) -> String {
    if params.is_empty() {
        format!("closurefunc_nil__{}", mono_to_type_tag(ret))
    } else {
        let param_tags = params
            .iter()
            .map(mono_to_type_tag)
            .collect::<Vec<_>>()
            .join("_");
        format!("closurefunc_{}_{}", param_tags, mono_to_type_tag(ret))
    }
}

/// Symbol for a typed closure struct with the given signature.
/// e.g. `[Int, Int] -> Int` → `"closure_i64_i64_i64"`.
pub fn typed_closure_struct_sym(params: &[MonoType], ret: &MonoType) -> String {
    if params.is_empty() {
        format!("closure_nil__{}", mono_to_type_tag(ret))
    } else {
        let param_tags = params
            .iter()
            .map(mono_to_type_tag)
            .collect::<Vec<_>>()
            .join("_");
        format!("closure_{}_{}", param_tags, mono_to_type_tag(ret))
    }
}

/// Like [`mono_to_valtype`] but maps a concrete `MonoType::Function` to the
/// typed closure struct ValType instead of the universal `$Closure`.
///
/// Falls back to [`mono_to_valtype`] when `concrete_func_sigs` is empty
/// (universal / non-typed-closure path) or when the function type contains
/// generic variables.
pub fn mono_to_valtype_for_param(
    mono_ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> ValType {
    if !concrete_func_sigs.is_empty() {
        if let MonoType::Function { params, ret } = mono_ty {
            if is_concrete_mono_type(mono_ty) {
                let sym = typed_closure_struct_sym(params, ret);
                return ref_named(true, &sym);
            }
        }
    }
    mono_to_valtype(mono_ty, type_env)
}

fn collect_pattern_locals_typed(
    pattern: &CorePattern,
    expected: Option<&ValType>,
    type_env: &TypeEnv,
    out: &mut Vec<(LocalId, ValType)>,
) {
    match pattern {
        CorePattern::Var(local_id) => {
            let ty = expected.cloned().unwrap_or(ValType::Anyref);
            out.push((*local_id, ty));
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
            let field_tys = sum_variant_field_valtypes(type_env, *type_id, variant.0);
            for (idx, field_pat) in fields.iter().enumerate() {
                let field_expected = field_tys.get(idx);
                collect_pattern_locals_typed(field_pat, field_expected, type_env, out);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

fn sum_variant_field_valtypes(
    type_env: &TypeEnv,
    type_id: TypeId,
    variant_idx: usize,
) -> Vec<ValType> {
    let (fields, source_type_id, has_type_params): (Vec<MonoType>, TypeId, bool) =
        match type_env.get_def(type_id) {
            Some(TypeDef::Sum {
                variants,
                type_params,
                ..
            }) => (
                variants
                    .get(variant_idx)
                    .map(|v| v.fields.clone())
                    .unwrap_or_default(),
                type_id,
                !type_params.is_empty(),
            ),
            Some(TypeDef::Alias { target, .. }) => match target {
                MonoType::Named { type_id, .. } => match type_env.get_def(*type_id) {
                    Some(TypeDef::Sum {
                        variants,
                        type_params,
                        ..
                    }) => (
                        variants
                            .get(variant_idx)
                            .map(|v| v.fields.clone())
                            .unwrap_or_default(),
                        *type_id,
                        !type_params.is_empty(),
                    ),
                    _ => (Vec::new(), *type_id, false),
                },
                _ => (Vec::new(), type_id, false),
            },
            _ => (Vec::new(), type_id, false),
        };
    let builtin_placeholder_sum = source_type_id == OPTION_TYPE_ID
        || source_type_id == RESULT_TYPE_ID
        || source_type_id == UNFOLD_STEP_TYPE_ID;
    fields
        .iter()
        .map(|mono| {
            // Generic sum placeholders (e.g. built-in Option/Result definitions) store
            // `Void` in the field list; concrete call-site instantiations are erased to
            // `anyref` at codegen time.
            if (has_type_params || builtin_placeholder_sum) && matches!(mono, MonoType::Void) {
                ValType::Anyref
            } else {
                mono_to_valtype(mono, type_env)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::prelude::build_prelude_map;
    use crate::ir::lower::prelude as prelude_ids;
    use crate::ir::{FieldId, VariantId};
    use crate::types::ty::{RESULT_TYPE_ID, Variant};

    #[test]
    fn local_type_if_with_continue_branch_prefers_value_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(1),
            name: "if_continue".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AIf {
                    cond: Atom::ALitBool(true),
                    then_branch: Box::new(AnfExpr::Atom(Atom::ALitInt(7))),
                    else_branch: Box::new(AnfExpr::Continue),
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_loop_with_break_value_prefers_break_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(2),
            name: "loop_break_value".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ALoop {
                    body: Box::new(AnfExpr::Break(Some(Atom::ALitInt(9)))),
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_array_len_call_uses_i64_int_semantics() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(101),
            name: "array_len_type".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AArrayLit(vec![Atom::ALitInt(1)])),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(2),
                    op: Box::new(AnfOp::ACall {
                        callee: Atom::AGlobalFunc(prelude_ids::VECTOR_LEN),
                        args: vec![Atom::ALocal(LocalId(1))],
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
                }),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(2)).expect("missing local L2");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_record_get_prefers_field_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(3),
            name: "record_get".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::named(crate::types::ty::RANGE_TYPE_ID)],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ARecordGet {
                    target: Atom::ALocal(LocalId(0)),
                    field: FieldId(0),
                    type_id: crate::types::ty::RANGE_TYPE_ID,
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_index_prefers_element_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(4),
            name: "index_get".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::Vector(Box::new(MonoType::Int))],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AIndex {
                    base: Atom::ALocal(LocalId(0)),
                    index: Atom::ALitInt(0),
                    base_ty: crate::ir::anf::IndexKind::Array,
                    result_ty: MonoType::Int,
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_match_variant_binding_prefers_variant_field_type() {
        let mut type_env = TypeEnv::new();
        let sum_ty = type_env.add_type(TypeDef::Sum {
            name: "IntBox".to_string(),
            type_params: vec![],
            variants: vec![
                Variant {
                    name: "None".to_string(),
                    fields: vec![],
                },
                Variant {
                    name: "Some".to_string(),
                    fields: vec![MonoType::Int],
                },
            ],
        });
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(5),
            name: "match_bind".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::named(sum_ty)],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AMatch {
                    scrutinee: Atom::ALocal(LocalId(0)),
                    arms: vec![
                        AnfMatchArm {
                            pattern: CorePattern::Variant {
                                type_id: sum_ty,
                                variant: VariantId(1),
                                fields: vec![CorePattern::Var(LocalId(2))],
                            },
                            body: AnfExpr::Atom(Atom::ALocal(LocalId(2))),
                        },
                        AnfMatchArm {
                            pattern: CorePattern::Wildcard,
                            body: AnfExpr::Atom(Atom::ALitInt(0)),
                        },
                    ],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx
            .local(LocalId(2))
            .expect("missing pattern-bound local L2");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_match_var_binding_prefers_scrutinee_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(6),
            name: "match_var_bind".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::Int],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AMatch {
                    scrutinee: Atom::ALocal(LocalId(0)),
                    arms: vec![AnfMatchArm {
                        pattern: CorePattern::Var(LocalId(2)),
                        body: AnfExpr::Atom(Atom::ALocal(LocalId(2))),
                    }],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx
            .local(LocalId(2))
            .expect("missing pattern-bound local L2");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_match_with_diverging_arm_prefers_non_diverging_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(7),
            name: "match_diverge".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AMatch {
                    scrutinee: Atom::ALitBool(true),
                    arms: vec![
                        AnfMatchArm {
                            pattern: CorePattern::LitBool(true),
                            body: AnfExpr::Return(None),
                        },
                        AnfMatchArm {
                            pattern: CorePattern::Wildcard,
                            body: AnfExpr::Atom(Atom::ALitInt(1)),
                        },
                    ],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: MonoType::Int,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn local_type_match_result_payload_prefers_anyref_placeholder() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let result_string_string = MonoType::Named {
            type_id: RESULT_TYPE_ID,
            args: vec![MonoType::String, MonoType::String],
        };
        let func = AnfFunctionDef {
            func_id: FuncId(8),
            name: "match_result_bind".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![result_string_string.clone()],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AMatch {
                    scrutinee: Atom::ALocal(LocalId(0)),
                    arms: vec![
                        AnfMatchArm {
                            pattern: CorePattern::Variant {
                                type_id: RESULT_TYPE_ID,
                                variant: VariantId(0),
                                fields: vec![CorePattern::Var(LocalId(2))],
                            },
                            body: AnfExpr::Atom(Atom::ALocal(LocalId(2))),
                        },
                        AnfMatchArm {
                            pattern: CorePattern::Variant {
                                type_id: RESULT_TYPE_ID,
                                variant: VariantId(1),
                                fields: vec![CorePattern::Var(LocalId(3))],
                            },
                            body: AnfExpr::Atom(Atom::ALocal(LocalId(3))),
                        },
                    ],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(1)))),
            },
            return_ty: result_string_string,
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty_ok) = ctx.local(LocalId(2)).expect("missing Ok payload local");
        let (_, ty_err) = ctx.local(LocalId(3)).expect("missing Err payload local");
        assert_eq!(*ty_ok, ValType::Anyref);
        assert_eq!(*ty_err, ValType::Anyref);
    }
}
