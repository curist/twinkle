use std::collections::{BTreeMap, HashMap, HashSet};

use crate::codegen::prelude::{PreludeEntry, PreludeMap};
use crate::ir::FuncId;
use crate::ir::LocalId;
use crate::ir::VariantId;
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom, OpKind};
use crate::ir::core::CorePattern;
use crate::runtime::types::{T_ARRAY, T_CLOSURE, T_DICT, T_ITER_STATE, T_STRING, T_VARIANT};
use crate::syntax::ast::{BinOp, UnOp};
use crate::types::env::TypeEnv;
use crate::types::ty::{
    CELL_TYPE_ID, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID,
    TypeDef, TypeId, UNFOLD_STEP_TYPE_ID,
};
use crate::wasm::ir::{FuncSym, HeapType, ImportDef, Label, ValType};

#[derive(Debug, Clone)]
pub struct FuncSigInfo {
    pub params: Vec<ValType>,
    pub result: Option<ValType>,
    pub result_mono: Option<MonoType>,
}

#[derive(Debug, Clone)]
pub struct UserFuncAbi {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
    pub semantic_result_mono: Option<MonoType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IteratorStateInfo {
    pub yield_ty: MonoType,
    pub seed_ty: MonoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueRepr {
    TypedClosure {
        params: Vec<MonoType>,
        ret: MonoType,
    },
    TypedCell {
        elem_ty: MonoType,
    },
}

#[derive(Debug, Clone, Default)]
pub struct LocalBackendInfo {
    pub repr: Option<ValueRepr>,
    pub iterator_state: Option<IteratorStateInfo>,
    pub iterator_next_state: Option<IteratorStateInfo>,
    pub iter_item_state: Option<IteratorStateInfo>,
    /// Tracks locals that hold a typed Option<T> or Result<T, E> value.
    pub typed_option: Option<MonoType>,
}

#[derive(Debug, Clone, Default)]
pub struct SpecializedTypeRegistry {
    pub iterator_helpers: BTreeMap<String, IteratorStateInfo>,
    pub typed_iterator_states: BTreeMap<String, IteratorStateInfo>,
    pub typed_iter_items: BTreeMap<String, IteratorStateInfo>,
    pub typed_iter_options: BTreeMap<String, IteratorStateInfo>,
    pub typed_unfold_steps: BTreeMap<String, (MonoType, MonoType)>,
    pub typed_closures: BTreeMap<String, (Vec<MonoType>, MonoType)>,
    pub typed_cells: BTreeMap<String, MonoType>,
    /// Typed Option<T> / Result<T,E> struct types keyed by sym.
    pub typed_general_options: BTreeMap<String, MonoType>,
    /// Pooled module-local string literals keyed by UTF-8 bytes.
    pub string_literals: BTreeMap<Vec<u8>, StringLiteralPoolEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteralPoolEntry {
    pub global_sym: String,
    pub getter_sym: String,
}

pub struct EmitCtx<'a> {
    pub local_map: HashMap<LocalId, (u32, ValType)>,
    /// Tracks concrete monomorphic types for locals when codegen can preserve a
    /// more specific Wasm representation than plain `Anyref`.
    pub local_mono: HashMap<LocalId, MonoType>,
    pub capture_mono_by_func: HashMap<FuncId, HashMap<LocalId, MonoType>>,
    /// Tracks local bindings created from `AMakeClosure` so direct user calls
    /// can materialize typed closures only at concrete higher-order boundaries.
    pub closure_locals: HashMap<LocalId, (FuncId, Vec<LocalId>)>,
    /// Unified backend flow metadata for iterator-related local specialization.
    pub local_backend: HashMap<LocalId, LocalBackendInfo>,
    assigned_locals: HashSet<LocalId>,
    rebound_locals: HashSet<LocalId>,
    in_init_func: bool,
    pub current_func_id: Option<FuncId>,
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
    /// User functions whose return value is known to be a concrete
    /// iterator-unfold state. This lets callers specialize `Iterator.next`
    /// even though the surface type is only `Iterator<T>`.
    pub user_func_iterator_states: HashMap<FuncId, IteratorStateInfo>,
    specialized_types: SpecializedTypeRegistry,
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
            capture_mono_by_func: HashMap::new(),
            closure_locals: HashMap::new(),
            local_backend: HashMap::new(),
            assigned_locals: HashSet::new(),
            rebound_locals: HashSet::new(),
            in_init_func: false,
            current_func_id: None,
            module_globals: HashMap::new(),
            label_stack: Vec::new(),
            loop_result_stack: Vec::new(),
            next_label_id: 0,
            imports: BTreeMap::new(),
            type_env,
            prelude,
            user_funcs,
            concrete_func_sigs: HashMap::new(),
            user_func_iterator_states: HashMap::new(),
            specialized_types: SpecializedTypeRegistry::default(),
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

    pub fn set_user_func_iterator_states(&mut self, states: HashMap<FuncId, IteratorStateInfo>) {
        self.user_func_iterator_states = states;
    }

    pub fn user_func_iterator_state(&self, func_id: FuncId) -> Option<&IteratorStateInfo> {
        self.user_func_iterator_states.get(&func_id)
    }

    pub fn request_iterator_helper(&mut self, sym: String, info: IteratorStateInfo) {
        self.specialized_types
            .iterator_helpers
            .entry(sym)
            .or_insert(info);
    }

    pub fn requested_iterator_helpers(&self) -> &BTreeMap<String, IteratorStateInfo> {
        &self.specialized_types.iterator_helpers
    }

    pub fn request_typed_iterator_state(&mut self, sym: String, info: IteratorStateInfo) {
        self.specialized_types
            .typed_iterator_states
            .entry(sym)
            .or_insert(info);
    }

    pub fn requested_typed_iterator_states(&self) -> &BTreeMap<String, IteratorStateInfo> {
        &self.specialized_types.typed_iterator_states
    }

    pub fn request_typed_iter_item(&mut self, sym: String, info: IteratorStateInfo) {
        self.specialized_types
            .typed_iter_items
            .entry(sym)
            .or_insert(info);
    }

    pub fn requested_typed_iter_items(&self) -> &BTreeMap<String, IteratorStateInfo> {
        &self.specialized_types.typed_iter_items
    }

    pub fn request_typed_iter_option(&mut self, sym: String, info: IteratorStateInfo) {
        self.specialized_types
            .typed_iter_options
            .entry(sym)
            .or_insert(info);
    }

    pub fn requested_typed_iter_options(&self) -> &BTreeMap<String, IteratorStateInfo> {
        &self.specialized_types.typed_iter_options
    }

    pub fn request_typed_unfold_step(
        &mut self,
        sym: String,
        yield_ty: MonoType,
        seed_ty: MonoType,
    ) {
        self.specialized_types
            .typed_unfold_steps
            .entry(sym)
            .or_insert((yield_ty, seed_ty));
    }

    pub fn requested_typed_unfold_steps(&self) -> &BTreeMap<String, (MonoType, MonoType)> {
        &self.specialized_types.typed_unfold_steps
    }

    pub fn request_typed_closure(&mut self, sym: String, params: Vec<MonoType>, ret: MonoType) {
        self.specialized_types
            .typed_closures
            .entry(sym)
            .or_insert((params, ret));
    }

    pub fn requested_typed_closures(&self) -> &BTreeMap<String, (Vec<MonoType>, MonoType)> {
        &self.specialized_types.typed_closures
    }

    pub fn request_typed_cell(&mut self, sym: String, elem_ty: MonoType) {
        self.specialized_types
            .typed_cells
            .entry(sym)
            .or_insert(elem_ty);
    }

    pub fn requested_typed_cells(&self) -> &BTreeMap<String, MonoType> {
        &self.specialized_types.typed_cells
    }

    pub fn request_typed_general_option(&mut self, sym: String, mono: MonoType) {
        self.specialized_types
            .typed_general_options
            .entry(sym)
            .or_insert(mono);
    }

    pub fn requested_typed_general_options(&self) -> &BTreeMap<String, MonoType> {
        &self.specialized_types.typed_general_options
    }

    pub fn request_string_literal(&mut self, literal: &str) -> String {
        let bytes = literal.as_bytes().to_vec();
        let entry = self
            .specialized_types
            .string_literals
            .entry(bytes.clone())
            .or_insert_with(|| {
                let suffix = string_literal_symbol_suffix(&bytes);
                StringLiteralPoolEntry {
                    global_sym: format!("__str_lit_global_{suffix}"),
                    getter_sym: format!("__str_lit_get_{suffix}"),
                }
            });
        entry.getter_sym.clone()
    }

    pub fn requested_string_literals(&self) -> &BTreeMap<Vec<u8>, StringLiteralPoolEntry> {
        &self.specialized_types.string_literals
    }

    pub fn local_typed_option(&self, local_id: LocalId) -> Option<&MonoType> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.typed_option.as_ref())
    }

    pub fn set_local_typed_option(&mut self, local_id: LocalId, mono: Option<MonoType>) {
        self.local_backend.entry(local_id).or_default().typed_option = mono;
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
        self.local_backend.clear();
        self.assigned_locals.clear();
        self.rebound_locals.clear();
        self.label_stack.clear();
        self.loop_result_stack.clear();
        self.next_label_id = 0;
        self.in_init_func = func.name == "__init__";
        self.current_func_id = Some(func.func_id);
        collect_assigned_locals_expr(&func.body, &mut self.assigned_locals);
        let mut local_bind_counts = HashMap::new();
        collect_local_binding_counts_expr(&func.body, &mut local_bind_counts);
        self.rebound_locals.extend(
            local_bind_counts
                .into_iter()
                .filter_map(|(local_id, count)| (count > 1).then_some(local_id)),
        );
        let mut next_idx = 0_u32;

        for (i, local_id) in func.params.iter().enumerate() {
            let mono_ty = func.param_tys.get(i).cloned().unwrap_or(MonoType::Void);
            let erased_assignment = (self.assigned_locals.contains(local_id)
                || self.rebound_locals.contains(local_id))
                && should_erase_assigned_local(&mono_ty);
            let erase_init_cell = self.in_init_func && is_cell_mono(&mono_ty);
            let local_repr = if erased_assignment || erase_init_cell {
                None
            } else {
                value_repr_from_mono(&mono_ty, &self.concrete_func_sigs)
            };
            if !erased_assignment {
                if !erase_init_cell {
                    self.local_mono.insert(*local_id, mono_ty.clone());
                }
            }
            self.set_local_value_repr(*local_id, local_repr);
            let ty = if erased_assignment || erase_init_cell {
                ValType::Anyref
            } else {
                mono_to_valtype_specialized(&mono_ty, self.type_env, &self.concrete_func_sigs)
            };
            self.local_map.insert(*local_id, (next_idx, ty));
            next_idx += 1;
        }
        for (local_id, ty) in extra_params {
            self.local_map.insert(*local_id, (next_idx, ty.clone()));
            next_idx += 1;
        }
        if let Some(capture_mono) = self.capture_mono_by_func.get(&func.func_id).cloned() {
            for (local_id, mono) in capture_mono {
                self.local_mono.insert(local_id, mono.clone());
                self.set_local_value_repr(
                    local_id,
                    value_repr_from_mono(&mono, &self.concrete_func_sigs),
                );
            }
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

    pub fn has_import(&self, as_sym: &str) -> bool {
        self.imports.contains_key(as_sym)
    }

    pub fn local(&self, local_id: LocalId) -> Option<&(u32, ValType)> {
        self.local_map.get(&local_id)
    }

    pub fn set_module_globals(&mut self, module_globals: HashMap<LocalId, String>) {
        self.module_globals = module_globals;
    }

    pub fn set_capture_mono_by_func(
        &mut self,
        capture_mono_by_func: HashMap<FuncId, HashMap<LocalId, MonoType>>,
    ) {
        self.capture_mono_by_func = capture_mono_by_func;
    }

    pub fn module_global_sym(&self, local_id: LocalId) -> Option<&String> {
        self.module_globals.get(&local_id)
    }

    pub fn local_iterator_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.iterator_state.clone())
    }

    pub fn local_value_repr(&self, local_id: LocalId) -> Option<ValueRepr> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.repr.clone())
    }

    pub fn local_typed_closure_sig(&self, local_id: LocalId) -> Option<(Vec<MonoType>, MonoType)> {
        match self.local_value_repr(local_id)? {
            ValueRepr::TypedClosure { params, ret } => Some((params, ret)),
            _ => None,
        }
    }

    pub fn local_typed_cell_elem(&self, local_id: LocalId) -> Option<MonoType> {
        match self.local_value_repr(local_id)? {
            ValueRepr::TypedCell { elem_ty } => Some(elem_ty),
            _ => None,
        }
    }

    pub fn local_iterator_next_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.iterator_next_state.clone())
    }

    pub fn local_iter_item_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.iter_item_state.clone())
    }

    pub(crate) fn set_local_iterator_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.iterator_state = info;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    pub(crate) fn set_local_iterator_next_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.iterator_next_state = info;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    pub(crate) fn set_local_iter_item_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.iter_item_state = info;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    pub(crate) fn set_local_value_repr(&mut self, local_id: LocalId, repr: Option<ValueRepr>) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.repr = repr;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_local_typed_closure_sig(
        &mut self,
        local_id: LocalId,
        sig: Option<(Vec<MonoType>, MonoType)>,
    ) {
        let repr = sig.map(|(params, ret)| ValueRepr::TypedClosure { params, ret });
        self.set_local_value_repr(local_id, repr);
    }

    #[cfg(test)]
    pub(crate) fn set_local_typed_cell_elem(&mut self, local_id: LocalId, elem: Option<MonoType>) {
        let repr = elem.map(|elem_ty| ValueRepr::TypedCell { elem_ty });
        self.set_local_value_repr(local_id, repr);
    }

    pub fn user_func_sig(&self, func_id: FuncId) -> Option<&FuncSigInfo> {
        self.user_funcs.get(&func_id)
    }

    pub fn user_func_abi(&self, func_id: FuncId) -> Option<UserFuncAbi> {
        let sig = self.user_funcs.get(&func_id)?;
        Some(UserFuncAbi {
            params: sig.params.clone(),
            results: sig.result.iter().cloned().collect(),
            semantic_result_mono: sig.result_mono.clone(),
        })
    }

    pub fn infer_op_mono_for_emit(&self, op: &AnfOp) -> Option<MonoType> {
        self.infer_op_mono(op)
    }

    pub fn push_flow_mono_binding(
        &mut self,
        local: LocalId,
        mono: Option<MonoType>,
        restores: &mut Vec<(LocalId, Option<MonoType>)>,
    ) {
        let prev = match mono {
            Some(mono) => self.local_mono.insert(local, mono),
            None => self.local_mono.remove(&local),
        };
        restores.push((local, prev));
    }

    pub fn restore_flow_mono_binding(&mut self, local: LocalId, prev: Option<MonoType>) {
        if let Some(prev) = prev {
            self.local_mono.insert(local, prev);
        } else {
            self.local_mono.remove(&local);
        }
    }

    pub fn push_flow_value_repr_binding(
        &mut self,
        local: LocalId,
        repr: Option<ValueRepr>,
        restores: &mut Vec<(LocalId, Option<ValueRepr>)>,
    ) {
        let prev = self.local_value_repr(local);
        self.set_local_value_repr(local, repr);
        restores.push((local, prev));
    }

    pub fn restore_flow_value_repr_binding(&mut self, local: LocalId, prev: Option<ValueRepr>) {
        self.set_local_value_repr(local, prev);
    }

    pub fn push_flow_iterator_binding(
        &mut self,
        local: LocalId,
        info: Option<IteratorStateInfo>,
        restores: &mut Vec<(LocalId, Option<IteratorStateInfo>)>,
    ) {
        let prev = self.local_iterator_state(local);
        self.set_local_iterator_state(local, info);
        restores.push((local, prev));
    }

    pub fn restore_flow_iterator_binding(
        &mut self,
        local: LocalId,
        prev: Option<IteratorStateInfo>,
    ) {
        self.set_local_iterator_state(local, prev);
    }

    fn push_flow_iterator_next_binding(
        &mut self,
        local: LocalId,
        info: Option<IteratorStateInfo>,
        restores: &mut Vec<(LocalId, Option<IteratorStateInfo>)>,
    ) {
        let prev = self.local_iterator_next_state(local);
        self.set_local_iterator_next_state(local, info);
        restores.push((local, prev));
    }

    fn restore_flow_iterator_next_binding(
        &mut self,
        local: LocalId,
        prev: Option<IteratorStateInfo>,
    ) {
        self.set_local_iterator_next_state(local, prev);
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
                    let inferred_mono = if self.module_global_sym(*local).is_some() {
                        None
                    } else {
                        self.infer_op_mono(op)
                    };
                    let iterator_state = iterator_state_from_setup_op(op, self);
                    let iterator_next_state = iterator_next_result_state_from_op(op, self);
                    let erase_assignment = (self.assigned_locals.contains(local)
                        || self.rebound_locals.contains(local))
                        && inferred_mono
                            .as_ref()
                            .is_some_and(should_erase_assigned_local);
                    let erase_init_cell =
                        self.in_init_func && inferred_mono.as_ref().is_some_and(is_cell_mono);
                    let local_ty = if erase_assignment || erase_init_cell {
                        ValType::Anyref
                    } else if let Some(info) = iterator_state.as_ref() {
                        ref_named(true, &typed_iterator_state_sym(info))
                    } else if let Some(info) = iterator_next_state.as_ref() {
                        ref_named(true, &typed_iter_option_sym(info))
                    } else if inferred_mono
                        .as_ref()
                        .is_some_and(is_typed_general_option_candidate)
                    {
                        // Keep specialized general Option<T> locals at anyref to avoid
                        // forcing an invalid cast into universal Variant layout.
                        ValType::Anyref
                    } else {
                        self.infer_op_valtype(op)
                            .or_else(|| {
                                inferred_mono.as_ref().map(|mono| {
                                    mono_to_valtype_specialized(
                                        mono,
                                        self.type_env,
                                        &self.concrete_func_sigs,
                                    )
                                })
                            })
                            .unwrap_or(ValType::Anyref)
                    };
                    let preserved_mono = inferred_mono
                        .as_ref()
                        .filter(|_| !(erase_assignment || erase_init_cell))
                        .cloned();
                    if let Some(mono) = preserved_mono.clone() {
                        self.local_mono.insert(*local, mono);
                    }
                    self.set_local_value_repr(
                        *local,
                        preserved_mono
                            .as_ref()
                            .and_then(|mono| value_repr_from_mono(mono, &self.concrete_func_sigs)),
                    );
                    if let Some(info) = iterator_state {
                        self.set_local_iterator_state(*local, Some(info));
                    }
                    if let Some(info) = iterator_next_state {
                        self.set_local_iterator_next_state(*local, Some(info));
                    }
                    if let AnfOp::AMakeClosure { func_id, free_vars } = op.as_ref() {
                        self.closure_locals
                            .insert(*local, (*func_id, free_vars.clone()));
                    }
                    self.local_map.insert(*local, (*next_idx, local_ty.clone()));
                    wasm_locals.push(local_ty);
                    *next_idx += 1;
                }

                let mut mono_restores = Vec::new();
                let mut repr_restores = Vec::new();
                let mut iterator_restores = Vec::new();
                let mut iterator_next_restores = Vec::new();
                let local_mono = self.infer_op_mono(op);
                self.push_flow_mono_binding(*local, local_mono.clone(), &mut mono_restores);
                self.push_flow_value_repr_binding(
                    *local,
                    local_mono
                        .as_ref()
                        .and_then(|mono| value_repr_from_mono(mono, &self.concrete_func_sigs)),
                    &mut repr_restores,
                );
                self.push_flow_iterator_binding(
                    *local,
                    iterator_state_from_setup_op(op, self),
                    &mut iterator_restores,
                );
                self.push_flow_iterator_next_binding(
                    *local,
                    iterator_next_result_state_from_op(op, self),
                    &mut iterator_next_restores,
                );
                if let AnfOp::AAssign {
                    local: target,
                    value,
                } = op.as_ref()
                {
                    let value_mono = self.infer_atom_mono(value);
                    self.push_flow_mono_binding(*target, value_mono.clone(), &mut mono_restores);
                    self.push_flow_value_repr_binding(
                        *target,
                        value_mono
                            .as_ref()
                            .and_then(|mono| value_repr_from_mono(mono, &self.concrete_func_sigs)),
                        &mut repr_restores,
                    );
                    self.push_flow_iterator_binding(
                        *target,
                        atom_iterator_state(value, self),
                        &mut iterator_restores,
                    );
                    self.push_flow_iterator_next_binding(
                        *target,
                        iterator_next_result_state_from_atom(value, self),
                        &mut iterator_next_restores,
                    );
                }
                self.assign_expr_locals(body, next_idx, wasm_locals);
                while let Some((local_id, prev)) = iterator_next_restores.pop() {
                    self.restore_flow_iterator_next_binding(local_id, prev);
                }
                while let Some((local_id, prev)) = iterator_restores.pop() {
                    self.restore_flow_iterator_binding(local_id, prev);
                }
                while let Some((local_id, prev)) = repr_restores.pop() {
                    self.restore_flow_value_repr_binding(local_id, prev);
                }
                while let Some((local_id, prev)) = mono_restores.pop() {
                    self.restore_flow_mono_binding(local_id, prev);
                }
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
                let scrutinee_mono = self.infer_atom_mono(scrutinee);
                let option_iter_item_state = iterator_next_result_state_from_atom(scrutinee, self);
                let mut pat_types: HashMap<
                    LocalId,
                    (ValType, Option<MonoType>, Option<IteratorStateInfo>),
                > = HashMap::new();
                for AnfMatchArm { pattern, .. } in arms {
                    let mut typed = Vec::new();
                    collect_pattern_locals_typed(
                        pattern,
                        scrutinee_mono.as_ref(),
                        option_iter_item_state.as_ref(),
                        self.type_env,
                        &self.concrete_func_sigs,
                        &mut typed,
                    );
                    for (local_id, inferred_ty, inferred_mono, inferred_iter_item_state) in typed {
                        pat_types
                            .entry(local_id)
                            .and_modify(|(existing_ty, existing_mono, existing_iter_item_state)| {
                                if *existing_ty != inferred_ty {
                                    *existing_ty = ValType::Anyref;
                                    *existing_mono = None;
                                    *existing_iter_item_state = None;
                                } else if *existing_mono != inferred_mono {
                                    *existing_mono = None;
                                } else if *existing_iter_item_state != inferred_iter_item_state {
                                    *existing_iter_item_state = None;
                                }
                            })
                            .or_insert((inferred_ty, inferred_mono, inferred_iter_item_state));
                    }
                }
                let mut pat_locals = pat_types.into_iter().collect::<Vec<_>>();
                pat_locals.sort_by_key(|(local_id, _)| local_id.0);
                for (local_id, (local_ty, local_mono, local_iter_item_state)) in pat_locals {
                    if !self.local_map.contains_key(&local_id) {
                        self.local_map
                            .insert(local_id, (*next_idx, local_ty.clone()));
                        if let Some(mono) = local_mono {
                            let repr = value_repr_from_mono(&mono, &self.concrete_func_sigs);
                            self.local_mono.insert(local_id, mono);
                            self.set_local_value_repr(local_id, repr);
                        } else {
                            self.set_local_value_repr(local_id, None);
                        }
                        if let Some(info) = local_iter_item_state {
                            self.set_local_iter_item_state(local_id, Some(info));
                        }
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
            AnfOp::ACall { callee, args } => self.infer_call_result_valtype(callee, args),
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
            AnfOp::AVariant {
                type_id,
                variant,
                args,
            } if !self.concrete_func_sigs.is_empty() && *type_id == UNFOLD_STEP_TYPE_ID => {
                if let Some((yield_ty, seed_ty)) = resolve_unfold_step_types(*variant, args, self) {
                    let sym = typed_unfold_step_sym(&yield_ty, &seed_ty);
                    Some(ref_named(true, &sym))
                } else {
                    Some(ref_named(true, T_VARIANT))
                }
            }
            AnfOp::AVariant { .. } => Some(ref_named(true, T_VARIANT)),
            AnfOp::AArrayLit(_) => Some(ref_named(true, T_ARRAY)),
            AnfOp::AInit { value } => self.infer_atom_valtype(value),
            AnfOp::AAssign { .. } | AnfOp::ADefer(_) => Some(ValType::I32),
            AnfOp::ALoop { body } => self.infer_loop_result_valtype(body),
            AnfOp::ARecordGet {
                target,
                field,
                type_id,
            } => self.infer_record_field_valtype(
                *type_id,
                *field,
                self.infer_atom_mono(target).as_ref(),
                iter_item_state_from_atom(target, self).as_ref(),
            ),
            AnfOp::AIndex { result_ty, .. } => Some(mono_to_valtype(result_ty, self.type_env)),
        }
    }

    fn infer_atom_valtype(&self, atom: &Atom) -> Option<ValType> {
        if let Some(info) = iter_item_state_from_atom(atom, self) {
            return Some(ref_named(true, &typed_iter_item_sym(&info)));
        }
        if let Some(info) = iterator_next_result_state_from_atom(atom, self) {
            return Some(ref_named(true, &typed_iter_option_sym(&info)));
        }
        if let Some(info) = atom_iterator_state(atom, self) {
            return Some(ref_named(true, &typed_iterator_state_sym(&info)));
        }
        self.infer_atom_mono(atom)
            .map(|mono| mono_to_valtype_specialized(&mono, self.type_env, &self.concrete_func_sigs))
            .or_else(|| match atom {
                Atom::ALocal(local_id) => self.local(*local_id).map(|(_, ty)| ty.clone()),
                _ => None,
            })
    }

    pub fn infer_atom_mono(&self, atom: &Atom) -> Option<MonoType> {
        match atom {
            Atom::ALocal(local_id) => self.local_mono.get(local_id).cloned().or_else(|| {
                self.current_func_id
                    .and_then(|func_id| self.capture_mono_by_func.get(&func_id))
                    .and_then(|m| m.get(local_id).cloned())
            }),
            Atom::AGlobalFunc(func_id) => {
                self.concrete_func_sigs
                    .get(func_id)
                    .map(|(params, ret)| MonoType::Function {
                        params: params.clone(),
                        ret: Box::new(ret.clone()),
                    })
            }
            Atom::ALitInt(_) => Some(MonoType::Int),
            Atom::ALitFloat(_) => Some(MonoType::Float),
            Atom::ALitBool(_) => Some(MonoType::Bool),
            Atom::ALitStr(_) => Some(MonoType::String),
            Atom::ALitVoid => Some(MonoType::Void),
        }
    }

    fn infer_call_result_valtype(&self, callee: &Atom, args: &[Atom]) -> Option<ValType> {
        if let Atom::AGlobalFunc(func_id) = callee {
            use crate::ir::lower::prelude as ids;

            if *func_id == ids::ITERATOR_UNFOLD {
                if let Some(info) =
                    iterator_state_from_unfold_args(args.first()?, args.get(1)?, self)
                {
                    return Some(ref_named(true, &typed_iterator_state_sym(&info)));
                }
            }
            if *func_id == ids::ITERATOR_NEXT {
                if let Some(info) = atom_iterator_state(args.first()?, self) {
                    return Some(ref_named(true, &typed_iter_option_sym(&info)));
                }
            }
        }
        if let Some(mono) = self.infer_call_result_mono(callee, args) {
            return Some(mono_to_valtype_for_call_result_abi(
                &mono,
                self.type_env,
                &self.concrete_func_sigs,
            ));
        }
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
                if let Some((_params, ret)) = self.local_typed_closure_sig(*local_id) {
                    if is_concrete_mono_type(&ret) {
                        return Some(mono_to_valtype_specialized(
                            &ret,
                            self.type_env,
                            &self.concrete_func_sigs,
                        ));
                    }
                }
                Some(ValType::Anyref)
            }
            _ => None,
        }
    }

    fn infer_expr_valtype(&self, expr: &AnfExpr) -> Option<ValType> {
        if let Some(mono) = self.infer_expr_mono(expr) {
            return Some(mono_to_valtype_specialized(
                &mono,
                self.type_env,
                &self.concrete_func_sigs,
            ));
        }
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
        target_mono: Option<&MonoType>,
        target_iter_item_state: Option<&IteratorStateInfo>,
    ) -> Option<ValType> {
        if let Some(info) = target_iter_item_state.filter(|_| type_id == ITER_ITEM_TYPE_ID) {
            return Some(match field.0 {
                0 => mono_to_valtype_specialized(
                    &info.yield_ty,
                    self.type_env,
                    &self.concrete_func_sigs,
                ),
                1 => ref_named(true, &typed_iterator_state_sym(info)),
                _ => return None,
            });
        }
        let field_ty = record_field_mono(self.type_env, type_id, field.0, target_mono)?;
        Some(mono_to_valtype_specialized(
            &field_ty,
            self.type_env,
            &self.concrete_func_sigs,
        ))
    }

    fn infer_op_mono(&self, op: &AnfOp) -> Option<MonoType> {
        match op {
            AnfOp::ACall { callee, args } => self.infer_call_result_mono(callee, args),
            AnfOp::AIf {
                then_branch,
                else_branch,
                ..
            } => {
                let then_ty = self.infer_expr_mono(then_branch);
                let else_ty = self.infer_expr_mono(else_branch);
                match (then_ty, else_ty) {
                    (Some(a), Some(b)) if a == b => Some(a),
                    (Some(a), _) if expr_always_diverges(else_branch) => Some(a),
                    (_, Some(b)) if expr_always_diverges(then_branch) => Some(b),
                    _ => None,
                }
            }
            AnfOp::AMatch { arms, .. } => {
                let mut value_ty: Option<MonoType> = None;
                for arm in arms {
                    if expr_always_diverges(&arm.body) {
                        continue;
                    }
                    let arm_ty = self.infer_expr_mono(&arm.body)?;
                    match &value_ty {
                        None => value_ty = Some(arm_ty),
                        Some(expected) if *expected == arm_ty => {}
                        Some(_) => return None,
                    }
                }
                value_ty
            }
            AnfOp::ABinOp { op, operand_ty, .. } => Some(match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    match operand_ty {
                        OpKind::Int => MonoType::Int,
                        OpKind::Float => MonoType::Float,
                        OpKind::Bool => MonoType::Bool,
                        OpKind::String => MonoType::String,
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    MonoType::Bool
                }
                BinOp::And | BinOp::Or => MonoType::Bool,
                BinOp::Assign => MonoType::Void,
            }),
            AnfOp::AUnOp { op, operand_ty, .. } => Some(match op {
                UnOp::Neg => match operand_ty {
                    OpKind::Int => MonoType::Int,
                    OpKind::Float => MonoType::Float,
                    OpKind::Bool => MonoType::Bool,
                    OpKind::String => MonoType::String,
                },
                UnOp::Not => MonoType::Bool,
            }),
            AnfOp::AMakeClosure { func_id, .. } => {
                self.concrete_func_sigs
                    .get(func_id)
                    .map(|(params, ret)| MonoType::Function {
                        params: params.clone(),
                        ret: Box::new(ret.clone()),
                    })
            }
            AnfOp::ARecord { type_id, .. } | AnfOp::ARecordUpdate { type_id, .. } => {
                Some(MonoType::named(*type_id))
            }
            AnfOp::ARecordGet {
                target,
                type_id,
                field,
            } => record_field_mono(
                self.type_env,
                *type_id,
                field.0,
                self.infer_atom_mono(target).as_ref(),
            ),
            AnfOp::AVariant {
                type_id,
                variant,
                args,
            } => {
                if !self.concrete_func_sigs.is_empty() && *type_id == UNFOLD_STEP_TYPE_ID {
                    if let Some((yield_ty, seed_ty)) =
                        resolve_unfold_step_types(*variant, args, self)
                    {
                        return Some(MonoType::Named {
                            type_id: UNFOLD_STEP_TYPE_ID,
                            args: vec![yield_ty, seed_ty],
                        });
                    }
                }
                if *type_id == OPTION_TYPE_ID && variant.0 == 1 && args.len() == 1 {
                    if let Some(inner) = self.infer_atom_mono(&args[0]) {
                        return Some(MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![inner],
                        });
                    }
                }
                Some(MonoType::named(*type_id))
            }
            AnfOp::AArrayLit(elems) => {
                let first = elems.first()?;
                let elem_ty = self.infer_atom_mono(first)?;
                if elems
                    .iter()
                    .all(|elem| self.infer_atom_mono(elem).as_ref() == Some(&elem_ty))
                {
                    Some(MonoType::Vector(Box::new(elem_ty)))
                } else {
                    None
                }
            }
            AnfOp::AIndex { result_ty, .. } => Some(result_ty.clone()),
            AnfOp::AInit { value } => self.infer_atom_mono(value),
            AnfOp::AAssign { .. } | AnfOp::ADefer(_) | AnfOp::ALoop { .. } => None,
        }
    }

    fn infer_call_result_mono(&self, callee: &Atom, args: &[Atom]) -> Option<MonoType> {
        match callee {
            Atom::AGlobalFunc(func_id) => {
                use crate::ir::lower::prelude as ids;

                match *func_id {
                    id if id == ids::CELL_NEW => {
                        let inner = self.infer_atom_mono(args.first()?)?;
                        Some(MonoType::Named {
                            type_id: CELL_TYPE_ID,
                            args: vec![inner],
                        })
                    }
                    id if id == ids::ITERATOR_UNFOLD => {
                        let seed_ty = self.infer_atom_mono(args.first()?)?;
                        let MonoType::Function { params, ret } =
                            self.infer_atom_mono(args.get(1)?)?
                        else {
                            return None;
                        };
                        if params.len() != 1 || params[0] != seed_ty {
                            return None;
                        }
                        let MonoType::Named { type_id, args } = ret.as_ref() else {
                            return None;
                        };
                        if *type_id != UNFOLD_STEP_TYPE_ID || args.len() != 2 || args[1] != seed_ty
                        {
                            return None;
                        }
                        Some(MonoType::Named {
                            type_id: ITERATOR_TYPE_ID,
                            args: vec![args[0].clone()],
                        })
                    }
                    id if id == ids::CELL_GET => match self.infer_atom_mono(args.first()?)? {
                        MonoType::Named { type_id, args } if type_id == CELL_TYPE_ID => {
                            args.into_iter().next()
                        }
                        _ => None,
                    },
                    id if id == ids::CELL_SET || id == ids::CELL_UPDATE => Some(MonoType::Void),
                    id if id == ids::ITERATOR_NEXT => infer_iterator_item_mono(args.first()?, self)
                        .map(|item_ty| MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![item_ty],
                        }),
                    _ => self
                        .user_funcs
                        .get(func_id)
                        .and_then(|sig| sig.result_mono.clone()),
                }
            }
            Atom::ALocal(local_id) => {
                if let Some((_, ret)) = self.local_typed_closure_sig(*local_id) {
                    return Some(ret);
                }
                None
            }
            _ => None,
        }
    }

    fn infer_expr_mono(&self, expr: &AnfExpr) -> Option<MonoType> {
        match expr {
            AnfExpr::Let { body, .. } => self.infer_expr_mono(body),
            AnfExpr::Atom(atom) => self.infer_atom_mono(atom),
            AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => self.infer_atom_mono(atom),
            AnfExpr::Return(None) | AnfExpr::Break(None) => Some(MonoType::Void),
            AnfExpr::Continue => None,
        }
    }
}

fn string_literal_symbol_suffix(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "empty".to_string();
    }
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        suffix.push(hex_nibble((byte >> 4) & 0x0f));
        suffix.push(hex_nibble(byte & 0x0f));
    }
    suffix
}

fn hex_nibble(nibble: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    HEX[nibble as usize] as char
}

fn record_field_mono(
    type_env: &TypeEnv,
    type_id: TypeId,
    field_idx: usize,
    target_mono: Option<&MonoType>,
) -> Option<MonoType> {
    if type_id == ITER_ITEM_TYPE_ID {
        if let Some(MonoType::Named {
            type_id: mono_type_id,
            args,
        }) = target_mono
        {
            if *mono_type_id == ITER_ITEM_TYPE_ID && args.len() == 1 {
                return match field_idx {
                    0 => args.first().cloned(),
                    1 => Some(MonoType::Named {
                        type_id: ITERATOR_TYPE_ID,
                        args: vec![args[0].clone()],
                    }),
                    _ => None,
                };
            }
        }
    }
    match type_env.get_def(type_id)? {
        TypeDef::Record { fields, .. } => fields.get(field_idx).map(|f| f.ty.clone()),
        TypeDef::Alias { target, .. } => match target {
            MonoType::Named { type_id, .. } => {
                record_field_mono(type_env, *type_id, field_idx, target_mono)
            }
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

fn should_erase_assigned_local(mono: &MonoType) -> bool {
    match mono {
        MonoType::Int
        | MonoType::Float
        | MonoType::Bool
        | MonoType::String
        | MonoType::Void
        | MonoType::Never => false,
        _ => true,
    }
}

fn is_cell_mono(mono: &MonoType) -> bool {
    matches!(mono, MonoType::Named { type_id, .. } if *type_id == CELL_TYPE_ID)
}

fn local_backend_entry_empty(info: &LocalBackendInfo) -> bool {
    info.repr.is_none()
        && info.iterator_state.is_none()
        && info.iterator_next_state.is_none()
        && info.iter_item_state.is_none()
}

pub(crate) fn value_repr_from_mono(
    mono: &MonoType,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> Option<ValueRepr> {
    if concrete_func_sigs.is_empty() {
        return None;
    }

    match mono {
        MonoType::Function { params, ret } if is_concrete_mono_type(mono) => {
            Some(ValueRepr::TypedClosure {
                params: params.clone(),
                ret: ret.as_ref().clone(),
            })
        }
        MonoType::Named { type_id, args }
            if *type_id == CELL_TYPE_ID && args.len() == 1 && is_concrete_mono_type(&args[0]) =>
        {
            Some(ValueRepr::TypedCell {
                elem_ty: args[0].clone(),
            })
        }
        _ => None,
    }
}

fn collect_assigned_locals_expr(expr: &AnfExpr, out: &mut HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_assigned_locals_op(op, out);
            collect_assigned_locals_expr(body, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_assigned_locals_op(op: &AnfOp, out: &mut HashSet<LocalId>) {
    match op {
        AnfOp::AAssign { local, .. } => {
            out.insert(*local);
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_assigned_locals_expr(then_branch, out);
            collect_assigned_locals_expr(else_branch, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_assigned_locals_expr(&arm.body, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_assigned_locals_expr(body, out);
        }
        _ => {}
    }
}

fn collect_local_binding_counts_expr(expr: &AnfExpr, out: &mut HashMap<LocalId, usize>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            *out.entry(*local).or_insert(0) += 1;
            collect_local_binding_counts_op(op, out);
            collect_local_binding_counts_expr(body, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_local_binding_counts_op(op: &AnfOp, out: &mut HashMap<LocalId, usize>) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_local_binding_counts_expr(then_branch, out);
            collect_local_binding_counts_expr(else_branch, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_local_binding_counts_expr(&arm.body, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_local_binding_counts_expr(body, out);
        }
        _ => {}
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

pub fn atom_iterator_state(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_iterator_state(*local_id),
        _ => None,
    }
}

pub fn iterator_state_from_unfold_args(
    seed: &Atom,
    step: &Atom,
    ctx: &EmitCtx<'_>,
) -> Option<IteratorStateInfo> {
    let seed_ty = ctx.infer_atom_mono(seed)?;
    let MonoType::Function { params, ret } = ctx.infer_atom_mono(step)? else {
        return None;
    };
    if params.len() != 1 || params[0] != seed_ty {
        return None;
    }
    let MonoType::Named { type_id, args } = ret.as_ref() else {
        return None;
    };
    if *type_id != UNFOLD_STEP_TYPE_ID || args.len() != 2 || args[1] != seed_ty {
        return None;
    }
    Some(IteratorStateInfo {
        yield_ty: args[0].clone(),
        seed_ty,
    })
}

fn iterator_next_result_state_from_atom(
    atom: &Atom,
    ctx: &EmitCtx<'_>,
) -> Option<IteratorStateInfo> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_iterator_next_state(*local_id),
        _ => None,
    }
}

fn iter_item_state_from_atom(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_iter_item_state(*local_id),
        _ => None,
    }
}

fn iterator_state_from_setup_op(op: &AnfOp, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id)
                if *func_id == crate::ir::lower::prelude::ITERATOR_UNFOLD =>
            {
                iterator_state_from_unfold_args(args.first()?, args.get(1)?, ctx)
            }
            _ => None,
        },
        AnfOp::ARecordGet {
            target,
            type_id,
            field,
        } if *type_id == ITER_ITEM_TYPE_ID && field.0 == 1 => {
            iter_item_state_from_atom(target, ctx)
        }
        AnfOp::AInit { value } => atom_iterator_state(value, ctx),
        _ => None,
    }
}

fn iterator_next_result_state_from_op(op: &AnfOp, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id) if *func_id == crate::ir::lower::prelude::ITERATOR_NEXT => {
                atom_iterator_state(args.first()?, ctx)
            }
            _ => None,
        },
        AnfOp::AInit { value } => iterator_next_result_state_from_atom(value, ctx),
        _ => None,
    }
}

fn infer_iterator_item_mono(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<MonoType> {
    if let Some(info) = atom_iterator_state(atom, ctx) {
        return Some(MonoType::Named {
            type_id: ITER_ITEM_TYPE_ID,
            args: vec![info.yield_ty],
        });
    }
    match ctx.infer_atom_mono(atom)? {
        MonoType::Named { type_id, args } if type_id == ITERATOR_TYPE_ID && args.len() == 1 => {
            Some(MonoType::Named {
                type_id: ITER_ITEM_TYPE_ID,
                args: vec![args[0].clone()],
            })
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
        MonoType::Vector(_) => ref_named(true, T_ARRAY),
        MonoType::Dict(_, _) => ref_named(true, T_DICT),
        MonoType::Function { .. } => ref_named(true, T_CLOSURE),
        MonoType::Var(_) | MonoType::MetaVar(_) => ValType::Anyref,
        MonoType::Named { type_id, .. } => mono_named_to_valtype(*type_id, type_env),
    }
}

pub fn mono_to_valtype_specialized(
    ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> ValType {
    match ty {
        MonoType::Function { params, ret }
            if !concrete_func_sigs.is_empty() && is_concrete_mono_type(ty) =>
        {
            ref_named(true, &typed_closure_struct_sym(params, ret))
        }
        MonoType::Named { type_id, args }
            if !concrete_func_sigs.is_empty()
                && *type_id == UNFOLD_STEP_TYPE_ID
                && args.len() == 2
                && is_concrete_mono_type(&args[0])
                && is_concrete_mono_type(&args[1]) =>
        {
            ref_named(true, &typed_unfold_step_sym(&args[0], &args[1]))
        }
        MonoType::Named { type_id, args }
            if !concrete_func_sigs.is_empty()
                && *type_id == CELL_TYPE_ID
                && args.len() == 1
                && is_concrete_mono_type(&args[0]) =>
        {
            ref_named(true, &typed_cell_struct_sym(&args[0]))
        }
        _ => mono_to_valtype(ty, type_env),
    }
}

fn mono_to_valtype_for_call_result_abi(
    ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> ValType {
    match ty {
        MonoType::Named { type_id, .. }
            if *type_id == ITERATOR_TYPE_ID
                || *type_id == UNFOLD_STEP_TYPE_ID
                || *type_id == ITER_ITEM_TYPE_ID
                || *type_id == OPTION_TYPE_ID =>
        {
            mono_to_valtype(ty, type_env)
        }
        _ => mono_to_valtype_specialized(ty, type_env, concrete_func_sigs),
    }
}

fn mono_named_to_valtype(type_id: TypeId, type_env: &TypeEnv) -> ValType {
    if type_id == CELL_TYPE_ID {
        return ref_named(true, T_ARRAY);
    }
    if type_id == ITERATOR_TYPE_ID {
        return ref_named(true, T_ITER_STATE);
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

pub fn is_typed_general_option_candidate(mono: &MonoType) -> bool {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            if let MonoType::Named {
                type_id: inner_id, ..
            } = &args[0]
            {
                if *inner_id == ITER_ITEM_TYPE_ID {
                    return false;
                }
            }
            is_concrete_mono_type(&args[0])
        }
        _ => false,
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

pub fn mono_to_symbol_key(ty: &MonoType) -> String {
    match ty {
        MonoType::Int => "Int".to_string(),
        MonoType::Float => "Float".to_string(),
        MonoType::Bool => "Bool".to_string(),
        MonoType::String => "String".to_string(),
        MonoType::Void => "Void".to_string(),
        MonoType::Never => "Never".to_string(),
        MonoType::Var(name) => name.clone(),
        MonoType::MetaVar(id) => format!("M{}", id),
        MonoType::Vector(elem) => format!("Vec_{}", mono_to_symbol_key(elem)),
        MonoType::Dict(k, v) => format!("Dict_{}_{}", mono_to_symbol_key(k), mono_to_symbol_key(v)),
        MonoType::Named { type_id, args } => {
            if args.is_empty() {
                format!("T{}", type_id.0)
            } else {
                let args_str = args
                    .iter()
                    .map(mono_to_symbol_key)
                    .collect::<Vec<_>>()
                    .join("_");
                format!("T{}_{}", type_id.0, args_str)
            }
        }
        MonoType::Function { params, ret } => {
            let params_str = params
                .iter()
                .map(mono_to_symbol_key)
                .collect::<Vec<_>>()
                .join("_");
            format!("Fn_{}_{}", params_str, mono_to_symbol_key(ret))
        }
    }
}

pub fn typed_cell_struct_sym(elem: &MonoType) -> String {
    format!("cell_{}", mono_to_symbol_key(elem))
}

/// Resolve concrete `(yield_ty, seed_ty)` for an UnfoldStep variant literal.
///
/// For `Yield(value, next_seed)`: infers types from the atom arguments.
/// For `Done` (no args): falls back to the current function's return type.
pub fn resolve_unfold_step_types(
    variant: VariantId,
    args: &[Atom],
    ctx: &EmitCtx<'_>,
) -> Option<(MonoType, MonoType)> {
    // Yield: try to infer types from the args
    if variant.0 == 1 && args.len() == 2 {
        let yield_ty = ctx.infer_atom_mono(&args[0])?;
        let seed_ty = ctx.infer_atom_mono(&args[1])?;
        if is_concrete_mono_type(&yield_ty) && is_concrete_mono_type(&seed_ty) {
            return Some((yield_ty, seed_ty));
        }
    }
    // Done or fallback: use current function's return type
    let func_id = ctx.current_func_id?;
    let sig = ctx.user_func_sig(func_id)?;
    let result_mono = sig.result_mono.as_ref()?;
    if let MonoType::Named { type_id, args } = result_mono {
        if *type_id == UNFOLD_STEP_TYPE_ID
            && args.len() == 2
            && is_concrete_mono_type(&args[0])
            && is_concrete_mono_type(&args[1])
        {
            return Some((args[0].clone(), args[1].clone()));
        }
    }
    None
}

/// Symbol for a typed UnfoldStep struct for concrete `(yield_ty, seed_ty)`.
/// e.g. `(Int, Int)` → `"unfold_step__Int__Int"`.
pub fn typed_unfold_step_sym(yield_ty: &MonoType, seed_ty: &MonoType) -> String {
    format!(
        "unfold_step__{}__{}",
        mono_to_symbol_key(yield_ty),
        mono_to_symbol_key(seed_ty),
    )
}

/// Symbol for a typed iterator-state struct for concrete `(yield_ty, seed_ty)`.
/// e.g. `(Int, Int)` → `"iter_state__Int__Int"`.
pub fn typed_iterator_state_sym(info: &IteratorStateInfo) -> String {
    format!(
        "iter_state__{}__{}",
        mono_to_symbol_key(&info.yield_ty),
        mono_to_symbol_key(&info.seed_ty),
    )
}

/// Symbol for a typed IterItem struct for a concrete iterator-state shape.
/// e.g. `(yield=Int, seed=Int)` → `"iter_item__Int__Int"`.
pub fn typed_iter_item_sym(info: &IteratorStateInfo) -> String {
    format!(
        "iter_item__{}__{}",
        mono_to_symbol_key(&info.yield_ty),
        mono_to_symbol_key(&info.seed_ty),
    )
}

/// Symbol for a typed iterator-next Option struct for a concrete iterator-state shape.
/// e.g. `(yield=Int, seed=Int)` → `"option__iter_item__Int__Int"`.
/// Symbol for a typed general Option<T> or Result<T,E> struct.
/// e.g. `Option<Int>` → `"option__Int"`, `Result<String, Int>` → `"result__String__Int"`.
pub fn typed_general_option_sym(mono: &MonoType) -> String {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            format!("option__{}", mono_to_symbol_key(&args[0]))
        }
        MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID && args.len() == 2 => {
            format!(
                "result__{}__{}",
                mono_to_symbol_key(&args[0]),
                mono_to_symbol_key(&args[1])
            )
        }
        _ => format!("typed_variant__{}", mono_to_symbol_key(mono)),
    }
}

pub fn typed_iter_option_sym(info: &IteratorStateInfo) -> String {
    format!(
        "option__iter_item__{}__{}",
        mono_to_symbol_key(&info.yield_ty),
        mono_to_symbol_key(&info.seed_ty),
    )
}

/// Symbol for a typed closure func type with the given signature.
/// e.g. `[Int, Int] -> Int` → `"closurefunc_i64_i64_i64"`.
/// Zero-param functions use the prefix `"closurefunc_nil__<ret>"`.
pub fn typed_closurefunc_sym(params: &[MonoType], ret: &MonoType) -> String {
    if params.is_empty() {
        format!("closurefunc_nil__{}", mono_to_closure_sig_tag(ret))
    } else {
        let param_tags = params
            .iter()
            .map(mono_to_closure_sig_tag)
            .collect::<Vec<_>>()
            .join("_");
        format!(
            "closurefunc_{}_{}",
            param_tags,
            mono_to_closure_sig_tag(ret)
        )
    }
}

/// Symbol for a typed closure struct with the given signature.
/// e.g. `[Int, Int] -> Int` → `"closure_i64_i64_i64"`.
pub fn typed_closure_struct_sym(params: &[MonoType], ret: &MonoType) -> String {
    if params.is_empty() {
        format!("closure_nil__{}", mono_to_closure_sig_tag(ret))
    } else {
        let param_tags = params
            .iter()
            .map(mono_to_closure_sig_tag)
            .collect::<Vec<_>>()
            .join("_");
        format!("closure_{}_{}", param_tags, mono_to_closure_sig_tag(ret))
    }
}

fn mono_to_closure_sig_tag(ty: &MonoType) -> String {
    match ty {
        MonoType::Int
        | MonoType::Float
        | MonoType::Bool
        | MonoType::String
        | MonoType::Void
        | MonoType::Never => mono_to_type_tag(ty),
        _ => mono_to_symbol_key(ty),
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
    mono_to_valtype_specialized(mono_ty, type_env, concrete_func_sigs)
}

fn collect_pattern_locals_typed(
    pattern: &CorePattern,
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
    out: &mut Vec<(
        LocalId,
        ValType,
        Option<MonoType>,
        Option<IteratorStateInfo>,
    )>,
) {
    match pattern {
        CorePattern::Var(local_id) => {
            let (ty, mono, iter_item_state) = match expected_mono {
                Some(MonoType::Void) | None => (ValType::Anyref, None, None),
                Some(MonoType::Named { type_id, args })
                    if *type_id == ITER_ITEM_TYPE_ID
                        && args.len() == 1
                        && option_iter_item_state.is_some() =>
                {
                    let info = option_iter_item_state.cloned().unwrap();
                    (
                        ref_named(true, &typed_iter_item_sym(&info)),
                        Some(MonoType::Named {
                            type_id: ITER_ITEM_TYPE_ID,
                            args: vec![args[0].clone()],
                        }),
                        Some(info),
                    )
                }
                Some(mono) => (
                    mono_to_valtype_specialized(mono, type_env, concrete_func_sigs),
                    Some(mono.clone()),
                    None,
                ),
            };
            out.push((*local_id, ty, mono, iter_item_state));
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
            let field_tys = sum_variant_field_monos(type_env, *type_id, variant.0, expected_mono);
            for (idx, field_pat) in fields.iter().enumerate() {
                let field_expected = field_tys.get(idx);
                let field_iter_item_state = (*type_id == OPTION_TYPE_ID && variant.0 == 1)
                    .then_some(option_iter_item_state)
                    .flatten();
                collect_pattern_locals_typed(
                    field_pat,
                    field_expected,
                    field_iter_item_state,
                    type_env,
                    concrete_func_sigs,
                    out,
                );
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

fn sum_variant_field_monos(
    type_env: &TypeEnv,
    type_id: TypeId,
    variant_idx: usize,
    expected_mono: Option<&MonoType>,
) -> Vec<MonoType> {
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

    if let Some(concrete) =
        concrete_builtin_sum_field_monos(source_type_id, variant_idx, expected_mono)
    {
        return concrete;
    }

    fields
        .into_iter()
        .map(|mono| {
            // Generic sum placeholders (e.g. built-in Option/Result definitions) store
            // `Void` in the field list; concrete call-site instantiations are erased to
            // `anyref` at codegen time.
            if (has_type_params || builtin_placeholder_sum) && matches!(mono, MonoType::Void) {
                MonoType::Void
            } else {
                mono
            }
        })
        .collect()
}

fn concrete_builtin_sum_field_monos(
    source_type_id: TypeId,
    variant_idx: usize,
    expected_mono: Option<&MonoType>,
) -> Option<Vec<MonoType>> {
    let MonoType::Named {
        type_id: mono_type_id,
        args,
    } = expected_mono?
    else {
        return None;
    };
    if *mono_type_id != source_type_id {
        return None;
    }
    match source_type_id {
        OPTION_TYPE_ID if args.len() == 1 => Some(match variant_idx {
            1 => vec![args[0].clone()],
            _ => Vec::new(),
        }),
        RESULT_TYPE_ID if args.len() == 2 => Some(match variant_idx {
            0 => vec![args[0].clone()],
            1 => vec![args[1].clone()],
            _ => Vec::new(),
        }),
        UNFOLD_STEP_TYPE_ID if args.len() == 2 => Some(match variant_idx {
            1 => vec![args[0].clone(), args[1].clone()],
            _ => Vec::new(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::prelude::build_prelude_map;
    use crate::ir::lower::prelude as prelude_ids;
    use crate::ir::{FieldId, VariantId};
    use crate::types::ty::{CELL_TYPE_ID, RESULT_TYPE_ID, Variant};

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
    fn local_type_init_of_runtime_int_call_stays_i64() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(102),
            name: "init_runtime_int".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(prelude_ids::STRING_LEN),
                    args: vec![Atom::ALitStr("abc".to_string())],
                }),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(2),
                    op: Box::new(AnfOp::AInit {
                        value: Atom::ALocal(LocalId(1)),
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
    fn local_type_match_result_payload_uses_concrete_type() {
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
        assert_eq!(
            *ty_ok,
            ValType::Ref {
                nullable: true,
                heap: HeapType::Named("rt_types__String".to_string()),
            }
        );
        assert_eq!(
            *ty_err,
            ValType::Ref {
                nullable: true,
                heap: HeapType::Named("rt_types__String".to_string()),
            }
        );
    }

    #[test]
    fn local_backend_repr_tracks_typed_closure_and_cell_locals() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(42),
            (vec![MonoType::Int], MonoType::Int),
        )]));

        let func = AnfFunctionDef {
            func_id: FuncId(1001),
            name: "backend_repr_locals".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AMakeClosure {
                    func_id: FuncId(42),
                    free_vars: vec![],
                }),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(2),
                    op: Box::new(AnfOp::ACall {
                        callee: Atom::AGlobalFunc(prelude_ids::CELL_NEW),
                        args: vec![Atom::ALitInt(1)],
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
                }),
            },
            return_ty: MonoType::Named {
                type_id: CELL_TYPE_ID,
                args: vec![MonoType::Int],
            },
        };

        let _locals = ctx.setup_locals(&func);
        assert_eq!(
            ctx.local_value_repr(LocalId(1)),
            Some(ValueRepr::TypedClosure {
                params: vec![MonoType::Int],
                ret: MonoType::Int
            })
        );
        assert_eq!(
            ctx.local_typed_closure_sig(LocalId(1)),
            Some((vec![MonoType::Int], MonoType::Int))
        );
        assert_eq!(ctx.local_typed_cell_elem(LocalId(2)), Some(MonoType::Int));
    }

    #[test]
    fn local_call_result_inference_requires_backend_closure_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(77),
            (vec![MonoType::Int], MonoType::Int),
        )]));

        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_mono.insert(
            LocalId(1),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::Int),
            },
        );

        assert_eq!(
            ctx.infer_call_result_valtype(&Atom::ALocal(LocalId(1)), &[Atom::ALitInt(1)]),
            Some(ValType::Anyref)
        );
        assert_eq!(
            ctx.infer_call_result_mono(&Atom::ALocal(LocalId(1)), &[Atom::ALitInt(1)]),
            None
        );

        ctx.set_local_typed_closure_sig(LocalId(1), Some((vec![MonoType::Int], MonoType::Int)));
        assert_eq!(
            ctx.infer_call_result_valtype(&Atom::ALocal(LocalId(1)), &[Atom::ALitInt(1)]),
            Some(ValType::I64)
        );
        assert_eq!(
            ctx.infer_call_result_mono(&Atom::ALocal(LocalId(1)), &[Atom::ALitInt(1)]),
            Some(MonoType::Int)
        );
    }
}
