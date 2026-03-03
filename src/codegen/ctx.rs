use std::collections::{BTreeMap, HashMap};

use crate::codegen::prelude::{PreludeEntry, PreludeMap};
use crate::ir::FuncId;
use crate::ir::LocalId;
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom, OpKind};
use crate::ir::core::CorePattern;
use crate::runtime::types::{T_ARRAY, T_CLOSURE, T_DICT, T_STRING, T_VARIANT};
use crate::syntax::ast::{BinOp, UnOp};
use crate::types::env::TypeEnv;
use crate::types::ty::{MonoType, TypeDef, TypeId};
use crate::wasm::ir::{FuncSym, HeapType, ImportDef, Label, ValType};

#[derive(Debug, Clone)]
pub struct FuncSigInfo {
    pub params: Vec<ValType>,
    pub result: Option<ValType>,
}

pub struct EmitCtx<'a> {
    pub local_map: HashMap<LocalId, (u32, ValType)>,
    pub label_stack: Vec<(Label, Label)>,
    pub loop_result_stack: Vec<Option<ValType>>,
    next_label_id: u32,
    imports: BTreeMap<FuncSym, ImportDef>,
    pub type_env: &'a TypeEnv,
    pub prelude: &'a PreludeMap,
    user_funcs: &'a HashMap<FuncId, FuncSigInfo>,
}

impl<'a> EmitCtx<'a> {
    pub fn new(
        type_env: &'a TypeEnv,
        prelude: &'a PreludeMap,
        user_funcs: &'a HashMap<FuncId, FuncSigInfo>,
    ) -> Self {
        Self {
            local_map: HashMap::new(),
            label_stack: Vec::new(),
            loop_result_stack: Vec::new(),
            next_label_id: 0,
            imports: BTreeMap::new(),
            type_env,
            prelude,
            user_funcs,
        }
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
        self.label_stack.clear();
        self.loop_result_stack.clear();
        self.next_label_id = 0;
        let mut next_idx = 0_u32;

        for (i, local_id) in func.params.iter().enumerate() {
            let ty = func
                .param_tys
                .get(i)
                .map(|t| mono_to_valtype(t, self.type_env))
                .unwrap_or(ValType::Anyref);
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
                if !self.local_map.contains_key(local) {
                    let local_ty = self.infer_op_valtype(op).unwrap_or(ValType::Anyref);
                    self.local_map.insert(*local, (*next_idx, local_ty.clone()));
                    wasm_locals.push(local_ty);
                    *next_idx += 1;
                }

                self.assign_op_locals(op, next_idx, wasm_locals);
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
            AnfOp::AMatch { arms, .. } => {
                for AnfMatchArm { pattern, body } in arms {
                    let mut pat_locals = Vec::new();
                    collect_pattern_locals(pattern, &mut pat_locals);
                    for local_id in pat_locals {
                        if !self.local_map.contains_key(&local_id) {
                            self.local_map
                                .insert(local_id, (*next_idx, ValType::Anyref));
                            wasm_locals.push(ValType::Anyref);
                            *next_idx += 1;
                        }
                    }
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
                    _ => None,
                }
            }
            AnfOp::AMatch { arms, .. } => {
                let first_ty = arms
                    .first()
                    .and_then(|arm| self.infer_expr_valtype(&arm.body));
                if let Some(ref expected) = first_ty {
                    if arms
                        .iter()
                        .all(|arm| self.infer_expr_valtype(&arm.body).as_ref() == Some(expected))
                    {
                        return first_ty;
                    }
                }
                None
            }
            AnfOp::ABinOp { op, operand_ty, .. } => Some(binop_result_ty(*op, *operand_ty)),
            AnfOp::AUnOp { op, operand_ty, .. } => Some(unop_result_ty(*op, *operand_ty)),
            AnfOp::AMakeClosure { .. } => Some(ref_named(true, T_CLOSURE)),
            AnfOp::ARecord { type_id, .. } | AnfOp::ARecordUpdate { type_id, .. } => {
                Some(ref_named(true, &user_record_type_sym(*type_id)))
            }
            AnfOp::AVariant { .. } => Some(ref_named(true, T_VARIANT)),
            AnfOp::AArrayLit(_) => Some(ref_named(true, T_ARRAY)),
            AnfOp::AInit { value } => self.infer_atom_valtype(value),
            AnfOp::AAssign { .. } | AnfOp::ADefer(_) => Some(ValType::I32),
            AnfOp::ALoop { .. } | AnfOp::ARecordGet { .. } | AnfOp::AIndex { .. } => None,
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
                        match entry.runtime_results.as_slice() {
                            [] => Some(ValType::I32),
                            [single] => Some(single.clone()),
                            _ => None,
                        }
                    } else {
                        intrinsic_result_valtype(*func_id)
                    };
                }
                self.user_funcs
                    .get(func_id)
                    .and_then(|sig| sig.result.clone())
            }
            Atom::ALocal(_) => Some(ValType::Anyref),
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
        id if id == ids::ARRAY_APPEND => Some(named_ref(T_ARRAY)),
        id if id == ids::ARRAY_BUILDER_FREEZE => Some(named_ref(T_ARRAY)),
        id if id == ids::DEBUG_STDIN_READ_ALL => Some(named_ref(T_STRING)),
        id if id == ids::DEBUG_READ_FILE => Some(ValType::Anyref),
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
            || id == ids::ARRAY_BUILDER_NEW
            || id == ids::ARRAY_BUILDER_PUSH =>
        {
            Some(ValType::Anyref)
        }
        _ => None,
    }
}

pub fn mono_to_valtype(ty: &MonoType, type_env: &TypeEnv) -> ValType {
    match ty {
        MonoType::Int => ValType::I64,
        MonoType::Float => ValType::F64,
        MonoType::Bool => ValType::I32,
        MonoType::String => ref_named(true, T_STRING),
        MonoType::Void | MonoType::Never => ValType::I32,
        MonoType::Array(_) => ref_named(true, T_ARRAY),
        MonoType::Dict(_, _) => ref_named(true, T_DICT),
        MonoType::Function { .. } => ref_named(true, T_CLOSURE),
        MonoType::Var(_) | MonoType::MetaVar(_) => ValType::Anyref,
        MonoType::Named { type_id, .. } => mono_named_to_valtype(*type_id, type_env),
    }
}

fn mono_named_to_valtype(type_id: TypeId, type_env: &TypeEnv) -> ValType {
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

fn collect_pattern_locals(pattern: &CorePattern, out: &mut Vec<LocalId>) {
    match pattern {
        CorePattern::Var(local_id) => out.push(*local_id),
        CorePattern::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_locals(field, out);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}
