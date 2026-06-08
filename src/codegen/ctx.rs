use std::collections::{BTreeMap, HashMap, HashSet};

use crate::codegen::prelude::{PreludeEntry, PreludeMap};
use crate::intrinsics::contracts::{self, IntrinsicAbiResult};
use crate::ir::FuncId;
use crate::ir::LocalId;
use crate::ir::VariantId;
use crate::ir::anf::analysis::{
    DivergenceOptions, collect_assigned_locals, expr_always_diverges_with,
};
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom, OpKind};
use crate::ir::core::CorePattern;
use crate::runtime::types::{
    T_ARRAY, T_CLOSURE, T_ITER_STATE, T_PDICT, T_PVEC, T_STRING, T_VARIANT,
};
use crate::syntax::ast::{BinOp, UnOp};
use crate::types::env::TypeEnv;
use crate::types::ty::{
    CELL_TYPE_ID, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID,
    TASK_TYPE_ID, TypeDef, TypeId, UNFOLD_STEP_TYPE_ID,
};
use crate::wasm::ir::{FuncSym, HeapType, ImportDef, Label, ValType};

const CTX_DIVERGENCE: DivergenceOptions = DivergenceOptions {
    empty_match_diverges: false,
};

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

/// Physical runtime representation of a sum-like value (Option, Result, etc.).
///
/// Semantic `MonoType` tells us what the value *is*; `SumRepr` tells us what
/// Wasm struct layout it *lives in* at runtime. These can differ — for example
/// a local may hold `Option<Int>` semantically but be stored as an erased
/// `$Variant` struct when it crosses a function boundary.
///
/// All typed/erased boundary conversions should consult `SumRepr` rather than
/// guessing from `MonoType` alone.
///
/// Each typed variant stores the *full* `MonoType` (e.g. `Option<Int>`, not
/// just `Int`) so callers can reference the complete type without reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumRepr {
    /// Universal erased `$Variant` struct (type_id, variant_id, payload_array).
    ErasedVariant,
    /// Typed `Option<T>` struct with concrete payload.
    /// Stores the full `Option<T>` MonoType.
    TypedOption(MonoType),
    /// Typed `Result<T, E>` struct with concrete payload fields.
    /// Stores the full `Result<T, E>` MonoType.
    TypedResult(MonoType),
}

impl SumRepr {
    /// Returns the full semantic `MonoType` this repr corresponds to.
    pub fn mono_type(&self) -> Option<&MonoType> {
        match self {
            SumRepr::ErasedVariant => None,
            SumRepr::TypedOption(mono) | SumRepr::TypedResult(mono) => Some(mono),
        }
    }

    /// Returns true if this repr is a typed (non-erased) specialization.
    pub fn is_typed(&self) -> bool {
        !matches!(self, SumRepr::ErasedVariant)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LocalBackendInfo {
    pub repr: Option<ValueRepr>,
    pub vector_builder_elem: Option<MonoType>,
    pub iterator_state: Option<IteratorStateInfo>,
    pub iterator_next_state: Option<IteratorStateInfo>,
    pub iter_item_state: Option<IteratorStateInfo>,
    /// Physical sum representation for this local. Replaces the old
    /// `typed_option` field with an explicit representation enum.
    pub sum_repr: Option<SumRepr>,
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

/// Representation-flow context: owns per-local physical representation
/// metadata (sum repr, value repr, iterator state, closure locals) and
/// provides scoped push/restore helpers for branch-sensitive flow analysis.
///
/// Extracted from `EmitCtx` so it can be tested independently and to
/// decouple representation tracking from instruction emission.
#[derive(Debug, Clone, Default)]
pub struct ReprFlowCtx {
    /// Tracks local bindings created from `AMakeClosure` so direct user calls
    /// can materialize typed closures only at concrete higher-order boundaries.
    pub closure_locals: HashMap<LocalId, (FuncId, Vec<LocalId>)>,
    /// Unified backend flow metadata for iterator-related local specialization.
    pub local_backend: HashMap<LocalId, LocalBackendInfo>,
}

impl ReprFlowCtx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.closure_locals.clear();
        self.local_backend.clear();
    }

    // -- Sum repr --

    pub fn local_sum_repr(&self, local_id: LocalId) -> Option<&SumRepr> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.sum_repr.as_ref())
    }

    pub fn set_local_sum_repr(&mut self, local_id: LocalId, repr: Option<SumRepr>) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.sum_repr = repr;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    pub fn push_flow_sum_repr_binding(
        &mut self,
        local: LocalId,
        repr: Option<SumRepr>,
    ) -> Option<SumRepr> {
        let prev = self.local_sum_repr(local).cloned();
        self.set_local_sum_repr(local, repr);
        prev
    }

    pub fn restore_flow_sum_repr_binding(&mut self, local: LocalId, prev: Option<SumRepr>) {
        self.set_local_sum_repr(local, prev);
    }

    // -- Value repr --

    pub fn local_value_repr(&self, local_id: LocalId) -> Option<ValueRepr> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.repr.clone())
    }

    pub fn set_local_value_repr(&mut self, local_id: LocalId, repr: Option<ValueRepr>) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.repr = repr;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    // -- Vector builder repr --

    pub fn local_vector_builder_elem(&self, local_id: LocalId) -> Option<MonoType> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.vector_builder_elem.clone())
    }

    pub fn set_local_vector_builder_elem(&mut self, local_id: LocalId, elem: Option<MonoType>) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.vector_builder_elem = elem;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    // -- Iterator state --

    pub fn local_iterator_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.local_backend
            .get(&local_id)
            .and_then(|info| info.iterator_state.clone())
    }

    pub fn set_local_iterator_state(&mut self, local_id: LocalId, info: Option<IteratorStateInfo>) {
        let entry = self.local_backend.entry(local_id).or_default();
        entry.iterator_state = info;
        if local_backend_entry_empty(entry) {
            self.local_backend.remove(&local_id);
        }
    }

    // -- Closure locals --

    pub fn register_closure_local(
        &mut self,
        local: LocalId,
        func_id: FuncId,
        captures: Vec<LocalId>,
    ) {
        self.closure_locals.insert(local, (func_id, captures));
    }

    pub fn closure_local(&self, local: LocalId) -> Option<&(FuncId, Vec<LocalId>)> {
        self.closure_locals.get(&local)
    }
}

pub struct EmitCtx<'a> {
    pub local_map: HashMap<LocalId, (u32, ValType)>,
    /// Explicit ANF let-op result monotypes for the current function, keyed by
    /// bound local. Populated from `AnfFunctionDef::op_result_mono`.
    pub op_result_mono: HashMap<LocalId, MonoType>,
    /// Tracks concrete monomorphic types for locals when codegen can preserve a
    /// more specific Wasm representation than plain `Anyref`.
    pub local_mono: HashMap<LocalId, MonoType>,
    pub capture_mono_by_func: HashMap<FuncId, HashMap<LocalId, MonoType>>,
    /// Representation-flow sub-context for physical repr tracking.
    pub repr_flow: ReprFlowCtx,
    assigned_locals: HashSet<LocalId>,
    rebound_locals: HashSet<LocalId>,
    in_init_func: bool,
    pub current_func_id: Option<FuncId>,
    pub current_func_name: Option<String>,
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
            op_result_mono: HashMap::new(),
            local_mono: HashMap::new(),
            capture_mono_by_func: HashMap::new(),
            repr_flow: ReprFlowCtx::new(),
            assigned_locals: HashSet::new(),
            rebound_locals: HashSet::new(),
            in_init_func: false,
            current_func_id: None,
            current_func_name: None,
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

    /// Query the physical sum representation for a local.
    pub fn local_sum_repr(&self, local_id: LocalId) -> Option<&SumRepr> {
        self.repr_flow.local_sum_repr(local_id)
    }

    /// Set the physical sum representation for a local.
    pub fn set_local_sum_repr(&mut self, local_id: LocalId, repr: Option<SumRepr>) {
        self.repr_flow.set_local_sum_repr(local_id, repr);
    }

    /// Compatibility shim: returns the full MonoType if the local holds a
    /// typed Option<T> or Result<T,E> sum repr. Callers should migrate to
    /// `local_sum_repr()` over time.
    pub fn local_typed_option(&self, local_id: LocalId) -> Option<&MonoType> {
        self.local_sum_repr(local_id).and_then(|r| r.mono_type())
    }

    /// Compatibility shim: sets a typed Option/Result sum repr from a full
    /// MonoType. Callers should migrate to `set_local_sum_repr()` over time.
    pub fn set_local_typed_option(&mut self, local_id: LocalId, mono: Option<MonoType>) {
        let repr = mono.map(|m| sum_repr_from_mono(&m));
        self.set_local_sum_repr(local_id, repr);
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
        self.op_result_mono = func.op_result_mono.clone();
        self.local_mono.clear();
        self.repr_flow.closure_locals.clear();
        self.repr_flow.local_backend.clear();
        self.assigned_locals.clear();
        self.rebound_locals.clear();
        self.label_stack.clear();
        self.loop_result_stack.clear();
        self.next_label_id = 0;
        self.in_init_func = func.name == "__init__";
        self.current_func_id = Some(func.func_id);
        self.current_func_name = Some(func.name.clone());
        self.assigned_locals = collect_assigned_locals(&func.body);
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
            // Params must stay ABI-compatible with the function signature.
            // Even if a param is reassigned in ANF, its wasm local type is
            // fixed by `FuncDef.params` and cannot be widened here.
            let erased_assignment = false;
            let erase_init_cell = self.in_init_func && is_cell_mono(&mono_ty);
            let local_repr = if erased_assignment || erase_init_cell {
                None
            } else {
                value_repr_from_mono(&mono_ty, &self.concrete_func_sigs)
            };
            if !erased_assignment && !erase_init_cell {
                self.local_mono.insert(*local_id, mono_ty.clone());
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

        self.collect_vector_builder_hints_expr(&func.body);

        let mut wasm_locals = Vec::new();
        self.assign_expr_locals(&func.body, &mut next_idx, &mut wasm_locals);

        #[cfg(debug_assertions)]
        self.verify_codegen_metadata(&func.name, func.func_id);

        wasm_locals
    }

    /// Verify internal consistency of codegen metadata after local assignment.
    ///
    /// Checks:
    /// - Iterator metadata coherence: `iterator_state` locals have Iterator mono,
    ///   `iterator_next_state` locals have Option mono, `iter_item_state` locals
    ///   have IterItem mono.
    /// - Sum repr coherence: `SumRepr::TypedOption` / `TypedResult` agree with
    ///   local mono inference.
    /// - Typed symbol consistency: named ref-type locals have corresponding type
    ///   registrations in the specialized type registry.
    #[cfg(debug_assertions)]
    fn verify_codegen_metadata(&self, func_name: &str, func_id: FuncId) {
        for (local_id, info) in &self.repr_flow.local_backend {
            let local_mono = self.local_mono.get(local_id);

            // ── Iterator metadata coherence ────────────────────────────
            if info.iterator_state.is_some()
                && let Some(mono) = local_mono
            {
                let is_iterator = matches!(
                    mono,
                    MonoType::Named { type_id, .. } if *type_id == ITERATOR_TYPE_ID
                );
                debug_assert!(
                    is_iterator,
                    "codegen verify: {func_name} (FuncId({})): L{} has iterator_state but mono is {:?}, expected Iterator",
                    func_id.0, local_id.0, mono
                );
            }
            if let Some(next_info) = &info.iterator_next_state
                && let Some(mono) = local_mono
            {
                let is_option = matches!(
                    mono,
                    MonoType::Named { type_id, .. } if *type_id == OPTION_TYPE_ID
                );
                debug_assert!(
                    is_option,
                    "codegen verify: {func_name} (FuncId({})): L{} has iterator_next_state ({:?}) but mono is {:?}, expected Option",
                    func_id.0, local_id.0, next_info, mono
                );
            }
            if let Some(item_info) = &info.iter_item_state
                && let Some(mono) = local_mono
            {
                let is_iter_item = matches!(
                    mono,
                    MonoType::Named { type_id, .. } if *type_id == ITER_ITEM_TYPE_ID
                );
                debug_assert!(
                    is_iter_item,
                    "codegen verify: {func_name} (FuncId({})): L{} has iter_item_state ({:?}) but mono is {:?}, expected IterItem",
                    func_id.0, local_id.0, item_info, mono
                );
            }
            if info.vector_builder_elem.is_some()
                && let Some((_, local_ty)) = self.local(*local_id)
            {
                let is_builder_storage = match local_ty {
                    ValType::Anyref => true,
                    ValType::Ref {
                        heap: HeapType::Named(sym),
                        ..
                    } => sym == T_ARRAY || sym == T_PVEC,
                    _ => false,
                };
                debug_assert!(
                    is_builder_storage,
                    "codegen verify: {func_name} (FuncId({})): L{} has vector_builder_elem metadata but local type is {:?}, expected Anyref, Array, or PVec-backed builder storage",
                    func_id.0, local_id.0, local_ty
                );
            }

            // ── Sum repr coherence ─────────────────────────────────────
            if let Some(sum_repr) = &info.sum_repr {
                match sum_repr {
                    SumRepr::TypedOption(repr_mono) => {
                        if let Some(mono) = local_mono {
                            debug_assert!(
                                mono == repr_mono,
                                "codegen verify: {func_name} (FuncId({})): L{} SumRepr::TypedOption({:?}) disagrees with local mono {:?}",
                                func_id.0,
                                local_id.0,
                                repr_mono,
                                mono
                            );
                        }
                    }
                    SumRepr::TypedResult(repr_mono) => {
                        if let Some(mono) = local_mono {
                            debug_assert!(
                                mono == repr_mono,
                                "codegen verify: {func_name} (FuncId({})): L{} SumRepr::TypedResult({:?}) disagrees with local mono {:?}",
                                func_id.0,
                                local_id.0,
                                repr_mono,
                                mono
                            );
                        }
                    }
                    SumRepr::ErasedVariant => {}
                }
            }
        }

        // ── Typed symbol ↔ local ref-type consistency ──────────────────
        // The specialized type registry is populated lazily during function
        // emission (request_typed_*), so we can't check against it here.
        // Instead we verify that the local's typed ref-type is consistent
        // with its per-local metadata (iterator_state, value_repr, etc.).
        for (local_id, (_idx, val_ty)) in &self.local_map {
            if let ValType::Ref {
                heap: HeapType::Named(sym),
                ..
            } = val_ty
            {
                if sym.starts_with("iter_state__") {
                    debug_assert!(
                        self.repr_flow.local_iterator_state(*local_id).is_some(),
                        "codegen verify: {func_name} (FuncId({})): L{} has ref type {sym} but no iterator_state metadata",
                        func_id.0,
                        local_id.0,
                    );
                } else if sym.starts_with("iter_item__") {
                    debug_assert!(
                        self.repr_flow
                            .local_backend
                            .get(local_id)
                            .is_some_and(|info| info.iter_item_state.is_some()),
                        "codegen verify: {func_name} (FuncId({})): L{} has ref type {sym} but no iter_item_state metadata",
                        func_id.0,
                        local_id.0,
                    );
                } else if sym.starts_with("option__iter_item__") {
                    debug_assert!(
                        self.repr_flow
                            .local_backend
                            .get(local_id)
                            .is_some_and(|info| info.iterator_next_state.is_some()),
                        "codegen verify: {func_name} (FuncId({})): L{} has ref type {sym} but no iterator_next_state metadata",
                        func_id.0,
                        local_id.0,
                    );
                } else if sym.starts_with("closure_") {
                    // Closure locals may get a typed ref via infer_op_valtype
                    // (AMakeClosure with concrete sig). Verify that at least one
                    // of: value_repr, local_mono, or closure_locals confirms it.
                    let has_repr = self
                        .repr_flow
                        .local_value_repr(*local_id)
                        .is_some_and(|r| matches!(r, ValueRepr::TypedClosure { .. }));
                    let has_func_mono = self
                        .local_mono
                        .get(local_id)
                        .is_some_and(|m| matches!(m, MonoType::Function { .. }));
                    let has_closure_local = self.repr_flow.closure_locals.contains_key(local_id);
                    debug_assert!(
                        has_repr || has_func_mono || has_closure_local,
                        "codegen verify: {func_name} (FuncId({})): L{} has closure ref type {sym} but no TypedClosure repr, Function mono, or closure_locals entry",
                        func_id.0,
                        local_id.0,
                    );
                } else if sym.starts_with("cell_") {
                    let has_repr = self
                        .repr_flow
                        .local_value_repr(*local_id)
                        .is_some_and(|r| matches!(r, ValueRepr::TypedCell { .. }));
                    let is_cell = |m: &MonoType| matches!(m, MonoType::Named { type_id, .. } if *type_id == CELL_TYPE_ID);
                    // Check both local_mono and op_result_mono: module globals
                    // skip local_mono population but retain the Cell mono in
                    // op_result_mono.
                    let has_cell_mono = self.local_mono.get(local_id).is_some_and(is_cell)
                        || self.op_result_mono.get(local_id).is_some_and(is_cell);
                    debug_assert!(
                        has_repr || has_cell_mono,
                        "codegen verify: {func_name} (FuncId({})): L{} has cell ref type {sym} but no TypedCell repr or Cell mono",
                        func_id.0,
                        local_id.0,
                    );
                }
            }
        }
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

    pub fn in_init_func(&self) -> bool {
        self.in_init_func
    }

    pub fn module_global_sym(&self, local_id: LocalId) -> Option<&String> {
        self.module_globals.get(&local_id)
    }

    pub fn local_iterator_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.repr_flow.local_iterator_state(local_id)
    }

    pub fn local_value_repr(&self, local_id: LocalId) -> Option<ValueRepr> {
        self.repr_flow.local_value_repr(local_id)
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

    pub fn local_vector_builder_elem(&self, local_id: LocalId) -> Option<MonoType> {
        self.repr_flow.local_vector_builder_elem(local_id)
    }

    pub fn local_iterator_next_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.repr_flow
            .local_backend
            .get(&local_id)
            .and_then(|info| info.iterator_next_state.clone())
    }

    pub fn local_iter_item_state(&self, local_id: LocalId) -> Option<IteratorStateInfo> {
        self.repr_flow
            .local_backend
            .get(&local_id)
            .and_then(|info| info.iter_item_state.clone())
    }

    pub(crate) fn set_local_iterator_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        self.repr_flow.set_local_iterator_state(local_id, info);
    }

    pub(crate) fn set_local_iterator_next_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        let entry = self.repr_flow.local_backend.entry(local_id).or_default();
        entry.iterator_next_state = info;
        if local_backend_entry_empty(entry) {
            self.repr_flow.local_backend.remove(&local_id);
        }
    }

    pub(crate) fn set_local_iter_item_state(
        &mut self,
        local_id: LocalId,
        info: Option<IteratorStateInfo>,
    ) {
        let entry = self.repr_flow.local_backend.entry(local_id).or_default();
        entry.iter_item_state = info;
        if local_backend_entry_empty(entry) {
            self.repr_flow.local_backend.remove(&local_id);
        }
    }

    pub(crate) fn set_local_value_repr(&mut self, local_id: LocalId, repr: Option<ValueRepr>) {
        self.repr_flow.set_local_value_repr(local_id, repr);
    }

    pub(crate) fn set_local_vector_builder_elem(
        &mut self,
        local_id: LocalId,
        elem: Option<MonoType>,
    ) {
        self.repr_flow.set_local_vector_builder_elem(local_id, elem);
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

    pub fn infer_let_op_mono_for_emit(&self, local: LocalId, op: &AnfOp) -> Option<MonoType> {
        let metadata = self
            .op_result_mono
            .get(&local)
            .cloned()
            .filter(|mono| !should_ignore_void_metadata(op, mono));
        let inferred = self.infer_op_mono(op);
        if !self.concrete_func_sigs.is_empty() && is_unfold_step_variant_op(op) {
            debug_assert!(
                metadata
                    .as_ref()
                    .or(inferred.as_ref())
                    .is_some_and(is_concrete_unfold_step_mono),
                "missing concrete UnfoldStep op-result metadata for let-bound local L{}",
                local.0
            );
        }
        match (metadata, inferred) {
            (Some(meta), Some(fresh)) if meta != fresh => {
                if should_prefer_fresh_inference_over_metadata(op) {
                    Some(fresh)
                } else {
                    Some(meta)
                }
            }
            (Some(meta), _) => Some(meta),
            (None, inferred) => inferred,
        }
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

    pub fn push_flow_vector_builder_binding(
        &mut self,
        local: LocalId,
        elem: Option<MonoType>,
        restores: &mut Vec<(LocalId, Option<MonoType>)>,
    ) {
        let prev = self.local_vector_builder_elem(local);
        self.set_local_vector_builder_elem(local, elem);
        restores.push((local, prev));
    }

    pub fn restore_flow_vector_builder_binding(&mut self, local: LocalId, prev: Option<MonoType>) {
        self.set_local_vector_builder_elem(local, prev);
    }

    pub fn push_flow_sum_repr_binding(
        &mut self,
        local: LocalId,
        repr: Option<SumRepr>,
        restores: &mut Vec<(LocalId, Option<SumRepr>)>,
    ) {
        let prev = self.local_sum_repr(local).cloned();
        self.set_local_sum_repr(local, repr);
        restores.push((local, prev));
    }

    pub fn restore_flow_sum_repr_binding(&mut self, local: LocalId, prev: Option<SumRepr>) {
        self.set_local_sum_repr(local, prev);
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
                    let mut inferred_mono =
                        if self.in_init_func && self.module_global_sym(*local).is_some() {
                            None
                        } else {
                            self.infer_let_op_mono_for_emit(*local, op)
                        };
                    let iterator_state = iterator_state_from_setup_op(op, self);
                    let iterator_next_state = iterator_next_result_state_from_op(op, self);
                    let vector_builder_elem = merge_vector_builder_elem(
                        self.local_vector_builder_elem(*local),
                        vector_builder_elem_from_setup_op(op, self),
                    );
                    // Builder handle locals stay as Anyref — the builder is a
                    // 3-slot $Array, not a $PVec.
                    if is_vector_builder_setup_op(op) {
                        inferred_mono = None;
                    }
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
                    } else if is_unfold_step_variant_op(op)
                        && inferred_mono
                            .as_ref()
                            .is_some_and(is_concrete_unfold_step_mono)
                    {
                        // UnfoldStep literals emitted as typed structs require
                        // a matching typed local representation.
                        mono_to_valtype_specialized(
                            inferred_mono.as_ref().expect("checked is_some"),
                            self.type_env,
                            &self.concrete_func_sigs,
                        )
                    } else if let Some(mono) = inferred_mono.as_ref() {
                        if matches!(op.as_ref(), AnfOp::ACall { .. }) {
                            self.infer_op_valtype(op).unwrap_or_else(|| {
                                mono_to_valtype_specialized(
                                    mono,
                                    self.type_env,
                                    &self.concrete_func_sigs,
                                )
                            })
                        } else {
                            mono_to_valtype_specialized(
                                mono,
                                self.type_env,
                                &self.concrete_func_sigs,
                            )
                        }
                    } else if is_vector_builder_setup_op(op) {
                        // Builder handle is a 3-slot $Array, not $PVec
                        ValType::Anyref
                    } else {
                        self.infer_op_valtype(op).unwrap_or(ValType::Anyref)
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
                    self.set_local_vector_builder_elem(*local, vector_builder_elem);
                    if let Some(info) = iterator_state {
                        self.set_local_iterator_state(*local, Some(info));
                    }
                    if let Some(info) = iterator_next_state {
                        self.set_local_iterator_next_state(*local, Some(info));
                    }
                    if let AnfOp::AMakeClosure { func_id, free_vars } = op.as_ref() {
                        self.repr_flow
                            .closure_locals
                            .insert(*local, (*func_id, free_vars.clone()));
                    }
                    self.local_map.insert(*local, (*next_idx, local_ty.clone()));
                    wasm_locals.push(local_ty);
                    *next_idx += 1;
                }

                let mut mono_restores = Vec::new();
                let mut repr_restores = Vec::new();
                let mut builder_restores = Vec::new();
                let mut iterator_restores = Vec::new();
                let mut iterator_next_restores = Vec::new();
                let local_mono = self.infer_let_op_mono_for_emit(*local, op);
                self.push_flow_mono_binding(*local, local_mono.clone(), &mut mono_restores);
                self.push_flow_value_repr_binding(
                    *local,
                    local_mono
                        .as_ref()
                        .and_then(|mono| value_repr_from_mono(mono, &self.concrete_func_sigs)),
                    &mut repr_restores,
                );
                self.push_flow_vector_builder_binding(
                    *local,
                    merge_vector_builder_elem(
                        self.local_vector_builder_elem(*local),
                        vector_builder_elem_from_setup_op(op, self),
                    ),
                    &mut builder_restores,
                );
                if let Some((target, elem)) = vector_builder_mutation_from_op(op, self) {
                    self.set_local_vector_builder_elem(target, elem.clone());
                    self.push_flow_vector_builder_binding(target, elem, &mut builder_restores);
                }
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
                    self.push_flow_vector_builder_binding(
                        *target,
                        vector_builder_elem_from_atom(value, self),
                        &mut builder_restores,
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
                while let Some((local_id, prev)) = builder_restores.pop() {
                    self.restore_flow_vector_builder_binding(local_id, prev);
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
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        self.local_map.entry(local_id)
                    {
                        e.insert((*next_idx, local_ty.clone()));
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

    fn collect_vector_builder_hints_expr(&mut self, expr: &AnfExpr) {
        match expr {
            AnfExpr::Let { local, op, body } => {
                self.collect_vector_builder_hints_op(op);

                let mut mono_restores = Vec::new();
                let local_mono = self.infer_let_op_mono_for_emit(*local, op);
                self.push_flow_mono_binding(*local, local_mono, &mut mono_restores);
                self.set_local_vector_builder_elem(
                    *local,
                    merge_vector_builder_elem(
                        self.local_vector_builder_elem(*local),
                        vector_builder_elem_from_setup_op(op, self),
                    ),
                );
                if let Some((target, elem)) = vector_builder_mutation_from_op(op, self) {
                    self.set_local_vector_builder_elem(target, elem);
                }
                if let AnfOp::AAssign {
                    local: target,
                    value,
                } = op.as_ref()
                {
                    let value_mono = self.infer_atom_mono(value);
                    self.push_flow_mono_binding(*target, value_mono, &mut mono_restores);
                    self.set_local_vector_builder_elem(
                        *target,
                        vector_builder_elem_from_atom(value, self),
                    );
                }
                self.collect_vector_builder_hints_expr(body);
                while let Some((local_id, prev)) = mono_restores.pop() {
                    self.restore_flow_mono_binding(local_id, prev);
                }
            }
            AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
        }
    }

    fn snapshot_builder_elems(&self) -> Vec<(LocalId, Option<MonoType>)> {
        self.repr_flow
            .local_backend
            .iter()
            .filter_map(|(id, info)| {
                if info.vector_builder_elem.is_some() {
                    Some((*id, info.vector_builder_elem.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn restore_builder_elems(&mut self, snapshot: &[(LocalId, Option<MonoType>)]) {
        // Clear any builder elems that were added during the branch
        let current_ids: Vec<LocalId> = self
            .repr_flow
            .local_backend
            .iter()
            .filter(|(_, info)| info.vector_builder_elem.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in current_ids {
            self.set_local_vector_builder_elem(id, None);
        }
        // Restore snapshot
        for (id, elem) in snapshot {
            self.set_local_vector_builder_elem(*id, elem.clone());
        }
    }

    fn collect_vector_builder_hints_op(&mut self, op: &AnfOp) {
        match op {
            AnfOp::AIf {
                then_branch,
                else_branch,
                ..
            } => {
                let snapshot = self.snapshot_builder_elems();
                self.collect_vector_builder_hints_expr(then_branch);
                let then_elems = self.snapshot_builder_elems();
                self.restore_builder_elems(&snapshot);
                self.collect_vector_builder_hints_expr(else_branch);
                let else_elems = self.snapshot_builder_elems();
                self.restore_builder_elems(&snapshot);
                self.merge_branch_builder_elems(&snapshot, &[then_elems, else_elems]);
            }
            AnfOp::AMatch { arms, .. } => {
                let snapshot = self.snapshot_builder_elems();
                let mut branch_results = Vec::new();
                for arm in arms {
                    self.collect_vector_builder_hints_expr(&arm.body);
                    branch_results.push(self.snapshot_builder_elems());
                    self.restore_builder_elems(&snapshot);
                }
                self.merge_branch_builder_elems(&snapshot, &branch_results);
            }
            AnfOp::ALoop { body } => {
                // Walk twice: first pass collects builder hints from push
                // calls inside the loop body; second pass makes those hints
                // visible to freeze calls in other arms of the same match
                // (e.g. collect pattern: push in Some arm, freeze in None arm).
                self.collect_vector_builder_hints_expr(body);
                self.collect_vector_builder_hints_expr(body);
            }
            AnfOp::ADefer(body) => {
                self.collect_vector_builder_hints_expr(body);
            }
            _ => {}
        }
    }

    fn merge_branch_builder_elems(
        &mut self,
        pre_snapshot: &[(LocalId, Option<MonoType>)],
        branches: &[Vec<(LocalId, Option<MonoType>)>],
    ) {
        if branches.is_empty() {
            return;
        }
        // Collect all locals mentioned in any branch
        let mut all_locals = std::collections::HashSet::new();
        for branch in branches {
            for (id, _) in branch {
                all_locals.insert(*id);
            }
        }
        // For each local, merge branch results conservatively.
        // A branch that didn't change a local (same as pre-snapshot) is
        // treated as "no opinion", not as disagreement.
        for local_id in all_locals {
            let pre_value = pre_snapshot
                .iter()
                .find(|(id, _)| *id == local_id)
                .and_then(|(_, e)| e.clone());
            let mut agreed: Option<Option<MonoType>> = None;
            let mut conflict = false;
            for branch in branches {
                let branch_value = branch
                    .iter()
                    .find(|(id, _)| *id == local_id)
                    .and_then(|(_, e)| e.clone());
                // Skip branches that didn't change this local
                if branch_value == pre_value {
                    continue;
                }
                match &agreed {
                    None => agreed = Some(branch_value),
                    Some(prev) if *prev == branch_value => {}
                    Some(_) => {
                        conflict = true;
                        break;
                    }
                }
            }
            if conflict {
                // Branches actively disagree — clear the hint
                self.set_local_vector_builder_elem(local_id, None);
            } else if let Some(new_value) = agreed {
                // At least one branch changed the value, all changers agree
                self.set_local_vector_builder_elem(local_id, new_value);
            }
            // If no branch changed the value, pre-snapshot state is already
            // restored — nothing to do.
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
                    (Some(a), _) if expr_always_diverges_with(else_branch, CTX_DIVERGENCE) => {
                        Some(a)
                    }
                    (_, Some(b)) if expr_always_diverges_with(then_branch, CTX_DIVERGENCE) => {
                        Some(b)
                    }
                    _ => None,
                }
            }
            AnfOp::AMatch { arms, .. } => {
                let mut value_ty: Option<ValType> = None;
                for arm in arms {
                    if expr_always_diverges_with(&arm.body, CTX_DIVERGENCE) {
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
                if !arms.is_empty()
                    && arms
                        .iter()
                        .all(|arm| expr_always_diverges_with(&arm.body, CTX_DIVERGENCE))
                {
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
        if let Atom::ALocal(local_id) = atom
            && let Some((_, ty)) = self.local(*local_id)
        {
            return Some(ty.clone());
        }
        self.infer_atom_mono(atom)
            .map(|mono| mono_to_valtype_specialized(&mono, self.type_env, &self.concrete_func_sigs))
    }

    pub fn infer_atom_mono(&self, atom: &Atom) -> Option<MonoType> {
        match atom {
            Atom::ALocal(local_id) => self.local_mono.get(local_id).cloned().or_else(|| {
                self.op_result_mono
                    .get(local_id)
                    .cloned()
                    .filter(|mono| {
                        if *mono != MonoType::Void {
                            return true;
                        }
                        match self.local(*local_id) {
                            Some((_, ty)) => *ty == ValType::I32,
                            None => true,
                        }
                    })
                    .or_else(|| {
                        self.current_func_id
                            .and_then(|func_id| self.capture_mono_by_func.get(&func_id))
                            .and_then(|m| m.get(local_id).cloned())
                    })
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

            if *func_id == ids::ITERATOR_UNFOLD
                && let Some(info) =
                    iterator_state_from_unfold_args(args.first()?, args.get(1)?, self)
            {
                return Some(ref_named(true, &typed_iterator_state_sym(&info)));
            }
            if *func_id == ids::ITERATOR_NEXT
                && let Some(info) = atom_iterator_state(args.first()?, self)
            {
                return Some(ref_named(true, &typed_iter_option_sym(&info)));
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
                if let Some((_params, ret)) = self.local_typed_closure_sig(*local_id)
                    && is_concrete_mono_type(&ret)
                {
                    return Some(mono_to_valtype_specialized(
                        &ret,
                        self.type_env,
                        &self.concrete_func_sigs,
                    ));
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
                    (Some(a), _) if expr_always_diverges_with(else_branch, CTX_DIVERGENCE) => {
                        Some(a)
                    }
                    (_, Some(b)) if expr_always_diverges_with(then_branch, CTX_DIVERGENCE) => {
                        Some(b)
                    }
                    _ => None,
                }
            }
            AnfOp::AMatch { arms, .. } => {
                let mut value_ty: Option<MonoType> = None;
                for arm in arms {
                    if expr_always_diverges_with(&arm.body, CTX_DIVERGENCE) {
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
                        OpKind::RuntimeEq => MonoType::Bool,
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    MonoType::Bool
                }
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                    MonoType::Int
                }
                BinOp::And | BinOp::Or => MonoType::Bool,
                BinOp::Assign => MonoType::Void,
                BinOp::Range => unreachable!("Range desugared to range_from call by lowerer"),
            }),
            AnfOp::AUnOp { op, operand_ty, .. } => Some(match op {
                UnOp::Neg => match operand_ty {
                    OpKind::Int => MonoType::Int,
                    OpKind::Float => MonoType::Float,
                    OpKind::Bool => MonoType::Bool,
                    OpKind::String => MonoType::String,
                    OpKind::RuntimeEq => MonoType::Bool,
                },
                UnOp::Not => MonoType::Bool,
                UnOp::BitNot => MonoType::Int,
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
                if !self.concrete_func_sigs.is_empty()
                    && *type_id == UNFOLD_STEP_TYPE_ID
                    && let Some((yield_ty, seed_ty)) =
                        resolve_unfold_step_types(*variant, args, self)
                {
                    return Some(MonoType::Named {
                        type_id: UNFOLD_STEP_TYPE_ID,
                        args: vec![yield_ty, seed_ty],
                    });
                }
                if *type_id == OPTION_TYPE_ID
                    && variant.0 == 1
                    && args.len() == 1
                    && let Some(inner) = self.infer_atom_mono(&args[0])
                {
                    return Some(MonoType::Named {
                        type_id: OPTION_TYPE_ID,
                        args: vec![inner],
                    });
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
                    id if id == ids::BYTE_TO_INT => Some(MonoType::Int),
                    id if id == ids::FLOAT_BITS => Some(MonoType::Int),
                    id if id == ids::BYTE_FROM_INT => Some(MonoType::Named {
                        type_id: OPTION_TYPE_ID,
                        args: vec![MonoType::Byte],
                    }),
                    id if id == ids::BYTE_TO_STRING => Some(MonoType::String),
                    id if id == ids::ITERATOR_NEXT => infer_iterator_item_mono(args.first()?, self)
                        .map(|item_ty| MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![item_ty],
                        }),
                    id if id == ids::VECTOR_GET => {
                        infer_vector_elem_mono(args.first()?, self).map(|elem_ty| MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![elem_ty],
                        })
                    }
                    id if id == ids::VECTOR_SET => self
                        .infer_atom_mono(args.first()?)
                        .filter(|mono| matches!(mono, MonoType::Vector(_)))
                        .map(|mono| MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![mono],
                        }),
                    id if id == ids::VECTOR_MAKE => self
                        .infer_atom_mono(args.get(1)?)
                        .map(|elem_ty| MonoType::Vector(Box::new(elem_ty))),
                    id if id == ids::VECTOR_LEN => Some(MonoType::Int),
                    id if id == ids::VECTOR_SET_UNSAFE
                        || id == ids::VECTOR_APPEND
                        || id == ids::VECTOR_CONCAT
                        || id == ids::VECTOR_SLICE
                        || id == ids::VECTOR_SET_IN_PLACE =>
                    {
                        self.infer_atom_mono(args.first()?)
                    }
                    id if id == ids::VECTOR_BUILDER_NEW => {
                        // Builder handle typed as the vector it builds; elem
                        // type comes from the builder-elem tracking populated
                        // by `vector_builder_elem_from_setup_op`.  When elem
                        // is unknown yet we fall through to the prelude entry.
                        None
                    }
                    id if id == ids::VECTOR_BUILDER_FROM => {
                        // Builder seeded from an existing vector — return the
                        // same vector type so the local gets the right i64-
                        // specialised ValType.
                        self.infer_atom_mono(args.first()?)
                    }
                    id if id == ids::VECTOR_BUILDER_PUSH || id == ids::VECTOR_BUILDER_EXTEND => {
                        Some(MonoType::Void)
                    }
                    id if id == ids::VECTOR_BUILDER_FREEZE => {
                        infer_vector_mono_from_builder_atom(args.first()?, self)
                    }
                    id if id == ids::VECTOR_SET_IN_PLACE => self.infer_atom_mono(args.first()?),
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
    if type_id == ITER_ITEM_TYPE_ID
        && let Some(MonoType::Named {
            type_id: mono_type_id,
            args,
        }) = target_mono
        && *mono_type_id == ITER_ITEM_TYPE_ID
        && args.len() == 1
    {
        return match field_idx {
            0 => args.first().cloned(),
            1 => Some(MonoType::Named {
                type_id: ITERATOR_TYPE_ID,
                args: vec![args[0].clone()],
            }),
            _ => None,
        };
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

fn should_erase_assigned_local(mono: &MonoType) -> bool {
    !matches!(
        mono,
        MonoType::Int
            | MonoType::Float
            | MonoType::Bool
            | MonoType::Byte
            | MonoType::String
            | MonoType::Void
            | MonoType::Never
    )
}

fn is_unfold_step_variant_op(op: &AnfOp) -> bool {
    matches!(
        op,
        AnfOp::AVariant { type_id, .. } if *type_id == UNFOLD_STEP_TYPE_ID
    )
}

fn should_ignore_void_metadata(op: &AnfOp, mono: &MonoType) -> bool {
    if *mono != MonoType::Void {
        return false;
    }
    match op {
        AnfOp::AAssign { .. } | AnfOp::ADefer(_) => false,
        AnfOp::AInit { value } => !matches!(value, Atom::ALitVoid),
        _ => true,
    }
}

fn should_prefer_fresh_inference_over_metadata(op: &AnfOp) -> bool {
    use crate::ir::lower::prelude as ids;
    match op {
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            ..
        } => *func_id == ids::VECTOR_BUILDER_PUSH,
        AnfOp::AInit { .. } | AnfOp::AAssign { .. } => true,
        _ => false,
    }
}

fn is_concrete_unfold_step_mono(mono: &MonoType) -> bool {
    matches!(
        mono,
        MonoType::Named { type_id, args }
            if *type_id == UNFOLD_STEP_TYPE_ID
                && args.len() == 2
                && is_concrete_mono_type(&args[0])
                && is_concrete_mono_type(&args[1])
    )
}

fn is_cell_mono(mono: &MonoType) -> bool {
    matches!(mono, MonoType::Named { type_id, .. } if *type_id == CELL_TYPE_ID)
}

fn local_backend_entry_empty(info: &LocalBackendInfo) -> bool {
    info.repr.is_none()
        && info.vector_builder_elem.is_none()
        && info.iterator_state.is_none()
        && info.iterator_next_state.is_none()
        && info.iter_item_state.is_none()
        && info.sum_repr.is_none()
}

/// Derive a `SumRepr` from a full `MonoType` (e.g. `Option<Int>` → `TypedOption(Option<Int>)`).
/// Returns `ErasedVariant` for non-Option/non-Result or unrecognized sum types.
pub fn sum_repr_from_mono(mono: &MonoType) -> SumRepr {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            SumRepr::TypedOption(mono.clone())
        }
        MonoType::Named { type_id, args: _ } if *type_id == RESULT_TYPE_ID => {
            SumRepr::TypedResult(mono.clone())
        }
        _ => SumRepr::ErasedVariant,
    }
}

pub(crate) fn value_repr_from_mono(
    mono: &MonoType,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> Option<ValueRepr> {
    match mono {
        MonoType::Function { params, ret }
            if is_concrete_mono_type(mono)
                && concrete_func_sigs
                    .values()
                    .any(|(p, r)| p == params && *r == **ret) =>
        {
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
    let abi = contracts::contract(func_id).and_then(|entry| entry.abi_result)?;
    Some(match abi {
        IntrinsicAbiResult::Anyref => ValType::Anyref,
        IntrinsicAbiResult::I64 => ValType::I64,
        IntrinsicAbiResult::RefStringNullable => ref_named(true, T_STRING),
        IntrinsicAbiResult::RefArrayNullable => ref_named(true, T_ARRAY),
    })
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
        Atom::ALocal(local_id) => ctx.local_iterator_next_state(*local_id).or_else(|| {
            let (_, local_ty) = ctx.local(*local_id)?;
            let ValType::Ref {
                heap: HeapType::Named(sym),
                ..
            } = local_ty
            else {
                return None;
            };
            ctx.requested_typed_iter_options().get(sym).cloned()
        }),
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

pub(crate) fn vector_builder_elem_from_atom(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<MonoType> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_vector_builder_elem(*local_id),
        _ => None,
    }
}

/// Returns true if `op` is a BUILDER_NEW or BUILDER_FROM call.
fn is_vector_builder_setup_op(op: &AnfOp) -> bool {
    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        ..
    } = op
    {
        func_id == &crate::ir::lower::prelude::VECTOR_BUILDER_NEW
            || func_id == &crate::ir::lower::prelude::VECTOR_BUILDER_FROM
    } else {
        false
    }
}

pub(crate) fn vector_builder_elem_from_setup_op(op: &AnfOp, ctx: &EmitCtx<'_>) -> Option<MonoType> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id)
                if *func_id == crate::ir::lower::prelude::VECTOR_BUILDER_FROM =>
            {
                infer_vector_elem_mono(args.first()?, ctx)
            }
            Atom::AGlobalFunc(func_id)
                if *func_id == crate::ir::lower::prelude::VECTOR_BUILDER_NEW =>
            {
                None
            }
            _ => None,
        },
        AnfOp::AInit { value } => vector_builder_elem_from_atom(value, ctx),
        _ => None,
    }
}

fn merge_vector_builder_elem(
    current: Option<MonoType>,
    incoming: Option<MonoType>,
) -> Option<MonoType> {
    match (current, incoming) {
        (Some(current), Some(incoming)) if current == incoming => Some(current),
        (Some(current), None) => Some(current),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
        (Some(_), Some(_)) => None,
    }
}

pub(crate) fn vector_builder_mutation_from_op(
    op: &AnfOp,
    ctx: &EmitCtx<'_>,
) -> Option<(LocalId, Option<MonoType>)> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id)
                if *func_id == crate::ir::lower::prelude::VECTOR_BUILDER_PUSH =>
            {
                let Atom::ALocal(target) = args.first()? else {
                    return None;
                };
                let incoming = ctx.infer_atom_mono(args.get(1)?);
                let merged =
                    merge_vector_builder_elem(ctx.local_vector_builder_elem(*target), incoming);
                Some((*target, merged))
            }
            Atom::AGlobalFunc(func_id)
                if *func_id == crate::ir::lower::prelude::VECTOR_BUILDER_EXTEND =>
            {
                let Atom::ALocal(target) = args.first()? else {
                    return None;
                };
                let incoming = infer_vector_elem_mono(args.get(1)?, ctx);
                let merged =
                    merge_vector_builder_elem(ctx.local_vector_builder_elem(*target), incoming);
                Some((*target, merged))
            }
            _ => None,
        },
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

pub(crate) fn infer_vector_elem_mono(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<MonoType> {
    match ctx.infer_atom_mono(atom)? {
        MonoType::Vector(elem) => Some(elem.as_ref().clone()),
        _ => None,
    }
}

pub(crate) fn infer_vector_mono_from_builder_atom(
    atom: &Atom,
    ctx: &EmitCtx<'_>,
) -> Option<MonoType> {
    if let Some(elem_ty) = vector_builder_elem_from_atom(atom, ctx) {
        return Some(MonoType::Vector(Box::new(elem_ty)));
    }
    match ctx.infer_atom_mono(atom)? {
        MonoType::Named { type_id, args } if type_id == CELL_TYPE_ID && args.len() == 1 => {
            match &args[0] {
                MonoType::Vector(elem) => Some(MonoType::Vector(elem.clone())),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn mono_to_valtype(ty: &MonoType, type_env: &TypeEnv) -> ValType {
    match ty {
        MonoType::Int => ValType::I64,
        MonoType::Float => ValType::F64,
        MonoType::Bool | MonoType::Byte => ValType::I32,
        MonoType::String => ref_named(true, T_STRING),
        MonoType::Void | MonoType::Never => ValType::I32,
        MonoType::Vector(_) => ref_named(true, T_PVEC),
        MonoType::Dict(_, _) => ref_named(true, T_PDICT),
        MonoType::Function { .. } => ref_named(true, T_CLOSURE),
        MonoType::Var(_) | MonoType::MetaVar(_) => ValType::Anyref,
        MonoType::Named { type_id, .. } => mono_named_to_valtype(*type_id, type_env),
        MonoType::ExternRef(_) => ValType::Ref {
            nullable: false,
            heap: HeapType::Extern,
        },
    }
}

pub fn mono_to_valtype_specialized(
    ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> ValType {
    match ty {
        MonoType::Function { params, ret }
            if !concrete_func_sigs.is_empty()
                && is_concrete_mono_type(ty)
                && concrete_func_sigs
                    .values()
                    .any(|(p, r)| p == params && *r == **ret) =>
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
    if type_id == TASK_TYPE_ID {
        // Phase 1: Task is a 1-element rt_types__Array wrapping the result.
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
            OpKind::RuntimeEq => ValType::I32,
        },
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ValType::I32,
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => ValType::I64,
        BinOp::And | BinOp::Or => ValType::I32,
        BinOp::Assign => ValType::I32,
        BinOp::Range => unreachable!("Range desugared to range_from call by lowerer"),
    }
}

fn unop_result_ty(op: UnOp, operand_ty: OpKind) -> ValType {
    match op {
        UnOp::Neg => match operand_ty {
            OpKind::Int => ValType::I64,
            OpKind::Float => ValType::F64,
            OpKind::Bool => ValType::I32,
            OpKind::String => ref_named(true, T_STRING),
            OpKind::RuntimeEq => ValType::I32,
        },
        UnOp::Not => ValType::I32,
        UnOp::BitNot => ValType::I64,
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
        | MonoType::Byte
        | MonoType::String
        | MonoType::Void
        | MonoType::Never => true,
        MonoType::Vector(inner) => is_concrete_mono_type(inner),
        MonoType::Dict(k, v) => is_concrete_mono_type(k) && is_concrete_mono_type(v),
        MonoType::Function { params, ret } => {
            params.iter().all(is_concrete_mono_type) && is_concrete_mono_type(ret)
        }
        MonoType::Named { args, .. } => args.iter().all(is_concrete_mono_type),
        MonoType::ExternRef(_) => true,
        MonoType::Var(_) | MonoType::MetaVar(_) => false,
    }
}

pub fn is_typed_general_option_candidate(mono: &MonoType) -> bool {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            if let MonoType::Named {
                type_id: inner_id, ..
            } = &args[0]
                && *inner_id == ITER_ITEM_TYPE_ID
            {
                return false;
            }
            is_concrete_mono_type(&args[0])
        }
        _ => false,
    }
}

pub fn is_typed_general_result_candidate(mono: &MonoType) -> bool {
    match mono {
        MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID && args.len() == 2 => {
            is_concrete_mono_type(&args[0]) && is_concrete_mono_type(&args[1])
        }
        _ => false,
    }
}

/// Check if a MonoType is a candidate for typed sum specialization (Option or Result).
pub fn is_typed_general_sum_candidate(mono: &MonoType) -> bool {
    is_typed_general_option_candidate(mono) || is_typed_general_result_candidate(mono)
}

/// Map a `MonoType` to a short tag string for use in mangled type symbols.
/// e.g. `Int` → `"i64"`, `String` → `"str"`, `Vector<Int>` → `"arr"`.
pub fn mono_to_type_tag(ty: &MonoType) -> String {
    match ty {
        MonoType::Int => "i64".to_string(),
        MonoType::Float => "f64".to_string(),
        MonoType::Bool | MonoType::Byte => "i32".to_string(),
        MonoType::String => "str".to_string(),
        MonoType::Void | MonoType::Never => "void".to_string(),
        MonoType::Vector(_) => "arr".to_string(),
        MonoType::Dict(_, _) => "dict".to_string(),
        MonoType::Function { .. } => "cls".to_string(),
        MonoType::Named { .. } => "ref".to_string(),
        MonoType::ExternRef(_) => "extern".to_string(),
        MonoType::Var(_) | MonoType::MetaVar(_) => "any".to_string(),
    }
}

pub fn mono_to_symbol_key(ty: &MonoType) -> String {
    match ty {
        MonoType::Int => "Int".to_string(),
        MonoType::Float => "Float".to_string(),
        MonoType::Bool => "Bool".to_string(),
        MonoType::Byte => "Byte".to_string(),
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
        MonoType::ExternRef(type_id) => format!("Extern{}", type_id.0),
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
/// This only uses variant payload atoms. Contextual fallback is intentionally
/// disallowed so callers must provide explicit metadata where needed.
pub fn resolve_unfold_step_types(
    variant: VariantId,
    args: &[Atom],
    ctx: &EmitCtx<'_>,
) -> Option<(MonoType, MonoType)> {
    if variant.0 != 1 || args.len() != 2 {
        return None;
    }
    let (yield_ty, seed_ty) = (
        ctx.infer_atom_mono(&args[0])?,
        ctx.infer_atom_mono(&args[1])?,
    );
    if is_concrete_mono_type(&yield_ty) && is_concrete_mono_type(&seed_ty) {
        Some((yield_ty, seed_ty))
    } else {
        None
    }
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
        | MonoType::Byte
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
                let field_iter_item_state = (*type_id == OPTION_TYPE_ID && variant.0 == 1)
                    .then_some(option_iter_item_state)
                    .flatten();
                let typed_iter_item_expected = field_iter_item_state.map(|info| MonoType::Named {
                    type_id: ITER_ITEM_TYPE_ID,
                    args: vec![info.yield_ty.clone()],
                });
                let field_expected_owned =
                    typed_iter_item_expected.or_else(|| field_tys.get(idx).cloned());
                collect_pattern_locals_typed(
                    field_pat,
                    field_expected_owned.as_ref(),
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
            Some(TypeDef::Alias {
                target: MonoType::Named { type_id, .. },
                ..
            }) => match type_env.get_def(*type_id) {
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
    use crate::runtime::types::ref_pvec_null;
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(1)).expect("missing local L1");
        assert_eq!(*ty, ValType::I64);
    }

    #[test]
    fn assigned_param_keeps_abi_local_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func = AnfFunctionDef {
            func_id: FuncId(103),
            name: "assigned_param_keeps_abi_type".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::named(crate::types::ty::RANGE_TYPE_ID)],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::AAssign {
                    local: LocalId(0),
                    value: Atom::ALocal(LocalId(0)),
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(0)))),
            },
            return_ty: MonoType::named(crate::types::ty::RANGE_TYPE_ID),
            op_result_mono: HashMap::new(),
        };

        let _locals = ctx.setup_locals(&func);
        let (_, ty) = ctx.local(LocalId(0)).expect("missing param local L0");
        assert_eq!(
            *ty,
            mono_to_valtype_specialized(
                &MonoType::named(crate::types::ty::RANGE_TYPE_ID),
                &type_env,
                &HashMap::new(),
            )
        );
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
            op_result_mono: HashMap::new(),
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
            doc: None,
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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
            op_result_mono: HashMap::new(),
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

    #[test]
    fn resolve_unfold_step_types_yield_uses_local_metadata() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let func_id = FuncId(5000);
        let func = AnfFunctionDef {
            func_id,
            name: "unfold_step_metadata".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Atom(Atom::ALitVoid),
            return_ty: MonoType::Void,
            op_result_mono: HashMap::from([
                (LocalId(10), MonoType::String),
                (LocalId(11), MonoType::Int),
            ]),
        };
        let _locals = ctx.setup_locals(&func);

        let resolved = resolve_unfold_step_types(
            VariantId(1),
            &[Atom::ALocal(LocalId(10)), Atom::ALocal(LocalId(11))],
            &ctx,
        );
        assert_eq!(resolved, Some((MonoType::String, MonoType::Int)));
    }

    #[test]
    fn infer_variant_unfold_step_uses_op_result_metadata_for_yield_payload() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let func_id = FuncId(5001);
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(42),
            (vec![MonoType::Int], MonoType::Int),
        )]));

        let func = AnfFunctionDef {
            func_id,
            name: "unfold_step_variant_metadata".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(prelude_ids::STRING_SLICE),
                    args: vec![
                        Atom::ALitStr("abc".to_string()),
                        Atom::ALitInt(0),
                        Atom::ALitInt(1),
                    ],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            },
            return_ty: MonoType::Void,
            op_result_mono: HashMap::from([(LocalId(1), MonoType::String)]),
        };
        let _locals = ctx.setup_locals(&func);
        assert_eq!(
            ctx.infer_atom_mono(&Atom::ALocal(LocalId(1))),
            Some(MonoType::String)
        );

        let op = AnfOp::AVariant {
            type_id: UNFOLD_STEP_TYPE_ID,
            variant: VariantId(1),
            args: vec![Atom::ALocal(LocalId(1)), Atom::ALitInt(1)],
        };
        assert_eq!(
            ctx.infer_op_mono_for_emit(&op),
            Some(MonoType::Named {
                type_id: UNFOLD_STEP_TYPE_ID,
                args: vec![MonoType::String, MonoType::Int],
            })
        );
    }

    #[test]
    fn infer_let_call_mono_uses_fresh_builder_push_type_when_metadata_is_stale() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let op = AnfOp::ACall {
            callee: Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_PUSH),
            args: vec![Atom::ALocal(LocalId(9)), Atom::ALitInt(1)],
        };
        let func = AnfFunctionDef {
            func_id: FuncId(5002),
            name: "stale_builder_push_metadata".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(op.clone()),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            },
            return_ty: MonoType::Void,
            op_result_mono: HashMap::from([(
                LocalId(1),
                MonoType::Vector(Box::new(MonoType::Int)),
            )]),
        };

        let _ = ctx.setup_locals(&func);
        assert_eq!(
            ctx.infer_let_op_mono_for_emit(LocalId(1), &op),
            Some(MonoType::Void)
        );
    }

    #[test]
    fn infer_vector_intrinsic_results_from_argument_types() {
        use crate::runtime::types::ref_pvec_null;
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        ctx.local_map.insert(LocalId(1), (0, ref_pvec_null()));
        ctx.local_mono
            .insert(LocalId(1), MonoType::Vector(Box::new(MonoType::Int)));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));
        ctx.local_mono.insert(
            LocalId(2),
            MonoType::Named {
                type_id: CELL_TYPE_ID,
                args: vec![MonoType::Vector(Box::new(MonoType::Int))],
            },
        );

        assert_eq!(
            ctx.infer_call_result_mono(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_MAKE),
                &[Atom::ALitInt(3), Atom::ALitInt(7)],
            ),
            Some(MonoType::Vector(Box::new(MonoType::Int)))
        );
        assert_eq!(
            ctx.infer_call_result_valtype(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_MAKE),
                &[Atom::ALitInt(3), Atom::ALitInt(7)],
            ),
            Some(ref_pvec_null())
        );
        assert_eq!(
            ctx.infer_call_result_mono(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_GET),
                &[Atom::ALocal(LocalId(1)), Atom::ALitInt(0)],
            ),
            Some(MonoType::Named {
                type_id: OPTION_TYPE_ID,
                args: vec![MonoType::Int],
            })
        );
        assert_eq!(
            ctx.infer_call_result_mono(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_SET),
                &[Atom::ALocal(LocalId(1)), Atom::ALitInt(0), Atom::ALitInt(9)],
            ),
            Some(MonoType::Named {
                type_id: OPTION_TYPE_ID,
                args: vec![MonoType::Vector(Box::new(MonoType::Int))],
            })
        );
        assert_eq!(
            ctx.infer_call_result_mono(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_FREEZE),
                &[Atom::ALocal(LocalId(2))],
            ),
            Some(MonoType::Vector(Box::new(MonoType::Int)))
        );
        assert_eq!(
            ctx.infer_call_result_valtype(
                &Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_FREEZE),
                &[Atom::ALocal(LocalId(2))],
            ),
            Some(ref_pvec_null())
        );
    }

    #[test]
    fn setup_locals_tracks_builder_family_without_forcing_cell_mono() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let func = AnfFunctionDef {
            func_id: FuncId(5003),
            name: "builder_family_local".to_string(),
            params: vec![LocalId(10)],
            param_tys: vec![MonoType::Vector(Box::new(MonoType::Int))],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_FROM),
                    args: vec![Atom::ALocal(LocalId(10))],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            },
            return_ty: MonoType::Void,
            op_result_mono: HashMap::new(),
        };

        let _ = ctx.setup_locals(&func);
        assert_eq!(
            ctx.local_vector_builder_elem(LocalId(1)),
            Some(MonoType::Int)
        );
        // Builder setup ops clear inferred_mono so the builder local
        // stays as Anyref (builder is a 3-slot $Array, not a $PVec).
        assert_eq!(ctx.local_mono.get(&LocalId(1)), None);
        // Builder locals get Anyref since inferred_mono is cleared
        assert_eq!(
            ctx.local(LocalId(1)).map(|(_, ty)| ty.clone()),
            Some(ValType::Anyref)
        );
        assert_eq!(
            infer_vector_mono_from_builder_atom(&Atom::ALocal(LocalId(1)), &ctx),
            Some(MonoType::Vector(Box::new(MonoType::Int)))
        );
    }

    #[test]
    fn builder_push_flow_enables_freeze_result_specialization() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let func = AnfFunctionDef {
            func_id: FuncId(5004),
            name: "builder_push_flow".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_NEW),
                    args: vec![],
                }),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(2),
                    op: Box::new(AnfOp::ACall {
                        callee: Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_PUSH),
                        args: vec![Atom::ALocal(LocalId(1)), Atom::ALitInt(1)],
                    }),
                    body: Box::new(AnfExpr::Let {
                        local: LocalId(3),
                        op: Box::new(AnfOp::ACall {
                            callee: Atom::AGlobalFunc(prelude_ids::VECTOR_BUILDER_FREEZE),
                            args: vec![Atom::ALocal(LocalId(1))],
                        }),
                        body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(3)))),
                    }),
                }),
            },
            return_ty: MonoType::Vector(Box::new(MonoType::Int)),
            op_result_mono: HashMap::new(),
        };

        let _ = ctx.setup_locals(&func);
        assert_eq!(
            ctx.local(LocalId(3)).map(|(_, ty)| ty.clone()),
            Some(ref_pvec_null())
        );
        assert_eq!(
            ctx.local_mono.get(&LocalId(3)),
            Some(&MonoType::Vector(Box::new(MonoType::Int)))
        );
    }

    #[test]
    fn set_local_vector_builder_elem_cleans_up_empty_entries() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        ctx.set_local_vector_builder_elem(LocalId(21), Some(MonoType::Int));
        assert!(ctx.repr_flow.local_backend.contains_key(&LocalId(21)));
        assert_eq!(
            ctx.local_vector_builder_elem(LocalId(21)),
            Some(MonoType::Int)
        );

        ctx.set_local_vector_builder_elem(LocalId(21), None);
        assert!(!ctx.repr_flow.local_backend.contains_key(&LocalId(21)));
    }

    // --- SumRepr tests ---

    #[test]
    fn sum_repr_from_option_is_typed_option() {
        let mono = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Int],
        };
        let repr = sum_repr_from_mono(&mono);
        assert!(matches!(repr, SumRepr::TypedOption(_)));
        assert!(repr.is_typed());
        assert_eq!(repr.mono_type(), Some(&mono));
    }

    #[test]
    fn sum_repr_from_result_is_typed_result() {
        let mono = MonoType::Named {
            type_id: RESULT_TYPE_ID,
            args: vec![MonoType::String, MonoType::Int],
        };
        let repr = sum_repr_from_mono(&mono);
        assert!(matches!(repr, SumRepr::TypedResult(_)));
        assert!(repr.is_typed());
        assert_eq!(repr.mono_type(), Some(&mono));
    }

    #[test]
    fn sum_repr_from_plain_type_is_erased() {
        let repr = sum_repr_from_mono(&MonoType::Int);
        assert!(matches!(repr, SumRepr::ErasedVariant));
        assert!(!repr.is_typed());
        assert_eq!(repr.mono_type(), None);
    }

    #[test]
    fn local_typed_option_shim_roundtrips_through_sum_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let option_int = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Int],
        };

        // Set via compatibility shim
        ctx.set_local_typed_option(LocalId(10), Some(option_int.clone()));

        // Read via compatibility shim
        assert_eq!(ctx.local_typed_option(LocalId(10)), Some(&option_int));

        // Read via new API
        assert!(matches!(
            ctx.local_sum_repr(LocalId(10)),
            Some(SumRepr::TypedOption(_))
        ));

        // Clear
        ctx.set_local_typed_option(LocalId(10), None);
        assert_eq!(ctx.local_typed_option(LocalId(10)), None);
        assert_eq!(ctx.local_sum_repr(LocalId(10)), None);
    }

    #[test]
    fn set_local_sum_repr_cleans_up_empty_entries() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let repr = SumRepr::TypedOption(MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::String],
        });

        ctx.set_local_sum_repr(LocalId(20), Some(repr));
        assert!(ctx.repr_flow.local_backend.contains_key(&LocalId(20)));

        ctx.set_local_sum_repr(LocalId(20), None);
        // Entry should be cleaned up since all fields are None
        assert!(!ctx.repr_flow.local_backend.contains_key(&LocalId(20)));
    }
}
