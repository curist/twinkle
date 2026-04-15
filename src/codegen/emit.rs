use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::codegen::ctx::{
    EmitCtx, FuncSigInfo, IteratorStateInfo, StringLiteralPoolEntry, SumRepr, atom_iterator_state,
    is_concrete_mono_type, is_typed_general_option_candidate, is_typed_general_result_candidate,
    is_typed_general_sum_candidate, iterator_state_from_unfold_args, mono_to_symbol_key,
    mono_to_valtype, mono_to_valtype_for_param, mono_to_valtype_specialized,
    resolve_unfold_step_types, sum_repr_from_mono, typed_cell_struct_sym, typed_closure_struct_sym,
    typed_closurefunc_sym, typed_general_option_sym, typed_iter_item_sym, typed_iter_option_sym,
    typed_iterator_state_sym, typed_unfold_step_sym, user_record_type_sym, value_repr_from_mono,
    vector_builder_elem_from_atom, vector_builder_elem_from_setup_op,
    vector_builder_mutation_from_op,
};
use crate::codegen::prelude::build_prelude_map;
use crate::intrinsics::registry::{self, LoweringKind};
use crate::ir::FuncId;
use crate::ir::anf::analysis::{
    collect_bound_locals, collect_free_locals, expr_always_diverges, op_always_diverges,
};
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom};
use crate::ir::core::CorePattern;
use crate::ir::lower::prelude as prelude_ids;
use crate::runtime::types::{
    T_ARRAY, T_BOXED_FLOAT, T_BOXED_INT, T_CLOSURE, T_CLOSURE_ENV, T_CLOSURE_FUNC, T_ITER_STATE,
    T_PVEC, T_STRING, T_VARIANT, ref_array, ref_array_null, ref_iter_state_null, ref_pdict_null,
    ref_pvec, ref_pvec_null, ref_string, ref_string_null,
};
use crate::types::env::TypeEnv;
use crate::types::ty::{
    ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID,
    TypeDef as LangTypeDef, TypeId, UNFOLD_STEP_TYPE_ID,
};
use crate::wasm::ir::{
    ExportDef, FieldDef as WasmFieldDef, FuncDef, GlobalDef, HeapType, ImportDef, Instr, ModuleIR,
    TypeDef as WasmTypeDef, ValType,
};

// ---------------------------------------------------------------------------
// Boundary conversion counters (debug builds only)
// ---------------------------------------------------------------------------

/// Lightweight counters tracking how many boundary conversions the emitter
/// performs.  Only active in debug builds (`#[cfg(debug_assertions)]`).
/// Used by characterization tests to verify that boundary paths are
/// exercised as expected.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Default)]
pub struct BoundaryCounters {
    /// Typed Option<T> → erased Variant conversions.
    pub typed_option_to_erased: u32,
    /// Typed Result<T,E> → erased Variant conversions.
    pub typed_result_to_erased: u32,
    /// Typed closure `call_ref` dispatches (concrete signature).
    pub typed_closure_calls: u32,
    /// Universal (erased) closure `call_ref` dispatches.
    pub universal_closure_calls: u32,
    /// Typed iterator state access/creation.
    pub typed_iterator_ops: u32,
    /// Typed Cell struct creation/access.
    pub typed_cell_ops: u32,
}

#[cfg(debug_assertions)]
std::thread_local! {
    static BOUNDARY_COUNTERS: std::cell::RefCell<BoundaryCounters> =
        std::cell::RefCell::new(BoundaryCounters::default());
}

#[cfg(debug_assertions)]
pub fn reset_boundary_counters() {
    BOUNDARY_COUNTERS.with(|c| *c.borrow_mut() = BoundaryCounters::default());
}

#[cfg(debug_assertions)]
pub fn boundary_counters() -> BoundaryCounters {
    BOUNDARY_COUNTERS.with(|c| c.borrow().clone())
}

#[cfg(debug_assertions)]
macro_rules! bump_boundary {
    ($field:ident) => {
        BOUNDARY_COUNTERS.with(|c| c.borrow_mut().$field += 1)
    };
}

/// ANF -> ModuleIR emission with typed closure specialization.
///
/// Emits specialized `ClosureFunc` / `Closure` struct types and typed
/// trampolines for each distinct concrete closure signature found in the
/// module.  At typed call sites a concrete `call_ref` is used — no anyref
/// arg-boxing.  Dispatch through universal closures is unchanged.
pub fn emit_user_module(anf: &AnfModule, type_env: &TypeEnv) -> ModuleIR {
    let plan = build_module_emit_plan_impl(anf, type_env);
    emit_user_module_from_plan(&plan, anf, type_env)
}

pub fn emit_named_module(
    anf: &AnfModule,
    type_env: &TypeEnv,
    namespace: &str,
    exported_names: &HashSet<String>,
) -> ModuleIR {
    let plan = build_module_emit_plan_impl(anf, type_env);
    emit_named_module_from_plan(&plan, anf, type_env, namespace, exported_names)
}

/// Build a `ModuleEmitPlan` by running all analysis/collection passes.
/// Called by `planner::build_module_emit_plan`.
pub(crate) fn build_module_emit_plan_impl(
    anf: &AnfModule,
    type_env: &TypeEnv,
) -> crate::codegen::planner::ModuleEmitPlan {
    let prelude = build_prelude_map();
    let concrete_func_sigs = collect_concrete_func_signatures(anf);
    validate_unfold_step_typing_invariants(anf, &concrete_func_sigs);
    let closure_capture_layouts = collect_closure_capture_layouts(anf);
    let user_sigs =
        build_user_sig_map_typed(anf, type_env, &closure_capture_layouts, &concrete_func_sigs);
    let mut ctx = EmitCtx::new(type_env, &prelude, &user_sigs);
    ctx.set_concrete_func_sigs(concrete_func_sigs.clone());
    let module_global_ids = collect_module_global_locals(anf);
    let module_global_map = module_global_ids
        .iter()
        .copied()
        .map(|id| (id, module_global_sym(id)))
        .collect::<HashMap<_, _>>();
    ctx.set_module_globals(module_global_map.clone());
    let capture_mono_by_func =
        collect_capture_mono_by_func(anf, &closure_capture_layouts, &mut ctx);
    ctx.set_capture_mono_by_func(capture_mono_by_func.clone());
    let user_func_iterator_states =
        collect_user_func_iterator_states(anf, &closure_capture_layouts, &mut ctx);
    let typed_cell_payloads =
        collect_typed_cell_payloads(anf, type_env, &closure_capture_layouts, &mut ctx);

    crate::codegen::planner::ModuleEmitPlan {
        concrete_func_sigs,
        closure_capture_layouts,
        user_sigs,
        module_global_ids,
        module_global_map,
        capture_mono_by_func,
        user_func_iterator_states,
        typed_cell_payloads,
    }
}

/// Emit a `ModuleIR` from a pre-computed plan.
pub(crate) fn emit_user_module_from_plan(
    plan: &crate::codegen::planner::ModuleEmitPlan,
    anf: &AnfModule,
    type_env: &TypeEnv,
) -> ModuleIR {
    let exported_names = HashSet::new();
    emit_named_module_from_plan(plan, anf, type_env, "user", &exported_names)
}

pub(crate) fn emit_named_module_from_plan(
    plan: &crate::codegen::planner::ModuleEmitPlan,
    anf: &AnfModule,
    type_env: &TypeEnv,
    namespace: &str,
    exported_names: &HashSet<String>,
) -> ModuleIR {
    let prelude = build_prelude_map();
    let user_sigs = plan.user_sigs.clone();
    let mut ctx = EmitCtx::new(type_env, &prelude, &user_sigs);
    ctx.set_concrete_func_sigs(plan.concrete_func_sigs.clone());
    ctx.set_module_globals(plan.module_global_map.clone());
    ctx.set_capture_mono_by_func(plan.capture_mono_by_func.clone());
    ctx.set_user_func_iterator_states(plan.user_func_iterator_states.clone());

    // Register typed closures and cells in the unified registry.
    for (_func_id, (params, ret)) in &plan.concrete_func_sigs {
        let sym = typed_closurefunc_sym(params, ret);
        ctx.request_typed_closure(sym, params.clone(), ret.clone());
    }
    for (sym, elem) in &plan.typed_cell_payloads {
        ctx.request_typed_cell(sym.clone(), elem.clone());
    }

    let concrete_func_sigs = &plan.concrete_func_sigs;
    let closure_capture_layouts = &plan.closure_capture_layouts;
    let module_global_ids = &plan.module_global_ids;

    let mut module = ModuleIR::new(namespace);

    // Emit typed ClosureFunc and Closure struct types from the unified registry.
    for (params, ret) in ctx.requested_typed_closures().values() {
        // Find any func_id with this signature to get ABI results
        let func_id = concrete_func_sigs
            .iter()
            .find(|(_, (p, r))| p == params && r == ret)
            .map(|(fid, _)| *fid)
            .expect("typed closure registered but no matching func_id");
        let abi_results = ctx
            .user_func_abi(func_id)
            .map(|abi| abi.results)
            .unwrap_or_else(|| panic!("missing ABI for closure func FuncId({})", func_id.0));
        module.types.push(emit_typed_closurefunc_def(
            params,
            ret,
            &abi_results,
            type_env,
            &concrete_func_sigs,
        ));
        module
            .types
            .push(emit_typed_closure_struct_def(params, ret));
    }
    for elem in ctx.requested_typed_cells().values() {
        module.types.push(emit_typed_cell_struct_def(
            elem,
            type_env,
            &concrete_func_sigs,
        ));
    }
    // Iterator-adjacent types are registered during function body emission
    // (request_typed_iterator_state, etc.) and emitted after function bodies
    // with dedup guards — see the post-emission loops below.
    module.types.extend(emit_user_record_type_defs(
        type_env,
        &ctx.concrete_func_sigs,
    ));

    module
        .globals
        .extend(module_global_ids.iter().map(|id| GlobalDef {
            name: module_global_sym(*id),
            mutable: true,
            ty: ValType::Anyref,
            init: vec![Instr::RefNull(HeapType::None)],
        }));

    for func in &anf.functions {
        let capture_locals = closure_capture_layouts
            .get(&func.func_id)
            .cloned()
            .unwrap_or_default();
        module
            .funcs
            .push(emit_func_stub(func, &capture_locals, &mut ctx));
    }

    // Emit trampolines: always emit the universal trampoline (needed for
    // closures stored in data structures). Additionally emit a typed
    // trampoline for concrete-signature functions used in typed call sites.
    for func in &anf.functions {
        let capture_count = closure_capture_layouts
            .get(&func.func_id)
            .map_or(0, std::vec::Vec::len);
        module
            .funcs
            .push(emit_user_closure_trampoline(func, capture_count, &ctx));
        if let Some((params, ret)) = concrete_func_sigs.get(&func.func_id) {
            module.funcs.push(emit_typed_closure_trampoline(
                func,
                capture_count,
                params,
                ret,
                &ctx,
            ));
        }
    }

    // Emit trampolines for prelude functions used as first-class values.
    {
        let prelude_refs = collect_prelude_func_refs(anf);
        for func_id in prelude_refs {
            if let Some(entry) = prelude.get(&func_id) {
                module
                    .funcs
                    .push(emit_prelude_closure_trampoline(func_id, entry));
            }
        }
    }

    {
        module.funcs.push(emit_iterator_next_helper());
        for info in ctx.requested_iterator_helpers().values() {
            module.funcs.push(emit_typed_iterator_next_helper(
                info,
                type_env,
                &concrete_func_sigs,
            ));
        }
    }

    // Emit typed UnfoldStep struct definitions (registered during function emission)
    for (yield_ty, seed_ty) in ctx.requested_typed_unfold_steps().values() {
        module.types.push(emit_typed_unfold_step_struct_def(
            yield_ty,
            seed_ty,
            type_env,
            &concrete_func_sigs,
        ));
    }
    for info in ctx.requested_typed_iterator_states().values() {
        if !module
            .types
            .iter()
            .any(|ty| ty.name() == typed_iterator_state_sym(info))
        {
            module.types.push(emit_typed_iterator_state_struct_def(
                info,
                type_env,
                &concrete_func_sigs,
            ));
        }
    }
    for info in ctx.requested_typed_iter_items().values() {
        if !module
            .types
            .iter()
            .any(|ty| ty.name() == typed_iter_item_sym(info))
        {
            module.types.push(emit_typed_iter_item_struct_def(
                info,
                type_env,
                &concrete_func_sigs,
            ));
        }
    }
    for info in ctx.requested_typed_iter_options().values() {
        if !module
            .types
            .iter()
            .any(|ty| ty.name() == typed_iter_option_sym(info))
        {
            module.types.push(emit_typed_iter_option_struct_def(info));
        }
    }
    // Emit typed general option structs registered during body emission.
    for (sym, mono) in ctx.requested_typed_general_options().clone() {
        if !module.types.iter().any(|ty| ty.name() == sym) {
            module.types.push(emit_typed_general_option_struct_def(
                &mono,
                type_env,
                &concrete_func_sigs,
            ));
        }
    }
    prioritize_specialized_iterator_types(&mut module);
    topologically_order_local_type_defs(&mut module);

    // Always emit parse helpers — they're small and may be referenced by intrinsics
    module.funcs.push(emit_int_from_string_helper());
    module.funcs.push(emit_from_code_point_helper());
    module.funcs.push(emit_string_utf8_bytes_helper());
    module.funcs.push(emit_string_from_utf8_helper());
    if ctx.has_import("host_parse_float") {
        module.funcs.push(emit_float_from_string_helper());
    }

    if let Some(init) = emit_user_init_func(anf) {
        module.start = Some(init.name.clone());
        module.funcs.push(init);
    }

    let string_literals = ctx.requested_string_literals().clone();
    module
        .globals
        .extend(emit_string_literal_pool_globals(&string_literals));
    module
        .funcs
        .extend(emit_string_literal_pool_getters(&string_literals));

    module.exports.extend(
        anf.functions
            .iter()
            .filter(|func| exported_names.contains(&func.name))
            .map(|func| ExportDef {
                wasm_name: func.name.clone(),
                func_sym: user_func_sym(func.func_id),
            }),
    );

    module.imports.extend(ctx.imports());

    #[cfg(debug_assertions)]
    verify_boundary_invariants(&module);

    module
}

/// Debug verifier: checks that all typed struct types referenced in
/// instructions (via ref.cast, struct.get, struct.new) have matching
/// type definitions in the module.  Catches missing type registrations
/// that would cause Wasm validation failures at runtime.
#[cfg(debug_assertions)]
fn verify_boundary_invariants(module: &ModuleIR) {
    let defined_types: HashSet<&str> = module.types.iter().map(|td| td.name()).collect();

    for func in &module.funcs {
        verify_func_boundary_refs(&func.name, &func.body, &defined_types);
    }
}

#[cfg(debug_assertions)]
fn verify_func_boundary_refs(func_name: &str, instrs: &[Instr], defined_types: &HashSet<&str>) {
    for instr in instrs {
        match instr {
            Instr::StructNew(sym) | Instr::StructGet(sym, _) | Instr::StructSet(sym, _) => {
                // Only verify user-defined types; runtime types (rt_*) are in separate modules.
                if sym.starts_with("user__") {
                    debug_assert!(
                        defined_types.contains(sym.as_str()),
                        "boundary verifier: {func_name} references struct type ${sym} which is not defined in the module"
                    );
                }
            }
            Instr::RefCast {
                heap: HeapType::Named(sym),
                ..
            }
            | Instr::RefTest {
                heap: HeapType::Named(sym),
                ..
            } => {
                if sym.starts_with("user__") {
                    debug_assert!(
                        defined_types.contains(sym.as_str()),
                        "boundary verifier: {func_name} casts to type ${sym} which is not defined in the module"
                    );
                }
            }
            Instr::If {
                then_body,
                else_body,
                ..
            } => {
                verify_func_boundary_refs(func_name, then_body, defined_types);
                verify_func_boundary_refs(func_name, else_body, defined_types);
            }
            Instr::Block { body, .. } | Instr::Loop { body, .. } => {
                verify_func_boundary_refs(func_name, body, defined_types);
            }
            _ => {}
        }
    }
}

fn emit_user_init_func(anf: &AnfModule) -> Option<FuncDef> {
    if anf.all_init_func_ids.is_empty() {
        return None;
    }
    Some(FuncDef {
        name: "__user_init".to_string(),
        params: Vec::new(),
        results: Vec::new(),
        locals: Vec::new(),
        body: anf
            .all_init_func_ids
            .iter()
            .map(|func_id| Instr::Call(user_func_sym(*func_id)))
            .collect(),
    })
}

fn emit_user_record_type_defs(
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> Vec<WasmTypeDef> {
    let mut defs = Vec::new();
    let mut next_type_id = 0_u32;
    loop {
        let type_id = TypeId(next_type_id);
        let Some(def) = type_env.get_def(type_id) else {
            break;
        };
        if let LangTypeDef::Record { fields, .. } = def {
            defs.push(WasmTypeDef::Struct {
                name: user_record_type_sym(type_id),
                supertype: None,
                non_final: false,
                fields: fields
                    .iter()
                    .enumerate()
                    .map(|(idx, field)| WasmFieldDef {
                        name: Some(format!("f{idx}")),
                        mutable: true,
                        ty: mono_to_valtype_specialized(&field.ty, type_env, concrete_func_sigs),
                    })
                    .collect(),
            });
        }
        next_type_id += 1;
    }
    defs
}

fn emit_func_stub(
    func: &AnfFunctionDef,
    capture_locals: &[crate::ir::LocalId],
    ctx: &mut EmitCtx<'_>,
) -> FuncDef {
    let extra_params = capture_locals
        .iter()
        .copied()
        .map(|local_id| (local_id, ValType::Anyref))
        .collect::<Vec<_>>();
    let locals = ctx.setup_locals_with_extra(func, &extra_params);
    let abi = ctx
        .user_func_abi(func.func_id)
        .unwrap_or_else(|| panic!("missing ABI for function FuncId({})", func.func_id.0));
    let body = emit_expr(&func.body, abi.results.first(), ctx);

    FuncDef {
        name: user_func_sym(func.func_id),
        params: abi.params,
        results: abi.results,
        locals,
        body,
    }
}

fn collect_closure_capture_layouts(anf: &AnfModule) -> HashMap<FuncId, Vec<crate::ir::LocalId>> {
    let mut captures = HashMap::<FuncId, HashSet<crate::ir::LocalId>>::new();
    for func in &anf.functions {
        collect_make_closure_captures_expr(&func.body, &mut captures);
    }

    captures
        .into_iter()
        .map(|(func_id, locals)| {
            let mut ordered = locals.into_iter().collect::<Vec<_>>();
            ordered.sort_by_key(|id| id.0);
            (func_id, ordered)
        })
        .collect()
}

fn collect_make_closure_captures_expr(
    expr: &AnfExpr,
    captures: &mut HashMap<FuncId, HashSet<crate::ir::LocalId>>,
) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_make_closure_captures_op(op, captures);
            collect_make_closure_captures_expr(body, captures);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_make_closure_captures_op(
    op: &AnfOp,
    captures: &mut HashMap<FuncId, HashSet<crate::ir::LocalId>>,
) {
    match op {
        AnfOp::AMakeClosure { func_id, free_vars } => {
            let entry = captures.entry(*func_id).or_default();
            for local_id in free_vars {
                entry.insert(*local_id);
            }
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_make_closure_captures_expr(then_branch, captures);
            collect_make_closure_captures_expr(else_branch, captures);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_make_closure_captures_expr(&arm.body, captures);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_make_closure_captures_expr(body, captures);
        }
        AnfOp::ACall { .. }
        | AnfOp::ABinOp { .. }
        | AnfOp::AUnOp { .. }
        | AnfOp::ARecord { .. }
        | AnfOp::ARecordGet { .. }
        | AnfOp::ARecordUpdate { .. }
        | AnfOp::AVariant { .. }
        | AnfOp::AArrayLit(_)
        | AnfOp::AIndex { .. }
        | AnfOp::AInit { .. }
        | AnfOp::AAssign { .. } => {}
    }
}

fn collect_capture_mono_by_func(
    anf: &AnfModule,
    closure_capture_layouts: &HashMap<FuncId, Vec<crate::ir::LocalId>>,
    ctx: &mut EmitCtx<'_>,
) -> HashMap<FuncId, HashMap<crate::ir::LocalId, MonoType>> {
    let mut out: HashMap<FuncId, HashMap<crate::ir::LocalId, MonoType>> = HashMap::new();
    loop {
        let snapshot = out.clone();
        ctx.set_capture_mono_by_func(out.clone());
        for func in &anf.functions {
            let capture_locals = closure_capture_layouts
                .get(&func.func_id)
                .cloned()
                .unwrap_or_default();
            let extra_params = capture_locals
                .iter()
                .copied()
                .map(|local_id| (local_id, ValType::Anyref))
                .collect::<Vec<_>>();
            let _locals = ctx.setup_locals_with_extra(func, &extra_params);
            collect_capture_mono_expr(&func.body, &ctx.local_mono, &ctx.op_result_mono, &mut out);
        }
        if out == snapshot {
            return out;
        }
    }
}

fn collect_capture_mono_expr(
    expr: &AnfExpr,
    local_mono: &HashMap<crate::ir::LocalId, MonoType>,
    op_result_mono: &HashMap<crate::ir::LocalId, MonoType>,
    out: &mut HashMap<FuncId, HashMap<crate::ir::LocalId, MonoType>>,
) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_capture_mono_op(op, local_mono, op_result_mono, out);
            collect_capture_mono_expr(body, local_mono, op_result_mono, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_capture_mono_op(
    op: &AnfOp,
    local_mono: &HashMap<crate::ir::LocalId, MonoType>,
    op_result_mono: &HashMap<crate::ir::LocalId, MonoType>,
    out: &mut HashMap<FuncId, HashMap<crate::ir::LocalId, MonoType>>,
) {
    match op {
        AnfOp::AMakeClosure { func_id, free_vars } => {
            let entry = out.entry(*func_id).or_default();
            for local_id in free_vars {
                if let Some(mono) = local_mono
                    .get(local_id)
                    .or_else(|| op_result_mono.get(local_id))
                {
                    entry.entry(*local_id).or_insert_with(|| mono.clone());
                }
            }
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_capture_mono_expr(then_branch, local_mono, op_result_mono, out);
            collect_capture_mono_expr(else_branch, local_mono, op_result_mono, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_capture_mono_expr(&arm.body, local_mono, op_result_mono, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_capture_mono_expr(body, local_mono, op_result_mono, out);
        }
        AnfOp::ACall { .. }
        | AnfOp::ABinOp { .. }
        | AnfOp::AUnOp { .. }
        | AnfOp::ARecord { .. }
        | AnfOp::ARecordGet { .. }
        | AnfOp::ARecordUpdate { .. }
        | AnfOp::AVariant { .. }
        | AnfOp::AArrayLit(_)
        | AnfOp::AIndex { .. }
        | AnfOp::AInit { .. }
        | AnfOp::AAssign { .. } => {}
    }
}

fn module_global_sym(local_id: crate::ir::LocalId) -> String {
    format!("global_local_{}", local_id.0)
}

fn collect_module_global_locals(anf: &AnfModule) -> Vec<crate::ir::LocalId> {
    let init_funcs = anf
        .functions
        .iter()
        .filter(|f| f.name == "__init__")
        .map(|f| f.func_id)
        .collect::<HashSet<_>>();
    let mut referenced_outside_init = HashSet::new();
    for func in &anf.functions {
        let declared = func.params.iter().copied().collect::<HashSet<_>>();
        let free = collect_free_locals(&func.body, declared);
        referenced_outside_init.extend(free);
    }

    let mut bound_in_init = HashSet::new();
    for func in &anf.functions {
        if init_funcs.contains(&func.func_id) {
            bound_in_init.extend(collect_bound_locals(&func.body));
        }
    }

    let mut globals = referenced_outside_init
        .into_iter()
        .filter(|id| bound_in_init.contains(id))
        .collect::<Vec<_>>();
    globals.sort_by_key(|id| id.0);
    globals
}

#[cfg(test)]
fn infer_capture_locals(func: &AnfFunctionDef) -> Vec<crate::ir::LocalId> {
    let declared = func.params.iter().copied().collect();
    let free = collect_free_locals(&func.body, declared);
    // Filter out locals that are assigned within the function (assign targets that
    // are declared by an earlier let/init in the same function are NOT captures).
    // The free set only contains truly undeclared locals.
    let mut ordered = free.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|id| id.0);
    ordered
}

fn mono_to_valtype_for_user_abi_result(
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

fn mono_to_valtype_for_user_abi_param(
    ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> ValType {
    match ty {
        MonoType::Function { .. } => mono_to_valtype_for_param(ty, type_env, concrete_func_sigs),
        _ => mono_to_valtype_for_user_abi_result(ty, type_env, concrete_func_sigs),
    }
}

fn user_func_sym(func_id: FuncId) -> String {
    format!("func_{}", func_id.0)
}

fn validate_unfold_step_typing_invariants(
    anf: &AnfModule,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) {
    if concrete_func_sigs.is_empty() {
        return;
    }
    for func in &anf.functions {
        validate_unfold_step_typing_expr(&func.body, func);
    }
}

fn validate_unfold_step_typing_expr(expr: &AnfExpr, func: &AnfFunctionDef) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            validate_unfold_step_typing_op(*local, op, func);
            validate_unfold_step_typing_expr(body, func);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn validate_unfold_step_typing_op(local: crate::ir::LocalId, op: &AnfOp, func: &AnfFunctionDef) {
    match op {
        AnfOp::AVariant { type_id, .. } if *type_id == UNFOLD_STEP_TYPE_ID => {
            let result_mono = func.op_result_mono.get(&local).unwrap_or_else(|| {
                panic!(
                    "missing UnfoldStep result metadata for function {} (FuncId({})), local L{}",
                    func.name, func.func_id.0, local.0
                )
            });
            if concrete_unfold_step_types(result_mono).is_none() {
                panic!(
                    "invalid UnfoldStep result metadata for function {} (FuncId({})), local L{}: {:?}",
                    func.name, func.func_id.0, local.0, result_mono
                );
            }
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            validate_unfold_step_typing_expr(then_branch, func);
            validate_unfold_step_typing_expr(else_branch, func);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                validate_unfold_step_typing_expr(&arm.body, func);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            validate_unfold_step_typing_expr(body, func);
        }
        _ => {}
    }
}

fn emit_expr(expr: &AnfExpr, return_ty: Option<&ValType>, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let Some(instrs) = emit_tail_let_call(*local, op, body, return_ty, ctx) {
                return instrs;
            }
            emit_let_expr(*local, op, body, return_ty, ctx, |ctx, body| {
                emit_expr(body, return_ty, ctx)
            })
        }
        AnfExpr::Return(None) => vec![Instr::Return],
        AnfExpr::Return(Some(atom)) => {
            let mut instrs = emit_atom(atom, return_ty, ctx);
            instrs.push(Instr::Return);
            instrs
        }
        AnfExpr::Break(value) => emit_break(value.as_ref(), ctx),
        AnfExpr::Continue => emit_continue(ctx),
        AnfExpr::Atom(atom) => {
            if let Some(ret) = return_ty {
                let mut instrs = emit_atom(atom, Some(ret), ctx);
                instrs.push(Instr::Return);
                instrs
            } else {
                let mut instrs = emit_atom(atom, None, ctx);
                if atom_produces_value(atom) {
                    instrs.push(Instr::Drop);
                }
                instrs
            }
        }
    }
}

fn emit_let_expr(
    local: crate::ir::LocalId,
    op: &AnfOp,
    body: &AnfExpr,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
    emit_body: impl FnOnce(&mut EmitCtx<'_>, &AnfExpr) -> Vec<Instr>,
) -> Vec<Instr> {
    let mut restores = Vec::new();
    let mut repr_restores = Vec::new();
    let mut builder_restores = Vec::new();
    let mut iterator_restores = Vec::new();
    let mut typed_option_restores = Vec::new();
    let local_mono = ctx.infer_let_op_mono_for_emit(local, op);
    let local_repr = local_mono
        .as_ref()
        .and_then(|mono| value_repr_from_mono(mono, &ctx.concrete_func_sigs));
    let local_builder_elem = match (
        ctx.local_vector_builder_elem(local),
        vector_builder_elem_from_setup_op(op, ctx),
    ) {
        (Some(current), Some(incoming)) if current == incoming => Some(current),
        (Some(current), None) => Some(current),
        (None, Some(incoming)) => Some(incoming),
        (Some(_), Some(_)) => None,
        (None, None) => None,
    };
    let local_iter = iterator_state_from_op(op, ctx);
    let local_opt_raw = match op {
        AnfOp::AVariant { type_id, .. }
            if *type_id == OPTION_TYPE_ID || *type_id == RESULT_TYPE_ID =>
        {
            local_mono
                .as_ref()
                .filter(|mono| is_typed_general_sum_candidate(mono))
                .cloned()
        }
        _ => typed_general_option_from_op(local, op, ctx),
    };
    let local_opt = local_opt_raw.filter(|mono| local_can_store_typed_option(local, mono, ctx));
    if let Some(mono) = local_opt.as_ref() {
        ctx.request_typed_general_option(typed_general_option_sym(mono), mono.clone());
    }
    ctx.push_flow_mono_binding(local, local_mono.clone(), &mut restores);
    ctx.push_flow_value_repr_binding(local, local_repr, &mut repr_restores);
    ctx.push_flow_vector_builder_binding(local, local_builder_elem, &mut builder_restores);
    if let Some((target, elem)) = vector_builder_mutation_from_op(op, ctx) {
        ctx.push_flow_vector_builder_binding(target, elem, &mut builder_restores);
    }
    ctx.push_flow_iterator_binding(local, local_iter, &mut iterator_restores);
    push_flow_typed_option_binding(local, local_opt, &mut typed_option_restores, ctx);
    if let AnfOp::AAssign {
        local: target,
        value,
    } = op
    {
        let value_mono = ctx.infer_atom_mono(value);
        let value_repr = value_mono
            .as_ref()
            .and_then(|mono| value_repr_from_mono(mono, &ctx.concrete_func_sigs));
        let value_iter = atom_iterator_state(value, ctx);
        let value_opt = atom_typed_general_option(value, ctx)
            .filter(|mono| local_can_store_typed_option(*target, mono, ctx));
        ctx.push_flow_mono_binding(*target, value_mono.clone(), &mut restores);
        ctx.push_flow_value_repr_binding(*target, value_repr, &mut repr_restores);
        ctx.push_flow_vector_builder_binding(
            *target,
            vector_builder_elem_from_atom(value, ctx),
            &mut builder_restores,
        );
        ctx.push_flow_iterator_binding(*target, value_iter, &mut iterator_restores);
        push_flow_typed_option_binding(*target, value_opt, &mut typed_option_restores, ctx);
    }

    let mut instrs = emit_let_binding(local, op, fn_return_ty, ctx);
    instrs.extend(emit_body(ctx, body));

    while let Some((local_id, prev)) = iterator_restores.pop() {
        ctx.restore_flow_iterator_binding(local_id, prev);
    }
    while let Some((local_id, prev)) = typed_option_restores.pop() {
        restore_flow_typed_option_binding(local_id, prev, ctx);
    }
    while let Some((local_id, prev)) = builder_restores.pop() {
        ctx.restore_flow_vector_builder_binding(local_id, prev);
    }
    while let Some((local_id, prev)) = repr_restores.pop() {
        ctx.restore_flow_value_repr_binding(local_id, prev);
    }
    while let Some((local_id, prev)) = restores.pop() {
        ctx.restore_flow_mono_binding(local_id, prev);
    }

    instrs
}

fn push_flow_typed_option_binding(
    local: crate::ir::LocalId,
    mono: Option<MonoType>,
    restores: &mut Vec<(crate::ir::LocalId, Option<SumRepr>)>,
    ctx: &mut EmitCtx<'_>,
) {
    let repr = mono.map(|m| sum_repr_from_mono(&m));
    ctx.push_flow_sum_repr_binding(local, repr, restores);
}

fn restore_flow_typed_option_binding(
    local: crate::ir::LocalId,
    prev: Option<SumRepr>,
    ctx: &mut EmitCtx<'_>,
) {
    ctx.restore_flow_sum_repr_binding(local, prev);
}

/// Check if a Wasm local's physical type can hold a typed option/result struct.
///
/// Sum-boundary rule: a typed Option/Result struct can only be stored in a local
/// whose Wasm type is either `Anyref` (universal) or a matching named ref type
/// (e.g. `(ref null $option__i64)`). Storing in a mismatched ref type would
/// cause a runtime cast failure.
fn local_can_store_typed_option(
    local: crate::ir::LocalId,
    mono: &MonoType,
    ctx: &EmitCtx<'_>,
) -> bool {
    let Some((_, local_ty)) = ctx.local(local) else {
        return false;
    };
    match local_ty {
        ValType::Anyref => true,
        ValType::Ref {
            heap: HeapType::Named(name),
            ..
        } => name == &typed_general_option_sym(mono),
        _ => false,
    }
}

/// Check if a value being stored into a local can keep typed sum repr without
/// needing coercion to the local's declared Wasm type.
///
/// Returns true when both the destination local and source value agree on the
/// same typed sum representation (e.g. both hold `Option<Int>` as a typed struct).
fn can_preserve_typed_sum(dest: crate::ir::LocalId, value: &Atom, ctx: &EmitCtx<'_>) -> bool {
    // Preserving typed-sum representation only makes sense for locals that can
    // actually store that representation (typically `anyref`, or a matching
    // typed option/result struct type). Otherwise we must emit the destination
    // coercion to keep wasm stack types valid.
    let dst_repr = ctx.local_sum_repr(dest);
    let value_typed_option = atom_typed_general_option(value, ctx);
    match (dst_repr, value_typed_option.as_ref()) {
        (Some(SumRepr::TypedOption(dst_mono)), Some(src_mono))
        | (Some(SumRepr::TypedResult(dst_mono)), Some(src_mono)) => {
            dst_mono == src_mono && local_can_store_typed_option(dest, dst_mono, ctx)
        }
        _ => false,
    }
}

fn local_has_typed_sum_repr(local: crate::ir::LocalId, ctx: &EmitCtx<'_>) -> bool {
    match ctx.local_sum_repr(local) {
        Some(SumRepr::TypedOption(mono)) | Some(SumRepr::TypedResult(mono)) => {
            local_can_store_typed_option(local, mono, ctx)
        }
        _ => false,
    }
}

fn emit_let_binding(
    local: crate::ir::LocalId,
    op: &AnfOp,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let (bind_idx, bind_ty) = ctx
        .local(local)
        .cloned()
        .unwrap_or_else(|| panic!("missing local mapping for L{}", local.0));

    match op {
        AnfOp::AInit { value } => {
            let global_sym = ctx.module_global_sym(local).cloned();
            let preserve = can_preserve_typed_sum(local, value, ctx);
            let expected = if preserve { None } else { Some(&bind_ty) };
            let mut instrs = emit_atom(value, expected, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            if let Some(global_sym) = global_sym {
                instrs.extend(emit_coerce_local(bind_idx, &bind_ty, &ValType::Anyref, ctx));
                instrs.push(Instr::GlobalSet(global_sym));
            }
            instrs
        }
        AnfOp::AAssign {
            local: target,
            value,
        } => {
            let mut instrs = Vec::new();
            let target_global_sym = ctx.module_global_sym(*target).cloned();

            if let Some((target_idx, target_ty)) = ctx.local(*target).cloned() {
                let preserve = can_preserve_typed_sum(*target, value, ctx);
                let expected = if preserve { None } else { Some(&target_ty) };
                instrs.extend(emit_atom(value, expected, ctx));
                instrs.push(Instr::LocalSet(target_idx));
                if let Some(global_sym) = target_global_sym.clone() {
                    instrs.extend(emit_coerce_local(
                        target_idx,
                        &target_ty,
                        &ValType::Anyref,
                        ctx,
                    ));
                    instrs.push(Instr::GlobalSet(global_sym));
                }
            } else if let Some(global_sym) = target_global_sym {
                instrs.extend(emit_atom(value, Some(&ValType::Anyref), ctx));
                instrs.push(Instr::GlobalSet(global_sym));
            } else {
                panic!("missing assignment target mapping for L{}", target.0);
            }

            // AAssign produces Void; materialize the synthetic result in the binding local.
            instrs.extend(emit_void_value(Some(&bind_ty)));
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::ABinOp {
            op,
            left,
            right,
            operand_ty,
        } => {
            let mut instrs = emit_binop(*op, left, right, *operand_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AUnOp {
            op,
            expr,
            operand_ty,
        } => {
            let mut instrs = emit_unop(*op, expr, *operand_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            let mut instrs = emit_atom(cond, Some(&ValType::I32), ctx);
            // Preserve typed Option/Result representation across `if` joins when
            // this let-binding is flow-tracked as a typed sum.
            let preserve_typed_sum = local_has_typed_sum_repr(local, ctx);
            let then_body = if preserve_typed_sum {
                emit_expr_value_with_expected(then_branch, None, fn_return_ty, ctx)
            } else {
                emit_expr_value(then_branch, &bind_ty, fn_return_ty, ctx)
            };
            let else_body = if preserve_typed_sum {
                emit_expr_value_with_expected(else_branch, None, fn_return_ty, ctx)
            } else {
                emit_expr_value(else_branch, &bind_ty, fn_return_ty, ctx)
            };
            let both_arms_diverge =
                expr_always_diverges(then_branch) && expr_always_diverges(else_branch);
            instrs.push(Instr::If {
                result: if both_arms_diverge {
                    None
                } else {
                    Some(bind_ty.clone())
                },
                then_body,
                else_body,
            });
            if !both_arms_diverge {
                instrs.push(Instr::LocalSet(bind_idx));
            }
            instrs
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let preserve_typed_sum = local_has_typed_sum_repr(local, ctx);
            let mut instrs = emit_match_op(
                scrutinee,
                arms,
                &bind_ty,
                preserve_typed_sum,
                fn_return_ty,
                ctx,
            );
            if !op_always_diverges(op) {
                instrs.push(Instr::LocalSet(bind_idx));
            }
            instrs
        }
        AnfOp::ALoop { body } => {
            let mut instrs = emit_loop_op(body, &bind_ty, fn_return_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::ACall { callee, args } => {
            let mut instrs = emit_call(callee, args, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::ARecord { type_id, fields } => {
            let mut instrs = emit_record_literal(*type_id, fields, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::ARecordGet {
            target,
            field,
            type_id,
        } => {
            let mut instrs = emit_record_get(*type_id, *field, target, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::ARecordUpdate {
            base,
            field,
            value,
            can_reuse_in_place,
            type_id,
        } => {
            let mut instrs = emit_record_update(
                *type_id,
                *field,
                base,
                value,
                *can_reuse_in_place,
                &bind_ty,
                ctx,
            );
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AVariant {
            type_id,
            variant,
            args,
        } => {
            let local_mono = ctx.local_mono.get(&local).cloned();
            let mut instrs =
                emit_variant_literal(*type_id, *variant, args, &bind_ty, local_mono.as_ref(), ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AArrayLit(elems) => {
            let elem_mono = ctx.local_mono.get(&local).and_then(|mono| match mono {
                MonoType::Vector(elem) => Some(elem.as_ref().clone()),
                _ => None,
            });
            let mut instrs = emit_array_literal(elems, &bind_ty, elem_mono.as_ref(), ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AIndex {
            base,
            index,
            base_ty,
            ..
        } => {
            let mut instrs = emit_index_op(base, index, *base_ty, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AMakeClosure { func_id, free_vars } => {
            let mut instrs = emit_make_closure(*func_id, free_vars, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        _ => panic!("let-op emission not implemented: {:?}", op),
    }
}

fn emit_match_op(
    scrutinee: &Atom,
    arms: &[AnfMatchArm],
    bind_ty: &ValType,
    preserve_typed_sum: bool,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let scrutinee_typed_option = atom_typed_general_option(scrutinee, ctx);
    // Use None expected_ty for typed option scrutinees so we get the raw typed
    // struct value (not converted to erased Variant). Other scrutinees use Anyref.
    let scrutinee_expected = if scrutinee_typed_option.is_some() {
        None
    } else {
        Some(ValType::Anyref)
    };
    let scrutinee_anyref = emit_atom(scrutinee, scrutinee_expected.as_ref(), ctx);
    let scrutinee_mono = ctx.infer_atom_mono(scrutinee);
    let scrutinee_iter_option = atom_iterator_next_state(scrutinee, ctx);
    let scrutinee_unfold_step = atom_typed_unfold_step(scrutinee, ctx);
    emit_match_arm_chain(
        &scrutinee_anyref,
        scrutinee_mono.as_ref(),
        scrutinee_iter_option.as_ref(),
        scrutinee_unfold_step.as_ref(),
        scrutinee_typed_option.as_ref(),
        arms,
        bind_ty,
        preserve_typed_sum,
        fn_return_ty,
        ctx,
    )
}

fn emit_match_arm_chain(
    scrutinee_anyref: &[Instr],
    scrutinee_mono: Option<&MonoType>,
    scrutinee_iter_option: Option<&IteratorStateInfo>,
    scrutinee_unfold_step: Option<&(MonoType, MonoType)>,
    scrutinee_typed_option: Option<&MonoType>,
    arms: &[AnfMatchArm],
    bind_ty: &ValType,
    preserve_typed_sum: bool,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if arms.is_empty() {
        return emit_non_exhaustive_match_fallback(ctx);
    }

    let head = &arms[0];
    let mut instrs = emit_pattern_condition(
        &head.pattern,
        scrutinee_anyref,
        scrutinee_mono,
        scrutinee_iter_option,
        scrutinee_unfold_step,
        scrutinee_typed_option,
        ctx,
    );
    let mut then_body = emit_pattern_bindings(
        &head.pattern,
        scrutinee_anyref,
        scrutinee_mono,
        scrutinee_iter_option,
        scrutinee_unfold_step,
        scrutinee_typed_option,
        ctx,
    );
    let mut mono_restores = Vec::new();
    push_pattern_mono_bindings(
        &head.pattern,
        scrutinee_mono,
        scrutinee_iter_option,
        ctx,
        &mut mono_restores,
    );
    let expected_ty = if preserve_typed_sum {
        None
    } else {
        Some(bind_ty)
    };
    then_body.extend(emit_expr_value_with_expected(
        &head.body,
        expected_ty,
        fn_return_ty,
        ctx,
    ));
    while let Some((local_id, prev)) = mono_restores.pop() {
        ctx.restore_flow_mono_binding(local_id, prev);
    }
    let tail_diverges = match_chain_always_diverges(&arms[1..]);
    let mut else_body = emit_match_arm_chain(
        scrutinee_anyref,
        scrutinee_mono,
        scrutinee_iter_option,
        scrutinee_unfold_step,
        scrutinee_typed_option,
        &arms[1..],
        bind_ty,
        preserve_typed_sum,
        fn_return_ty,
        ctx,
    );
    if tail_diverges {
        else_body.push(Instr::Unreachable);
    }
    let both_arms_diverge = expr_always_diverges(&head.body) && tail_diverges;
    instrs.push(Instr::If {
        result: if both_arms_diverge {
            None
        } else {
            Some(bind_ty.clone())
        },
        then_body,
        else_body,
    });
    instrs
}

fn push_pattern_mono_bindings(
    pattern: &CorePattern,
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    ctx: &mut EmitCtx<'_>,
    restores: &mut Vec<(crate::ir::LocalId, Option<MonoType>)>,
) {
    match pattern {
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
        CorePattern::Var(local_id) => {
            ctx.push_flow_mono_binding(*local_id, expected_mono.cloned(), restores);
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
            let typed_iter_option_fields = typed_iter_option_pattern_info(
                *type_id,
                expected_mono,
                option_iter_item_state,
                ctx,
            )
            .map(|(_, field_monos)| field_monos);
            for (idx, field_pat) in fields.iter().enumerate() {
                let field_expected = typed_iter_option_fields
                    .as_ref()
                    .and_then(|field_monos| field_monos.get(idx))
                    .or_else(|| {
                        expected_mono
                            .and_then(|mono| pattern_variant_field_mono(mono, *variant, idx))
                    });
                push_pattern_mono_bindings(
                    field_pat,
                    field_expected,
                    option_iter_item_state,
                    ctx,
                    restores,
                );
            }
        }
    }
}

fn match_chain_always_diverges(arms: &[AnfMatchArm]) -> bool {
    if arms.is_empty() {
        // Empty-arm fallback is `trap` + `unreachable`.
        return true;
    }
    let head = &arms[0];
    expr_always_diverges(&head.body) && match_chain_always_diverges(&arms[1..])
}

fn emit_pattern_condition(
    pattern: &CorePattern,
    value_anyref_instrs: &[Instr],
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    typed_unfold_step_state: Option<&(MonoType, MonoType)>,
    typed_general_option: Option<&MonoType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match pattern {
        CorePattern::Wildcard | CorePattern::Var(_) => vec![Instr::I32Const(1)],
        CorePattern::LitInt(n) => {
            let mut instrs = value_anyref_instrs.to_vec();
            instrs.extend(emit_unbox_on_stack(&ValType::I64));
            instrs.push(Instr::I64Const(*n));
            instrs.push(Instr::I64Eq);
            instrs
        }
        CorePattern::LitBool(b) => {
            let mut instrs = value_anyref_instrs.to_vec();
            instrs.extend(emit_unbox_on_stack(&ValType::I32));
            instrs.push(Instr::I32Const(if *b { 1 } else { 0 }));
            instrs.push(Instr::I32Eq);
            instrs
        }
        CorePattern::LitStr(s) => {
            ensure_rt_str_eq_import(ctx);
            let mut instrs = value_anyref_instrs.to_vec();
            instrs.extend(emit_unbox_on_stack(&ref_string_null()));
            instrs.extend(emit_pooled_string_literal_atom(s, ctx));
            instrs.push(Instr::Call("rt_str__eq".to_string()));
            instrs
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
            if let Some((option_sym, field_monos)) =
                typed_iter_option_pattern_info(*type_id, expected_mono, option_iter_item_state, ctx)
            {
                let typed_ref = ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(option_sym.clone()),
                };
                let mut outer_checks = Vec::new();

                let mut type_check = value_anyref_instrs.to_vec();
                type_check.push(Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(option_sym.clone()),
                });
                outer_checks.push(type_check);

                let mut variant_then = value_anyref_instrs.to_vec();
                variant_then.extend(emit_unbox_on_stack(&typed_ref));
                variant_then.push(Instr::StructGet(option_sym.clone(), 0));
                variant_then.push(Instr::I32Const(variant.0 as i32));
                variant_then.push(Instr::I32Eq);

                let mut variant_check = value_anyref_instrs.to_vec();
                variant_check.push(Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(option_sym.clone()),
                });
                variant_check.push(Instr::If {
                    result: Some(ValType::I32),
                    then_body: variant_then,
                    else_body: vec![Instr::I32Const(0)],
                });
                outer_checks.push(variant_check);

                let mut inner_checks = Vec::new();
                for (idx, field_pat) in fields.iter().enumerate() {
                    if pattern_is_trivially_true(field_pat) {
                        continue;
                    }
                    let field_anyref = emit_variant_field_anyref(
                        value_anyref_instrs,
                        expected_mono,
                        option_iter_item_state,
                        typed_unfold_step_state,
                        None,
                        field_monos.get(idx),
                        *variant,
                        idx as i32,
                        ctx,
                    );
                    inner_checks.push(emit_pattern_condition(
                        field_pat,
                        &field_anyref,
                        field_monos.get(idx),
                        option_iter_item_state,
                        None,
                        None,
                        ctx,
                    ));
                }

                if inner_checks.is_empty() {
                    return combine_i32_ands(outer_checks);
                }

                let outer_cond = combine_i32_ands(outer_checks);
                let inner_cond = combine_i32_ands(inner_checks);
                let mut instrs = outer_cond;
                instrs.push(Instr::If {
                    result: Some(ValType::I32),
                    then_body: inner_cond,
                    else_body: vec![Instr::I32Const(0)],
                });
                return instrs;
            }
            if let Some((unfold_sym, field_monos)) = typed_unfold_step_pattern_info(
                *type_id,
                expected_mono,
                typed_unfold_step_state,
                ctx,
            ) {
                let typed_ref = ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(unfold_sym.clone()),
                };

                let mut outer_checks = Vec::new();

                let mut type_check = value_anyref_instrs.to_vec();
                type_check.push(Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(unfold_sym.clone()),
                });
                outer_checks.push(type_check);

                let mut variant_then = value_anyref_instrs.to_vec();
                variant_then.extend(emit_unbox_on_stack(&typed_ref));
                variant_then.push(Instr::StructGet(unfold_sym.clone(), 0));
                variant_then.push(Instr::I32Const(variant.0 as i32));
                variant_then.push(Instr::I32Eq);

                let mut variant_check = value_anyref_instrs.to_vec();
                variant_check.push(Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(unfold_sym.clone()),
                });
                variant_check.push(Instr::If {
                    result: Some(ValType::I32),
                    then_body: variant_then,
                    else_body: vec![Instr::I32Const(0)],
                });
                outer_checks.push(variant_check);

                let mut inner_checks = Vec::new();
                for (idx, field_pat) in fields.iter().enumerate() {
                    if pattern_is_trivially_true(field_pat) {
                        continue;
                    }
                    let field_anyref = emit_variant_field_anyref(
                        value_anyref_instrs,
                        expected_mono,
                        option_iter_item_state,
                        typed_unfold_step_state,
                        None,
                        field_monos.get(idx),
                        *variant,
                        idx as i32,
                        ctx,
                    );
                    inner_checks.push(emit_pattern_condition(
                        field_pat,
                        &field_anyref,
                        field_monos.get(idx),
                        option_iter_item_state,
                        None,
                        None,
                        ctx,
                    ));
                }

                if inner_checks.is_empty() {
                    return combine_i32_ands(outer_checks);
                }

                let outer_cond = combine_i32_ands(outer_checks);
                let inner_cond = combine_i32_ands(inner_checks);
                let mut instrs = outer_cond;
                instrs.push(Instr::If {
                    result: Some(ValType::I32),
                    then_body: inner_cond,
                    else_body: vec![Instr::I32Const(0)],
                });
                return instrs;
            }

            // Typed general Option<T> / Result<T,E> path: direct struct field access.
            // Guarded by flow metadata to avoid assuming typed layout for erased/ABI values.
            if let Some(mono) = typed_general_option {
                if let Some(option_sym) =
                    typed_general_option_pattern_sym(*type_id, expected_mono, Some(mono))
                {
                    let typed_ref = ValType::Ref {
                        nullable: true,
                        heap: HeapType::Named(option_sym.clone()),
                    };

                    let mut outer_checks = Vec::new();

                    let mut type_check = value_anyref_instrs.to_vec();
                    type_check.push(Instr::RefTest {
                        nullable: true,
                        heap: HeapType::Named(option_sym.clone()),
                    });
                    outer_checks.push(type_check);

                    let mut variant_then = value_anyref_instrs.to_vec();
                    variant_then.extend(emit_unbox_on_stack(&typed_ref));
                    variant_then.push(Instr::StructGet(option_sym.clone(), 0));
                    variant_then.push(Instr::I32Const(variant.0 as i32));
                    variant_then.push(Instr::I32Eq);

                    let mut variant_check = value_anyref_instrs.to_vec();
                    variant_check.push(Instr::RefTest {
                        nullable: true,
                        heap: HeapType::Named(option_sym.clone()),
                    });
                    variant_check.push(Instr::If {
                        result: Some(ValType::I32),
                        then_body: variant_then,
                        else_body: vec![Instr::I32Const(0)],
                    });
                    outer_checks.push(variant_check);

                    let mut inner_checks = Vec::new();
                    for (idx, field_pat) in fields.iter().enumerate() {
                        if pattern_is_trivially_true(field_pat) {
                            continue;
                        }
                        let struct_field_idx =
                            typed_sum_struct_field_offset(mono, *variant, idx as u32);
                        let field_mono = variant_field_mono_for_typed_sum(mono, *variant, idx);
                        let payload_ty = mono_to_valtype_specialized(
                            field_mono.unwrap_or(&MonoType::Void),
                            ctx.type_env,
                            &ctx.concrete_func_sigs,
                        );
                        let mut field_instrs = value_anyref_instrs.to_vec();
                        field_instrs.extend(emit_unbox_on_stack(&typed_ref));
                        field_instrs.push(Instr::StructGet(option_sym.clone(), struct_field_idx));
                        field_instrs.extend(emit_coerce_stack(&payload_ty, &ValType::Anyref));
                        inner_checks.push(emit_pattern_condition(
                            field_pat,
                            &field_instrs,
                            field_mono,
                            None,
                            None,
                            None,
                            ctx,
                        ));
                    }

                    if inner_checks.is_empty() {
                        return combine_i32_ands(outer_checks);
                    }

                    let outer_cond = combine_i32_ands(outer_checks);
                    let inner_cond = combine_i32_ands(inner_checks);
                    let mut instrs = outer_cond;
                    instrs.push(Instr::If {
                        result: Some(ValType::I32),
                        then_body: inner_cond,
                        else_body: vec![Instr::I32Const(0)],
                    });
                    return instrs;
                }
            }

            // Outer checks: type_id and variant_idx (safe to evaluate eagerly)
            let mut outer_checks = Vec::new();

            let mut type_check = value_anyref_instrs.to_vec();
            type_check.extend(emit_unbox_on_stack(&ref_variant_null()));
            type_check.push(Instr::StructGet(T_VARIANT.to_string(), 0));
            type_check.push(Instr::I32Const(type_id.0 as i32));
            type_check.push(Instr::I32Eq);
            outer_checks.push(type_check);

            let mut variant_check = value_anyref_instrs.to_vec();
            variant_check.extend(emit_unbox_on_stack(&ref_variant_null()));
            variant_check.push(Instr::StructGet(T_VARIANT.to_string(), 1));
            variant_check.push(Instr::I32Const(variant.0 as i32));
            variant_check.push(Instr::I32Eq);
            outer_checks.push(variant_check);

            // Inner checks: field sub-patterns (may ref.cast and trap if outer didn't match)
            let mut inner_checks = Vec::new();
            for (idx, field_pat) in fields.iter().enumerate() {
                if pattern_is_trivially_true(field_pat) {
                    continue;
                }
                let field_anyref =
                    emit_variant_field_anyref_universal(value_anyref_instrs, idx as i32);
                inner_checks.push(emit_pattern_condition(
                    field_pat,
                    &field_anyref,
                    None,
                    None,
                    None,
                    None,
                    ctx,
                ));
            }

            if inner_checks.is_empty() {
                // No field sub-patterns need checking, flat AND is fine
                combine_i32_ands(outer_checks)
            } else {
                // Short-circuit: only evaluate field checks if outer checks pass
                let outer_cond = combine_i32_ands(outer_checks);
                let inner_cond = combine_i32_ands(inner_checks);
                let mut instrs = outer_cond;
                instrs.push(Instr::If {
                    result: Some(ValType::I32),
                    then_body: inner_cond,
                    else_body: vec![Instr::I32Const(0)],
                });
                instrs
            }
        }
    }
}

fn emit_pattern_bindings(
    pattern: &CorePattern,
    value_anyref_instrs: &[Instr],
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    typed_unfold_step_state: Option<&(MonoType, MonoType)>,
    typed_general_option: Option<&MonoType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match pattern {
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => Vec::new(),
        CorePattern::Var(local_id) => {
            let (idx, local_ty) = ctx
                .local(*local_id)
                .cloned()
                .unwrap_or_else(|| panic!("missing local mapping for pattern var L{}", local_id.0));
            let mut instrs = value_anyref_instrs.to_vec();
            instrs.extend(emit_coerce_stack(&ValType::Anyref, &local_ty));
            instrs.push(Instr::LocalSet(idx));
            instrs
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
            let mut instrs = Vec::new();
            let typed_iter_option_fields = typed_iter_option_pattern_info(
                *type_id,
                expected_mono,
                option_iter_item_state,
                ctx,
            )
            .map(|(_, field_monos)| field_monos);
            for (idx, field_pat) in fields.iter().enumerate() {
                let field_expected = typed_iter_option_fields
                    .as_ref()
                    .and_then(|field_monos| field_monos.get(idx))
                    .or_else(|| {
                        expected_mono
                            .and_then(|mono| pattern_variant_field_mono(mono, *variant, idx))
                    });
                let field_anyref = emit_variant_field_anyref(
                    value_anyref_instrs,
                    expected_mono,
                    option_iter_item_state,
                    typed_unfold_step_state,
                    typed_general_option,
                    field_expected,
                    *variant,
                    idx as i32,
                    ctx,
                );
                instrs.extend(emit_pattern_bindings(
                    field_pat,
                    &field_anyref,
                    field_expected,
                    option_iter_item_state,
                    None,
                    None,
                    ctx,
                ));
            }
            instrs
        }
    }
}

fn emit_variant_field_anyref_universal(
    value_anyref_instrs: &[Instr],
    field_idx: i32,
) -> Vec<Instr> {
    let mut instrs = value_anyref_instrs.to_vec();
    instrs.extend(emit_unbox_on_stack(&ref_variant_null()));
    instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    instrs.push(Instr::I32Const(field_idx));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs
}

fn emit_variant_field_anyref(
    value_anyref_instrs: &[Instr],
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    typed_unfold_step_state: Option<&(MonoType, MonoType)>,
    typed_general_option: Option<&MonoType>,
    field_mono: Option<&MonoType>,
    variant_id: crate::ir::VariantId,
    field_idx: i32,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    if let Some((option_sym, _)) =
        typed_iter_option_pattern_info(OPTION_TYPE_ID, expected_mono, option_iter_item_state, ctx)
    {
        let field_ty = field_mono
            .and_then(|mono| {
                option_iter_item_state
                    .filter(|_| matches!(mono, MonoType::Named { type_id, .. } if *type_id == ITER_ITEM_TYPE_ID))
                    .map(|info| ValType::Ref {
                        nullable: true,
                        heap: HeapType::Named(typed_iter_item_sym(info)),
                    })
                    .or_else(|| {
                        Some(mono_to_valtype_specialized(
                            mono,
                            ctx.type_env,
                            &ctx.concrete_func_sigs,
                        ))
                    })
            })
            .unwrap_or(ValType::Anyref);
        let mut instrs = value_anyref_instrs.to_vec();
        instrs.push(Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(option_sym.clone()),
        });
        instrs.push(Instr::StructGet(option_sym, (field_idx + 1) as u32));
        instrs.extend(emit_coerce_stack(&field_ty, &ValType::Anyref));
        return instrs;
    }
    if let Some((unfold_sym, _)) = typed_unfold_step_pattern_info(
        UNFOLD_STEP_TYPE_ID,
        expected_mono,
        typed_unfold_step_state,
        ctx,
    ) {
        let field_ty = field_mono
            .map(|mono| mono_to_valtype_specialized(mono, ctx.type_env, &ctx.concrete_func_sigs))
            .unwrap_or(ValType::Anyref);
        let mut instrs = value_anyref_instrs.to_vec();
        instrs.push(Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(unfold_sym.clone()),
        });
        instrs.push(Instr::StructGet(unfold_sym, (field_idx + 1) as u32));
        instrs.extend(emit_coerce_stack(&field_ty, &ValType::Anyref));
        return instrs;
    }

    // Typed general Option<T> / Result<T,E> path.
    if let Some(mono) = typed_general_option {
        if let Some(sum_sym) = typed_general_option_pattern_sym(
            mono_type_id(mono).unwrap_or(OPTION_TYPE_ID),
            expected_mono,
            Some(mono),
        ) {
            let struct_field_idx =
                typed_sum_struct_field_offset(mono, variant_id, field_idx as u32);
            let field_mono_resolved =
                variant_field_mono_for_typed_sum(mono, variant_id, field_idx as usize);
            let field_ty = field_mono_resolved
                .map(|m| mono_to_valtype_specialized(m, ctx.type_env, &ctx.concrete_func_sigs))
                .unwrap_or(ValType::Anyref);
            let mut instrs = value_anyref_instrs.to_vec();
            instrs.push(Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(sum_sym.clone()),
            });
            instrs.push(Instr::StructGet(sum_sym, struct_field_idx));
            instrs.extend(emit_coerce_stack(&field_ty, &ValType::Anyref));
            return instrs;
        }
    }

    let mut instrs = value_anyref_instrs.to_vec();
    instrs.extend(emit_unbox_on_stack(&ref_variant_null()));
    instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    instrs.push(Instr::I32Const(field_idx));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs
}

fn typed_iter_option_pattern_info(
    type_id: TypeId,
    expected_mono: Option<&MonoType>,
    option_iter_item_state: Option<&IteratorStateInfo>,
    ctx: &EmitCtx<'_>,
) -> Option<(String, Vec<MonoType>)> {
    if ctx.concrete_func_sigs.is_empty() || type_id != OPTION_TYPE_ID {
        return None;
    }
    let info = option_iter_item_state?;
    let fallback_payload = MonoType::Named {
        type_id: ITER_ITEM_TYPE_ID,
        args: vec![info.yield_ty.clone()],
    };

    if let Some(MonoType::Named {
        type_id: mono_type_id,
        args,
    }) = expected_mono
    {
        if *mono_type_id == OPTION_TYPE_ID && args.len() == 1 {
            if let MonoType::Named {
                type_id: payload_type_id,
                args: payload_args,
            } = &args[0]
            {
                if *payload_type_id == ITER_ITEM_TYPE_ID && payload_args.len() == 1 {
                    return Some((typed_iter_option_sym(info), vec![args[0].clone()]));
                }
            }
            // Keep typed iterator-option lowering even if local mono metadata for
            // the Option payload is absent or imprecise.
            return Some((typed_iter_option_sym(info), vec![fallback_payload]));
        }
    }

    Some((typed_iter_option_sym(info), vec![fallback_payload]))
}

fn typed_general_option_pattern_sym(
    type_id: TypeId,
    expected_mono: Option<&MonoType>,
    typed_general_option: Option<&MonoType>,
) -> Option<String> {
    if type_id != OPTION_TYPE_ID && type_id != RESULT_TYPE_ID {
        return None;
    }
    let mono = typed_general_option?;
    if !is_typed_general_sum_candidate(mono) {
        return None;
    }
    if let Some(expected) = expected_mono {
        if expected != mono {
            return None;
        }
    }
    Some(typed_general_option_sym(mono))
}

/// Map a variant's field index to a struct field index in a typed sum struct.
/// Option layout: (variant_id, payload)      → field 0 is at struct index 1
/// Result layout: (variant_id, ok, err)      → Ok field 0 is at struct index 1,
///                                              Err field 0 is at struct index 2
fn typed_sum_struct_field_offset(
    mono: &MonoType,
    variant_id: crate::ir::VariantId,
    field_idx: u32,
) -> u32 {
    match mono {
        MonoType::Named { type_id, .. } if *type_id == RESULT_TYPE_ID => {
            // Result struct: (variant_id, ok_payload, err_payload)
            // Ok = variant 0 → struct field 1; Err = variant 1 → struct field 2
            variant_id.0 as u32 + 1 + field_idx
        }
        _ => {
            // Option struct: (variant_id, payload)
            1 + field_idx
        }
    }
}

/// Get the MonoType for a field within a typed sum variant.
fn variant_field_mono_for_typed_sum<'a>(
    mono: &'a MonoType,
    variant_id: crate::ir::VariantId,
    field_idx: usize,
) -> Option<&'a MonoType> {
    match mono {
        MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID && args.len() == 2 => {
            // Ok = variant 0 → args[0], Err = variant 1 → args[1]
            if field_idx == 0 {
                args.get(variant_id.0 as usize)
            } else {
                None
            }
        }
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            // Some = variant 1, field 0 → args[0]
            if field_idx == 0 { args.get(0) } else { None }
        }
        _ => None,
    }
}

/// Extract the TypeId from a MonoType::Named.
fn mono_type_id(mono: &MonoType) -> Option<TypeId> {
    match mono {
        MonoType::Named { type_id, .. } => Some(*type_id),
        _ => None,
    }
}

fn typed_unfold_step_pattern_info(
    type_id: TypeId,
    expected_mono: Option<&MonoType>,
    typed_unfold_step_state: Option<&(MonoType, MonoType)>,
    ctx: &EmitCtx<'_>,
) -> Option<(String, Vec<MonoType>)> {
    if ctx.concrete_func_sigs.is_empty() || type_id != UNFOLD_STEP_TYPE_ID {
        return None;
    }
    let MonoType::Named {
        type_id: mono_type_id,
        args,
    } = expected_mono?
    else {
        return None;
    };
    if *mono_type_id != UNFOLD_STEP_TYPE_ID || args.len() != 2 {
        return None;
    }
    let (typed_yield, typed_seed) = typed_unfold_step_state?;
    if args[0] != *typed_yield || args[1] != *typed_seed {
        return None;
    }
    Some((
        typed_unfold_step_sym(&args[0], &args[1]),
        vec![args[0].clone(), args[1].clone()],
    ))
}

fn variant_field_mono(expected_mono: &MonoType, field_idx: usize) -> Option<&MonoType> {
    let MonoType::Named { type_id, args } = expected_mono else {
        return None;
    };
    match *type_id {
        OPTION_TYPE_ID => match field_idx {
            0 => args.first(),
            _ => None,
        },
        UNFOLD_STEP_TYPE_ID => match field_idx {
            0 => args.first(),
            1 => args.get(1),
            _ => None,
        },
        _ => None,
    }
}

fn pattern_variant_field_mono(
    expected_mono: &MonoType,
    variant: crate::ir::VariantId,
    field_idx: usize,
) -> Option<&MonoType> {
    variant_field_mono_for_typed_sum(expected_mono, variant, field_idx)
        .or_else(|| variant_field_mono(expected_mono, field_idx))
}

fn pattern_is_trivially_true(pattern: &CorePattern) -> bool {
    matches!(pattern, CorePattern::Wildcard | CorePattern::Var(_))
}

fn combine_i32_ands(mut checks: Vec<Vec<Instr>>) -> Vec<Instr> {
    if checks.is_empty() {
        return vec![Instr::I32Const(1)];
    }
    let mut instrs = checks.remove(0);
    for check in checks {
        instrs.extend(check);
        instrs.push(Instr::I32And);
    }
    instrs
}

fn emit_non_exhaustive_match_fallback(ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    ensure_rt_core_trap_import(ctx);
    let trap_msg = match (ctx.current_func_name.as_deref(), ctx.current_func_id) {
        (Some(name), Some(func_id)) => {
            format!("non-exhaustive match in {name} (FuncId({}))", func_id.0)
        }
        (None, Some(func_id)) => format!("non-exhaustive match in FuncId({})", func_id.0),
        _ => "non-exhaustive match".to_string(),
    };
    let mut instrs = emit_pooled_string_literal_atom(&trap_msg, ctx);
    instrs.push(Instr::Call("rt_core__trap".to_string()));
    instrs.push(Instr::Unreachable);
    instrs
}

fn emit_loop_op(
    body: &AnfExpr,
    bind_ty: &ValType,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let (break_label, cont_label) = ctx.fresh_loop_labels();
    ctx.label_stack
        .push((break_label.clone(), cont_label.clone()));
    ctx.loop_result_stack.push(Some(bind_ty.clone()));

    let mut loop_body = emit_loop_body_expr(body, fn_return_ty, ctx);
    // Core/ANF loop semantics: falling through means continue next iteration.
    loop_body.push(Instr::Br(cont_label.clone()));

    ctx.loop_result_stack.pop();
    ctx.label_stack.pop();

    vec![Instr::Block {
        label: break_label,
        result: Some(bind_ty.clone()),
        body: vec![
            Instr::Loop {
                label: cont_label,
                result: None,
                body: loop_body,
            },
            // The loop always branches (continue or break), so this is unreachable.
            // Needed to satisfy the block's result type for the Wasm validator.
            Instr::Unreachable,
        ],
    }]
}

fn emit_loop_body_expr(
    expr: &AnfExpr,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            emit_let_expr(*local, op, body, fn_return_ty, ctx, |ctx, body| {
                emit_loop_body_expr(body, fn_return_ty, ctx)
            })
        }
        AnfExpr::Return(None) => vec![Instr::Return],
        AnfExpr::Return(Some(atom)) => {
            let mut instrs = emit_atom(atom, fn_return_ty, ctx);
            instrs.push(Instr::Return);
            instrs
        }
        AnfExpr::Break(value) => emit_break(value.as_ref(), ctx),
        AnfExpr::Continue => emit_continue(ctx),
        AnfExpr::Atom(atom) => {
            let mut instrs = emit_atom(atom, None, ctx);
            if atom_produces_value(atom) {
                instrs.push(Instr::Drop);
            }
            instrs
        }
    }
}

fn emit_expr_value(
    expr: &AnfExpr,
    expected_ty: &ValType,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    emit_expr_value_with_expected(expr, Some(expected_ty), fn_return_ty, ctx)
}

fn emit_expr_value_with_expected(
    expr: &AnfExpr,
    expected_ty: Option<&ValType>,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            emit_let_expr(*local, op, body, fn_return_ty, ctx, |ctx, body| {
                emit_expr_value_with_expected(body, expected_ty, fn_return_ty, ctx)
            })
        }
        AnfExpr::Atom(atom) => emit_atom(atom, expected_ty, ctx),
        AnfExpr::Return(None) => vec![Instr::Return],
        AnfExpr::Return(Some(atom)) => {
            let mut instrs = emit_atom(atom, fn_return_ty, ctx);
            instrs.push(Instr::Return);
            instrs
        }
        AnfExpr::Break(value) => emit_break(value.as_ref(), ctx),
        AnfExpr::Continue => emit_continue(ctx),
    }
}

fn emit_break(value: Option<&Atom>, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let (break_label, _) = ctx
        .label_stack
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("break emitted outside of loop context"));
    let break_result_ty = ctx
        .loop_result_stack
        .last()
        .and_then(|ty| ty.as_ref())
        .cloned();
    let mut instrs = Vec::new();
    match (value, break_result_ty.as_ref()) {
        (Some(atom), Some(expected)) => instrs.extend(emit_atom(atom, Some(expected), ctx)),
        (Some(atom), None) => instrs.extend(emit_atom(atom, None, ctx)),
        // A bare `break` in a value-typed loop still needs to satisfy the
        // block result type in Wasm. Emit a typed default placeholder.
        (None, Some(expected)) => instrs.extend(emit_default_value_instrs(expected)),
        (None, None) => {}
    }
    instrs.push(Instr::Br(break_label));
    instrs
}

fn emit_continue(ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let (_, cont_label) = ctx
        .label_stack
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("continue emitted outside of loop context"));
    vec![Instr::Br(cont_label)]
}

fn emit_tail_let_call(
    local: crate::ir::LocalId,
    op: &AnfOp,
    body: &AnfExpr,
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    let AnfOp::ACall { callee, args } = op else {
        return None;
    };
    if !expr_returns_local(body, local) {
        return None;
    }
    emit_tail_call(callee, args, return_ty, ctx)
}

fn expr_returns_local(expr: &AnfExpr, local: crate::ir::LocalId) -> bool {
    match expr {
        AnfExpr::Return(Some(Atom::ALocal(id))) | AnfExpr::Atom(Atom::ALocal(id)) => *id == local,
        _ => false,
    }
}

fn emit_tail_call(
    callee: &Atom,
    args: &[Atom],
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    match callee {
        Atom::AGlobalFunc(func_id) => {
            if let Some(entry) = ctx.prelude.get(func_id).cloned() {
                emit_tail_runtime_prelude_call(*func_id, &entry, args, return_ty, ctx)
            } else {
                emit_tail_direct_user_call(*func_id, args, return_ty, ctx)
            }
        }
        Atom::ALocal(_) => emit_tail_closure_call(callee, args, return_ty, ctx),
        _ => None,
    }
}

fn emit_tail_runtime_prelude_call(
    _func_id: FuncId,
    entry: &crate::codegen::prelude::PreludeEntry,
    args: &[Atom],
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    if !entry.is_runtime_call()
        || !tail_runtime_result_compatible(&entry.runtime_results, return_ty)
    {
        return None;
    }
    if args.len() != entry.runtime_params.len() {
        panic!(
            "arity mismatch for prelude call '{}': expected {}, got {}",
            entry.twinkle_name,
            entry.runtime_params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (arg, param_ty) in args.iter().zip(entry.runtime_params.iter()) {
        instrs.extend(emit_atom(arg, Some(param_ty), ctx));
    }
    ctx.add_runtime_import(entry);
    let sym = entry.runtime_sym.as_ref().cloned().unwrap_or_else(|| {
        panic!(
            "runtime prelude entry missing symbol: {}",
            entry.twinkle_name
        )
    });
    instrs.push(Instr::ReturnCall(sym));
    Some(instrs)
}

fn tail_runtime_result_compatible(
    runtime_results: &[ValType],
    return_ty: Option<&ValType>,
) -> bool {
    match (runtime_results, return_ty) {
        ([], None) => true,
        ([single], Some(ret)) => single == ret,
        _ => false,
    }
}

fn typed_closure_valtype(func_id: FuncId, ctx: &EmitCtx<'_>) -> Option<ValType> {
    let (params, ret) = ctx.concrete_func_sigs.get(&func_id)?;
    Some(ValType::Ref {
        nullable: true,
        heap: HeapType::Named(typed_closure_struct_sym(params, ret)),
    })
}

fn emit_specialized_closure_arg(
    arg: &Atom,
    expected_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    match arg {
        Atom::ALocal(local_id) => {
            let (func_id, free_vars) = ctx.repr_flow.closure_locals.get(local_id)?.clone();
            let typed_ty = typed_closure_valtype(func_id, ctx)?;
            if &typed_ty != expected_ty {
                return None;
            }
            Some(emit_make_closure(func_id, &free_vars, expected_ty, ctx))
        }
        Atom::AGlobalFunc(func_id) => {
            let typed_ty = typed_closure_valtype(*func_id, ctx)?;
            if &typed_ty != expected_ty {
                return None;
            }
            Some(emit_make_closure(*func_id, &[], expected_ty, ctx))
        }
        _ => None,
    }
}

fn emit_tail_direct_user_call(
    func_id: FuncId,
    args: &[Atom],
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    let abi = ctx
        .user_func_abi(func_id)
        .unwrap_or_else(|| panic!("missing ABI for function FuncId({})", func_id.0));
    if !tail_user_result_compatible(abi.results.first(), return_ty) {
        return None;
    }
    if abi.params.len() != args.len() {
        panic!(
            "arity mismatch for direct call to FuncId({}): expected {}, got {}",
            func_id.0,
            abi.params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (arg, param_ty) in args.iter().zip(abi.params.iter()) {
        if let Some(specialized) = emit_specialized_closure_arg(arg, param_ty, ctx) {
            instrs.extend(specialized);
        } else {
            instrs.extend(emit_atom(arg, Some(param_ty), ctx));
        }
    }
    instrs.push(Instr::ReturnCall(user_func_sym(func_id)));
    Some(instrs)
}

fn tail_user_result_compatible(result_ty: Option<&ValType>, return_ty: Option<&ValType>) -> bool {
    match (result_ty, return_ty) {
        (None, None) => true,
        (Some(result), Some(ret)) => result == ret,
        _ => false,
    }
}

fn emit_tail_closure_call(
    callee: &Atom,
    args: &[Atom],
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    if return_ty != Some(&ValType::Anyref) {
        return None;
    }
    let mut instrs = emit_atom(callee, Some(&ref_closure_null()), ctx);
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 1));

    if args.is_empty() {
        instrs.push(Instr::RefNull(HeapType::None));
    } else {
        for arg in args {
            instrs.extend(emit_atom(arg, Some(&ValType::Anyref), ctx));
        }
        instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), args.len() as u32));
    }

    instrs.extend(emit_atom(callee, Some(&ref_closure_null()), ctx));
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 0));
    instrs.push(Instr::ReturnCallRef(T_CLOSURE_FUNC.to_string()));
    Some(instrs)
}

fn emit_atom(atom: &Atom, expected_ty: Option<&ValType>, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    match atom {
        Atom::ALocal(local_id) => emit_local_atom(*local_id, expected_ty, ctx),
        Atom::AGlobalFunc(func_id) => {
            if let Some(expected_ty) = expected_ty {
                if let Some(instrs) = emit_specialized_closure_arg(atom, expected_ty, ctx) {
                    return instrs;
                }
            }
            emit_global_func_atom(*func_id, expected_ty)
        }
        Atom::ALitInt(n) => emit_int_literal(*n, expected_ty),
        Atom::ALitFloat(v) => emit_float_literal(*v, expected_ty),
        Atom::ALitBool(b) => emit_bool_literal(*b, expected_ty),
        Atom::ALitStr(s) => emit_string_literal(s, expected_ty, ctx),
        Atom::ALitVoid => emit_void_value(expected_ty),
    }
}

/// Emit instructions to load a local and coerce it to the expected Wasm type.
///
/// Sum-boundary conversions happen here: if a local holds a typed Option/Result
/// struct (tracked via `SumRepr`) but the consumer expects an erased `$Variant`
/// or `anyref`, this function emits the typed→erased conversion. Conversely,
/// if the local is `anyref` but context proves it's a typed sum, direct struct
/// access may be used.
fn emit_local_atom(
    local_id: crate::ir::LocalId,
    expected_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Locals with semantic type Never are unreachable by construction.
    // Emitting a local.get/coercion for them can trigger impossible type casts
    // in diverging branches (e.g. `let x = exit(...); x`).
    if matches!(
        ctx.infer_atom_mono(&Atom::ALocal(local_id)),
        Some(MonoType::Never)
    ) {
        return vec![Instr::Unreachable];
    }

    if let Some((idx, local_ty)) = ctx.local(local_id).cloned() {
        // Sum-boundary conversion: when a consumer expects an erased Variant or
        // Anyref, check if this local holds a typed sum value that needs conversion.
        // Uses SumRepr metadata first (authoritative), then falls back to mono
        // inference for locals whose SumRepr wasn't set (e.g. anyref locals with
        // inferred typed option content).
        if let Some(expected) = expected_ty {
            if is_variant_ref_type(expected) || *expected == ValType::Anyref {
                let target = if *expected == ValType::Anyref {
                    &ref_variant_null()
                } else {
                    expected
                };
                if let Some(instrs) =
                    emit_sum_local_to_erased(local_id, idx, &local_ty, target, ctx)
                {
                    return instrs;
                }
            }
        }
        return match expected_ty {
            None => vec![Instr::LocalGet(idx)],
            Some(expected) if expected == &local_ty => vec![Instr::LocalGet(idx)],
            Some(expected) => emit_coerce_local(idx, &local_ty, expected, ctx),
        };
    }

    if let Some(global_sym) = ctx.module_global_sym(local_id).cloned() {
        let mut instrs = vec![Instr::GlobalGet(global_sym)];
        if let Some(expected) = expected_ty {
            instrs.extend(emit_coerce_stack(&ValType::Anyref, expected));
        }
        return instrs;
    }

    panic!("missing local mapping for L{}", local_id.0);
}

/// Centralized sum→erased conversion dispatcher for locals.
///
/// Checks `SumRepr` metadata first. If the local has a known typed sum repr,
/// emits direct typed→erased conversion. If SumRepr is not set but mono
/// inference suggests a typed option candidate (ambiguous anyref local),
/// emits a runtime-dispatched conversion that handles both erased and typed
/// representations.
///
/// Returns `None` if no sum conversion is needed.
fn emit_sum_local_to_erased(
    local_id: crate::ir::LocalId,
    local_idx: u32,
    local_ty: &ValType,
    target: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    // Path 1: SumRepr metadata confirms local is typed → direct conversion.
    if let Some(sum_repr) = ctx.local_sum_repr(local_id).cloned() {
        let candidate = match &sum_repr {
            SumRepr::TypedOption(mono) => is_typed_general_option_candidate(mono).then_some(mono),
            SumRepr::TypedResult(mono) => is_typed_general_result_candidate(mono).then_some(mono),
            SumRepr::ErasedVariant => None,
        };
        if let Some(mono) = candidate {
            if local_can_store_typed_option(local_id, mono, ctx) {
                // Debug: verify SumRepr and mono inference agree.
                debug_assert!(
                    ctx.infer_atom_mono(&Atom::ALocal(local_id))
                        .as_ref()
                        .is_none_or(|inferred| inferred == mono),
                    "SumRepr/mono inference mismatch for L{}: repr={:?} inferred={:?}",
                    local_id.0,
                    mono,
                    ctx.infer_atom_mono(&Atom::ALocal(local_id)),
                );
                // Anyref locals can end up carrying mixed typed/erased sum values
                // across branch joins. Use runtime dispatch to avoid trapping on
                // a direct typed ref.cast when the value is already erased.
                if *local_ty == ValType::Anyref {
                    return Some(emit_anyref_option_or_variant_local_to_variant(
                        local_idx, mono, target, ctx,
                    ));
                }
                return Some(emit_typed_general_option_local_to_variant(
                    local_idx, mono, target, ctx,
                ));
            }
        }
    }

    // Path 2: No SumRepr, but inferred mono suggests typed sum in anyref local.
    // Use runtime dispatch (ref.test) to handle both representations safely.
    if *local_ty == ValType::Anyref {
        if let Some(sum_mono) = ctx
            .infer_atom_mono(&Atom::ALocal(local_id))
            .filter(is_typed_general_sum_candidate)
        {
            return Some(emit_anyref_option_or_variant_local_to_variant(
                local_idx, &sum_mono, target, ctx,
            ));
        }
    }

    None
}

fn emit_anyref_option_or_variant_local_to_variant(
    local_idx: u32,
    mono: &MonoType,
    expected_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    ctx.request_typed_general_option(typed_general_option_sym(mono), mono.clone());
    let mut instrs = vec![
        Instr::LocalGet(local_idx),
        Instr::RefTest {
            nullable: true,
            heap: HeapType::Named(T_VARIANT.to_string()),
        },
    ];

    let then_body = vec![
        Instr::LocalGet(local_idx),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_VARIANT.to_string()),
        },
    ];
    let else_body =
        emit_typed_general_option_local_to_variant(local_idx, mono, &ref_variant_null(), ctx);
    instrs.push(Instr::If {
        result: Some(ref_variant_null()),
        then_body,
        else_body,
    });
    instrs.extend(emit_coerce_stack(&ref_variant_null(), expected_ty));
    instrs
}

fn is_variant_ref_type(ty: &ValType) -> bool {
    matches!(
        ty,
        ValType::Ref {
            heap: HeapType::Named(name),
            ..
        } if name == T_VARIANT
    )
}

fn emit_typed_general_option_local_to_variant(
    local_idx: u32,
    mono: &MonoType,
    expected_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            emit_typed_option_local_to_variant(local_idx, mono, &args[0], expected_ty, ctx)
        }
        MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID && args.len() == 2 => {
            emit_typed_result_local_to_variant(
                local_idx,
                mono,
                &args[0],
                &args[1],
                expected_ty,
                ctx,
            )
        }
        _ => panic!(
            "emit_typed_general_option_local_to_variant expects Option<T> or Result<T,E>, got {:?}",
            mono
        ),
    }
}

fn emit_typed_option_local_to_variant(
    local_idx: u32,
    mono: &MonoType,
    inner: &MonoType,
    expected_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    #[cfg(debug_assertions)]
    bump_boundary!(typed_option_to_erased);
    let option_sym = typed_general_option_sym(mono);
    let typed_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(option_sym.clone()),
    };
    let payload_ty = mono_to_valtype_specialized(inner, ctx.type_env, &ctx.concrete_func_sigs);

    let mut instrs = vec![Instr::I32Const(OPTION_TYPE_ID.0 as i32)];

    // variant_id field
    instrs.push(Instr::LocalGet(local_idx));
    instrs.extend(emit_unbox_on_stack(&typed_ref));
    instrs.push(Instr::StructGet(option_sym.clone(), 0));

    // payload array: Some -> [payload], None -> []
    instrs.push(Instr::LocalGet(local_idx));
    instrs.extend(emit_unbox_on_stack(&typed_ref));
    instrs.push(Instr::StructGet(option_sym.clone(), 0));
    instrs.push(Instr::I32Const(1));
    instrs.push(Instr::I32Eq);
    let mut then_body = vec![Instr::LocalGet(local_idx)];
    then_body.extend(emit_unbox_on_stack(&typed_ref));
    then_body.push(Instr::StructGet(option_sym, 1));
    then_body.extend(emit_coerce_stack(&payload_ty, &ValType::Anyref));
    then_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    let else_body = vec![Instr::ArrayNewFixed(T_ARRAY.to_string(), 0)];
    instrs.push(Instr::If {
        result: Some(ref_array()),
        then_body,
        else_body,
    });

    instrs.push(Instr::StructNew(T_VARIANT.to_string()));
    instrs.extend(emit_coerce_stack(&ref_variant(), expected_ty));
    instrs
}

/// Convert a typed Result<T,E> local to an erased $Variant.
/// Result always has a single-element payload array: Ok extracts field 1,
/// Err extracts field 2.
fn emit_typed_result_local_to_variant(
    local_idx: u32,
    mono: &MonoType,
    ok_inner: &MonoType,
    err_inner: &MonoType,
    expected_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    #[cfg(debug_assertions)]
    bump_boundary!(typed_result_to_erased);
    let result_sym = typed_general_option_sym(mono);
    let typed_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(result_sym.clone()),
    };
    let ok_ty = mono_to_valtype_specialized(ok_inner, ctx.type_env, &ctx.concrete_func_sigs);
    let err_ty = mono_to_valtype_specialized(err_inner, ctx.type_env, &ctx.concrete_func_sigs);

    let mut instrs = vec![Instr::I32Const(RESULT_TYPE_ID.0 as i32)];

    // variant_id field
    instrs.push(Instr::LocalGet(local_idx));
    instrs.extend(emit_unbox_on_stack(&typed_ref));
    instrs.push(Instr::StructGet(result_sym.clone(), 0));

    // payload array: always 1 element — pick ok_payload (field 1) or err_payload (field 2)
    instrs.push(Instr::LocalGet(local_idx));
    instrs.extend(emit_unbox_on_stack(&typed_ref));
    instrs.push(Instr::StructGet(result_sym.clone(), 0));
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::I32Eq);
    // Ok branch: extract field 1 (ok_payload)
    let mut ok_body = vec![Instr::LocalGet(local_idx)];
    ok_body.extend(emit_unbox_on_stack(&typed_ref));
    ok_body.push(Instr::StructGet(result_sym.clone(), 1));
    ok_body.extend(emit_coerce_stack(&ok_ty, &ValType::Anyref));
    ok_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    // Err branch: extract field 2 (err_payload)
    let mut err_body = vec![Instr::LocalGet(local_idx)];
    err_body.extend(emit_unbox_on_stack(&typed_ref));
    err_body.push(Instr::StructGet(result_sym, 2));
    err_body.extend(emit_coerce_stack(&err_ty, &ValType::Anyref));
    err_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    instrs.push(Instr::If {
        result: Some(ref_array()),
        then_body: ok_body,
        else_body: err_body,
    });

    instrs.push(Instr::StructNew(T_VARIANT.to_string()));
    instrs.extend(emit_coerce_stack(&ref_variant(), expected_ty));
    instrs
}

fn emit_global_func_atom(func_id: FuncId, expected_ty: Option<&ValType>) -> Vec<Instr> {
    if let Some(expected) = expected_ty {
        match expected {
            ValType::Anyref | ValType::Ref { .. } => {}
            _ => panic!(
                "global function atom cannot be coerced to non-reference type: {:?}",
                expected
            ),
        }
    }

    vec![
        Instr::RefFunc(global_func_trampoline_sym(func_id)),
        Instr::RefNull(HeapType::Named(T_CLOSURE_ENV.to_string())),
        Instr::StructNew(T_CLOSURE.to_string()),
    ]
}

fn emit_string_literal_atom(s: &str) -> Vec<Instr> {
    let bytes = s.as_bytes();
    let mut instrs = Vec::with_capacity(bytes.len() + 1);
    for b in bytes {
        instrs.push(Instr::I32Const(*b as i32));
    }
    instrs.push(Instr::ArrayNewFixed(
        T_STRING.to_string(),
        bytes.len() as u32,
    ));
    instrs
}

fn emit_pooled_string_literal_atom(s: &str, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let getter_sym = ctx.request_string_literal(s);
    vec![Instr::Call(getter_sym)]
}

fn emit_string_literal_pool_globals(
    literals: &BTreeMap<Vec<u8>, StringLiteralPoolEntry>,
) -> Vec<GlobalDef> {
    literals
        .values()
        .map(|entry| GlobalDef {
            name: entry.global_sym.clone(),
            mutable: true,
            ty: ref_string_null(),
            init: vec![Instr::RefNull(HeapType::Named(T_STRING.to_string()))],
        })
        .collect()
}

fn emit_string_literal_pool_getters(
    literals: &BTreeMap<Vec<u8>, StringLiteralPoolEntry>,
) -> Vec<FuncDef> {
    literals
        .iter()
        .map(|(bytes, entry)| emit_string_literal_pool_getter(bytes, entry))
        .collect()
}

fn emit_string_literal_pool_getter(bytes: &[u8], entry: &StringLiteralPoolEntry) -> FuncDef {
    let literal = std::str::from_utf8(bytes)
        .expect("string literal pool must contain valid UTF-8 bytes from Twinkle source");
    let mut init_then = emit_string_literal_atom(literal);
    init_then.push(Instr::GlobalSet(entry.global_sym.clone()));

    FuncDef {
        name: entry.getter_sym.clone(),
        params: Vec::new(),
        results: vec![ref_string()],
        locals: Vec::new(),
        body: vec![
            Instr::GlobalGet(entry.global_sym.clone()),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: init_then,
                else_body: Vec::new(),
            },
            Instr::GlobalGet(entry.global_sym.clone()),
            Instr::RefAsNonNull,
        ],
    }
}

fn emit_int_literal(n: i64, expected_ty: Option<&ValType>) -> Vec<Instr> {
    match expected_ty {
        None | Some(ValType::I64) => vec![Instr::I64Const(n)],
        Some(ValType::I32) => vec![Instr::I32Const(n as i32)],
        Some(ValType::Anyref) => vec![
            Instr::I64Const(n),
            Instr::StructNew(T_BOXED_INT.to_string()),
        ],
        Some(other) => panic!("cannot emit Int literal as {:?}", other),
    }
}

fn emit_float_literal(v: f64, expected_ty: Option<&ValType>) -> Vec<Instr> {
    match expected_ty {
        None | Some(ValType::F64) => vec![Instr::F64Const(v)],
        Some(ValType::Anyref) => {
            vec![
                Instr::F64Const(v),
                Instr::StructNew(T_BOXED_FLOAT.to_string()),
            ]
        }
        Some(other) => panic!("cannot emit Float literal as {:?}", other),
    }
}

fn emit_bool_literal(b: bool, expected_ty: Option<&ValType>) -> Vec<Instr> {
    let value = if b { 1 } else { 0 };
    match expected_ty {
        None | Some(ValType::I32) => vec![Instr::I32Const(value)],
        Some(ValType::Anyref) => vec![Instr::I32Const(value), Instr::RefI31],
        Some(other) => panic!("cannot emit Bool literal as {:?}", other),
    }
}

fn emit_string_literal(
    s: &str,
    expected_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match expected_ty {
        None => emit_pooled_string_literal_atom(s, ctx),
        Some(ValType::Anyref) | Some(ValType::Ref { .. }) => {
            let mut instrs = emit_pooled_string_literal_atom(s, ctx);
            if let Some(expected) = expected_ty {
                instrs.extend(emit_coerce_stack(&ref_string(), expected));
            }
            instrs
        }
        Some(other) => panic!("cannot emit String literal as {:?}", other),
    }
}

fn emit_void_value(expected_ty: Option<&ValType>) -> Vec<Instr> {
    match expected_ty {
        None => Vec::new(),
        Some(ValType::I32) => vec![Instr::I32Const(0)],
        Some(ValType::Anyref) => vec![Instr::I32Const(0), Instr::RefI31],
        Some(other) => panic!("cannot emit Void value as {:?}", other),
    }
}

fn emit_coerce_local(
    idx: u32,
    local_ty: &ValType,
    expected: &ValType,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    if is_universal_iter_state_ref(expected) {
        if let Some(info) = typed_iterator_state_info_for_valtype(local_ty, ctx) {
            return emit_box_typed_iterator_state_local(idx, &info, ctx);
        }
    }
    if let Some(info) = typed_iterator_state_info_for_valtype(expected, ctx) {
        if is_universal_iter_state_ref(local_ty) {
            return emit_unbox_erased_iterator_state_local(idx, &info, ctx);
        }
    }
    if is_variant_ref(expected) {
        if let Some((yield_ty, seed_ty)) = typed_unfold_step_info_for_valtype(local_ty, ctx) {
            return emit_box_typed_unfold_step_local(idx, &yield_ty, &seed_ty, ctx);
        }
        if let Some(info) = typed_iter_option_info_for_valtype(local_ty, ctx) {
            return emit_box_typed_iter_option_local(idx, &info, ctx);
        }
    }
    if let Some((yield_ty, seed_ty)) = typed_unfold_step_info_for_valtype(expected, ctx) {
        if is_variant_ref(local_ty) {
            return emit_unbox_erased_unfold_step_local(idx, &yield_ty, &seed_ty, ctx);
        }
    }
    if let Some(info) = typed_iter_option_info_for_valtype(expected, ctx) {
        if is_variant_ref(local_ty) {
            return emit_unbox_erased_iter_option_local(idx, &info, ctx);
        }
    }

    match (local_ty, expected) {
        (_, ValType::Anyref) => {
            let mut instrs = vec![Instr::LocalGet(idx)];
            instrs.extend(emit_box_on_stack(local_ty));
            instrs
        }
        (ValType::Anyref, _) => {
            let mut instrs = vec![Instr::LocalGet(idx)];
            instrs.extend(emit_unbox_on_stack(expected));
            instrs
        }
        // Numeric widening/narrowing
        (ValType::I32, ValType::I64) => vec![Instr::LocalGet(idx), Instr::I64ExtendI32S],
        (ValType::I64, ValType::I32) => vec![Instr::LocalGet(idx), Instr::I32WrapI64],
        (
            ValType::Ref {
                nullable: false,
                heap: from_heap,
            },
            ValType::Ref {
                nullable: true,
                heap: to_heap,
            },
        ) if from_heap == to_heap => vec![Instr::LocalGet(idx)],
        (
            ValType::Ref {
                nullable: true,
                heap: from_heap,
            },
            ValType::Ref {
                nullable: false,
                heap: to_heap,
            },
        ) if from_heap == to_heap => vec![Instr::LocalGet(idx), Instr::RefAsNonNull],
        (
            ValType::Ref {
                nullable: _,
                heap: _,
            },
            ValType::Ref { nullable, heap },
        ) => vec![
            Instr::LocalGet(idx),
            Instr::RefCast {
                nullable: *nullable,
                heap: heap.clone(),
            },
        ],
        // I32 → Ref coercion: only reachable for Never-typed locals (Bool/Byte/Void
        // never coerce to Ref in valid code). Emit unreachable to satisfy the Wasm
        // type validator in divergent branches.
        (ValType::I32, ValType::Ref { .. }) => {
            vec![Instr::Unreachable]
        }
        _ => panic!(
            "unsupported local coercion from {:?} to {:?} in Stage 8c Step 2 (FuncId {:?}, local idx {})",
            local_ty, expected, ctx.current_func_id, idx
        ),
    }
}

fn emit_box_on_stack(local_ty: &ValType) -> Vec<Instr> {
    match local_ty {
        ValType::I64 => vec![Instr::StructNew(T_BOXED_INT.to_string())],
        ValType::F64 => vec![Instr::StructNew(T_BOXED_FLOAT.to_string())],
        ValType::I32 => vec![Instr::RefI31],
        ValType::Anyref | ValType::Ref { .. } | ValType::I31ref | ValType::Funcref => Vec::new(),
        _ => panic!(
            "unsupported boxing coercion from {:?} to anyref in Stage 8c",
            local_ty
        ),
    }
}

fn is_universal_iter_state_ref(ty: &ValType) -> bool {
    matches!(
        ty,
        ValType::Ref {
            heap: HeapType::Named(sym),
            ..
        } if sym == T_ITER_STATE
    )
}

fn is_variant_ref(ty: &ValType) -> bool {
    matches!(
        ty,
        ValType::Ref {
            heap: HeapType::Named(sym),
            ..
        } if sym == T_VARIANT
    )
}

fn typed_iterator_state_info_for_valtype(
    ty: &ValType,
    ctx: &EmitCtx<'_>,
) -> Option<IteratorStateInfo> {
    let ValType::Ref {
        heap: HeapType::Named(sym),
        ..
    } = ty
    else {
        return None;
    };
    ctx.requested_typed_iterator_states().get(sym).cloned()
}

fn typed_iter_option_info_for_valtype(
    ty: &ValType,
    ctx: &EmitCtx<'_>,
) -> Option<IteratorStateInfo> {
    let ValType::Ref {
        heap: HeapType::Named(sym),
        ..
    } = ty
    else {
        return None;
    };
    ctx.requested_typed_iter_options().get(sym).cloned()
}

fn typed_unfold_step_info_for_valtype(
    ty: &ValType,
    ctx: &EmitCtx<'_>,
) -> Option<(MonoType, MonoType)> {
    let ValType::Ref {
        heap: HeapType::Named(sym),
        ..
    } = ty
    else {
        return None;
    };
    ctx.requested_typed_unfold_steps().get(sym).cloned()
}

fn emit_box_typed_iterator_state_local(
    idx: u32,
    info: &IteratorStateInfo,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let state_sym = typed_iterator_state_sym(info);
    let seed_ty = mono_to_valtype_specialized(&info.seed_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let step_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let step_ty = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(step_sym),
    };

    let mut instrs = vec![Instr::LocalGet(idx), Instr::StructGet(state_sym.clone(), 0)];
    instrs.extend(emit_coerce_stack(&seed_ty, &ValType::Anyref));
    instrs.push(Instr::LocalGet(idx));
    instrs.push(Instr::StructGet(state_sym, 1));
    instrs.extend(emit_coerce_stack(&step_ty, &ValType::Anyref));
    instrs.push(Instr::StructNew(T_ITER_STATE.to_string()));
    instrs
}

fn emit_unbox_erased_iterator_state_local(
    idx: u32,
    info: &IteratorStateInfo,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let state_sym = typed_iterator_state_sym(info);
    let seed_ty = mono_to_valtype_specialized(&info.seed_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let step_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let step_ty = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(step_sym),
    };

    let mut instrs = vec![
        Instr::LocalGet(idx),
        Instr::StructGet(T_ITER_STATE.to_string(), 0),
    ];
    instrs.extend(emit_coerce_stack(&ValType::Anyref, &seed_ty));
    instrs.push(Instr::LocalGet(idx));
    instrs.push(Instr::StructGet(T_ITER_STATE.to_string(), 1));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, &step_ty));
    instrs.push(Instr::StructNew(state_sym));
    instrs
}

fn concrete_unfold_step_types(mono: &MonoType) -> Option<(MonoType, MonoType)> {
    let MonoType::Named { type_id, args } = mono else {
        return None;
    };
    if *type_id != UNFOLD_STEP_TYPE_ID || args.len() != 2 {
        return None;
    }
    Some((args[0].clone(), args[1].clone()))
}

fn emit_box_typed_unfold_step_local(
    idx: u32,
    yield_ty: &MonoType,
    seed_ty: &MonoType,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let step_sym = typed_unfold_step_sym(yield_ty, seed_ty);
    let yield_valtype =
        mono_to_valtype_specialized(yield_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let seed_valtype = mono_to_valtype_specialized(seed_ty, ctx.type_env, &ctx.concrete_func_sigs);

    let done_variant = vec![
        Instr::I32Const(UNFOLD_STEP_TYPE_ID.0 as i32),
        Instr::I32Const(0),
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        Instr::StructNew(T_VARIANT.to_string()),
    ];

    let mut yield_variant = vec![
        Instr::I32Const(UNFOLD_STEP_TYPE_ID.0 as i32),
        Instr::I32Const(1),
        Instr::LocalGet(idx),
        Instr::StructGet(step_sym.clone(), 1),
    ];
    yield_variant.extend(emit_coerce_stack(&yield_valtype, &ValType::Anyref));
    yield_variant.push(Instr::LocalGet(idx));
    yield_variant.push(Instr::StructGet(step_sym, 2));
    yield_variant.extend(emit_coerce_stack(&seed_valtype, &ValType::Anyref));
    yield_variant.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 2));
    yield_variant.push(Instr::StructNew(T_VARIANT.to_string()));

    vec![
        Instr::LocalGet(idx),
        Instr::StructGet(typed_unfold_step_sym(yield_ty, seed_ty), 0),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ref_variant_null()),
            then_body: done_variant,
            else_body: yield_variant,
        },
    ]
}

fn emit_default_value_instrs(ty: &ValType) -> Vec<Instr> {
    match ty {
        ValType::I32 => vec![Instr::I32Const(0)],
        ValType::I64 => vec![Instr::I64Const(0)],
        ValType::F64 => vec![Instr::F64Const(0.0)],
        ValType::Anyref => vec![Instr::RefNull(HeapType::None)],
        ValType::Ref { heap, .. } => vec![Instr::RefNull(heap.clone())],
        ValType::I31ref => vec![Instr::I32Const(0), Instr::RefI31],
        ValType::Funcref => vec![Instr::RefNull(HeapType::Func)],
        other => panic!("no default value for {:?}", other),
    }
}

fn emit_unbox_erased_unfold_step_local(
    idx: u32,
    yield_ty: &MonoType,
    seed_ty: &MonoType,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let unfold_sym = typed_unfold_step_sym(yield_ty, seed_ty);
    let yield_valtype =
        mono_to_valtype_specialized(yield_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let seed_valtype = mono_to_valtype_specialized(seed_ty, ctx.type_env, &ctx.concrete_func_sigs);

    let mut done_value = vec![Instr::I32Const(0)];
    done_value.extend(emit_default_value_instrs(&yield_valtype));
    done_value.extend(emit_default_value_instrs(&seed_valtype));
    done_value.push(Instr::StructNew(unfold_sym.clone()));

    let mut yield_value = vec![
        Instr::I32Const(1),
        Instr::LocalGet(idx),
        Instr::StructGet(T_VARIANT.to_string(), 2),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.to_string()),
        },
        Instr::I32Const(0),
        Instr::ArrayGet(T_ARRAY.to_string()),
    ];
    yield_value.extend(emit_coerce_stack(&ValType::Anyref, &yield_valtype));
    yield_value.push(Instr::LocalGet(idx));
    yield_value.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    yield_value.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });
    yield_value.push(Instr::I32Const(1));
    yield_value.push(Instr::ArrayGet(T_ARRAY.to_string()));
    yield_value.extend(emit_coerce_stack(&ValType::Anyref, &seed_valtype));
    yield_value.push(Instr::StructNew(unfold_sym));

    vec![
        Instr::LocalGet(idx),
        Instr::StructGet(T_VARIANT.to_string(), 1),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ValType::Ref {
                nullable: true,
                heap: HeapType::Named(typed_unfold_step_sym(yield_ty, seed_ty)),
            }),
            then_body: done_value,
            else_body: yield_value,
        },
    ]
}

fn emit_box_typed_iter_option_local(
    idx: u32,
    info: &IteratorStateInfo,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let option_sym = typed_iter_option_sym(info);
    let item_sym = typed_iter_item_sym(info);
    let state_sym = typed_iterator_state_sym(info);
    let yield_ty =
        mono_to_valtype_specialized(&info.yield_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let seed_ty = mono_to_valtype_specialized(&info.seed_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let step_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let step_ty = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(step_sym),
    };
    let iter_item_record = user_record_type_sym(ITER_ITEM_TYPE_ID);

    let done_variant = vec![
        Instr::I32Const(OPTION_TYPE_ID.0 as i32),
        Instr::I32Const(0),
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        Instr::StructNew(T_VARIANT.to_string()),
    ];

    let mut some_variant = vec![
        Instr::I32Const(OPTION_TYPE_ID.0 as i32),
        Instr::I32Const(1),
        Instr::LocalGet(idx),
        Instr::StructGet(option_sym.clone(), 1),
        Instr::StructGet(item_sym.clone(), 0),
    ];
    some_variant.extend(emit_coerce_stack(&yield_ty, &ValType::Anyref));
    some_variant.push(Instr::LocalGet(idx));
    some_variant.push(Instr::StructGet(option_sym.clone(), 1));
    some_variant.push(Instr::StructGet(item_sym.clone(), 1));
    some_variant.push(Instr::StructGet(state_sym.clone(), 0));
    some_variant.extend(emit_coerce_stack(&seed_ty, &ValType::Anyref));
    some_variant.push(Instr::LocalGet(idx));
    some_variant.push(Instr::StructGet(option_sym, 1));
    some_variant.push(Instr::StructGet(item_sym, 1));
    some_variant.push(Instr::StructGet(state_sym, 1));
    some_variant.extend(emit_coerce_stack(&step_ty, &ValType::Anyref));
    some_variant.push(Instr::StructNew(T_ITER_STATE.to_string()));
    some_variant.push(Instr::StructNew(iter_item_record));
    some_variant.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    some_variant.push(Instr::StructNew(T_VARIANT.to_string()));

    vec![
        Instr::LocalGet(idx),
        Instr::StructGet(typed_iter_option_sym(info), 0),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ref_variant_null()),
            then_body: done_variant,
            else_body: some_variant,
        },
    ]
}

fn emit_unbox_erased_iter_option_local(
    idx: u32,
    info: &IteratorStateInfo,
    ctx: &EmitCtx<'_>,
) -> Vec<Instr> {
    let option_sym = typed_iter_option_sym(info);
    let item_sym = typed_iter_item_sym(info);
    let state_sym = typed_iterator_state_sym(info);
    let yield_ty =
        mono_to_valtype_specialized(&info.yield_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let seed_ty = mono_to_valtype_specialized(&info.seed_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let step_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let step_ty = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(step_sym),
    };
    let iter_item_record = user_record_type_sym(ITER_ITEM_TYPE_ID);

    let done_value = vec![
        Instr::I32Const(0),
        Instr::RefNull(HeapType::Named(item_sym.clone())),
        Instr::StructNew(option_sym.clone()),
    ];

    let mut some_value = vec![
        Instr::I32Const(1),
        Instr::LocalGet(idx),
        Instr::StructGet(T_VARIANT.to_string(), 2),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.to_string()),
        },
        Instr::I32Const(0),
        Instr::ArrayGet(T_ARRAY.to_string()),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(iter_item_record.clone()),
        },
        Instr::StructGet(iter_item_record.clone(), 0),
    ];
    some_value.extend(emit_coerce_stack(&ValType::Anyref, &yield_ty));
    some_value.push(Instr::LocalGet(idx));
    some_value.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    some_value.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });
    some_value.push(Instr::I32Const(0));
    some_value.push(Instr::ArrayGet(T_ARRAY.to_string()));
    some_value.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(iter_item_record.clone()),
    });
    some_value.push(Instr::StructGet(iter_item_record.clone(), 1));
    some_value.push(Instr::StructGet(T_ITER_STATE.to_string(), 0));
    some_value.extend(emit_coerce_stack(&ValType::Anyref, &seed_ty));
    some_value.push(Instr::LocalGet(idx));
    some_value.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    some_value.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });
    some_value.push(Instr::I32Const(0));
    some_value.push(Instr::ArrayGet(T_ARRAY.to_string()));
    some_value.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(iter_item_record.clone()),
    });
    some_value.push(Instr::StructGet(iter_item_record, 1));
    some_value.push(Instr::StructGet(T_ITER_STATE.to_string(), 1));
    some_value.extend(emit_coerce_stack(&ValType::Anyref, &step_ty));
    some_value.push(Instr::StructNew(state_sym));
    some_value.push(Instr::StructNew(item_sym));
    some_value.push(Instr::StructNew(option_sym.clone()));

    vec![
        Instr::LocalGet(idx),
        Instr::StructGet(T_VARIANT.to_string(), 1),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ValType::Ref {
                nullable: true,
                heap: HeapType::Named(option_sym),
            }),
            then_body: done_value,
            else_body: some_value,
        },
    ]
}

fn emit_unbox_on_stack(expected: &ValType) -> Vec<Instr> {
    match expected {
        ValType::I64 => vec![
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.to_string()),
            },
            Instr::StructGet(T_BOXED_INT.to_string(), 0),
        ],
        ValType::F64 => vec![
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_FLOAT.to_string()),
            },
            Instr::StructGet(T_BOXED_FLOAT.to_string(), 0),
        ],
        ValType::I32 => vec![
            Instr::RefCast {
                nullable: false,
                heap: HeapType::I31,
            },
            Instr::I31GetS,
        ],
        ValType::Ref { nullable, heap } => vec![Instr::RefCast {
            nullable: *nullable,
            heap: heap.clone(),
        }],
        ValType::Anyref => Vec::new(),
        _ => panic!(
            "unsupported anyref unboxing target {:?} in Stage 8c",
            expected
        ),
    }
}

fn emit_coerce_stack(from: &ValType, to: &ValType) -> Vec<Instr> {
    if from == to {
        return Vec::new();
    }
    match (from, to) {
        (_, ValType::Anyref) => emit_box_on_stack(from),
        (ValType::Anyref, _) => emit_unbox_on_stack(to),
        (ValType::I32, ValType::I64) => vec![Instr::I64ExtendI32S],
        (ValType::I64, ValType::I32) => vec![Instr::I32WrapI64],
        (
            ValType::Ref {
                nullable: false,
                heap: from_heap,
            },
            ValType::Ref {
                nullable: true,
                heap: to_heap,
            },
        ) if from_heap == to_heap => Vec::new(),
        (
            ValType::Ref {
                nullable: true,
                heap: from_heap,
            },
            ValType::Ref {
                nullable: false,
                heap: to_heap,
            },
        ) if from_heap == to_heap => vec![Instr::RefAsNonNull],
        (ValType::Ref { .. }, ValType::Ref { nullable, heap }) => vec![Instr::RefCast {
            nullable: *nullable,
            heap: heap.clone(),
        }],
        _ => panic!("unsupported stack coercion from {:?} to {:?}", from, to),
    }
}

fn emit_binop(
    op: crate::syntax::ast::BinOp,
    left: &Atom,
    right: &Atom,
    operand_ty: crate::ir::anf::OpKind,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let operand_vt = operand_valtype(operand_ty);
    let mut instrs = emit_atom(left, Some(&operand_vt), ctx);
    instrs.extend(emit_atom(right, Some(&operand_vt), ctx));

    match (operand_ty, op) {
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Add) => instrs.push(Instr::I64Add),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Sub) => instrs.push(Instr::I64Sub),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Mul) => instrs.push(Instr::I64Mul),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Div) => {
            instrs.push(Instr::I64DivS)
        }
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Mod) => {
            instrs.push(Instr::I64RemS)
        }
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Eq) => instrs.push(Instr::I64Eq),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Ne) => instrs.push(Instr::I64Ne),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Lt) => instrs.push(Instr::I64LtS),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Le) => instrs.push(Instr::I64LeS),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Gt) => instrs.push(Instr::I64GtS),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Ge) => instrs.push(Instr::I64GeS),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::BitAnd) => {
            instrs.push(Instr::I64And)
        }
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::BitOr) => {
            instrs.push(Instr::I64Or)
        }
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::BitXor) => {
            instrs.push(Instr::I64Xor)
        }
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Shl) => instrs.push(Instr::I64Shl),
        (crate::ir::anf::OpKind::Int, crate::syntax::ast::BinOp::Shr) => {
            instrs.push(Instr::I64ShrS)
        }

        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Add) => {
            instrs.push(Instr::F64Add)
        }
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Sub) => {
            instrs.push(Instr::F64Sub)
        }
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Mul) => {
            instrs.push(Instr::F64Mul)
        }
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Div) => {
            instrs.push(Instr::F64Div)
        }
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Eq) => instrs.push(Instr::F64Eq),
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Ne) => instrs.push(Instr::F64Ne),
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Lt) => instrs.push(Instr::F64Lt),
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Le) => instrs.push(Instr::F64Le),
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Gt) => instrs.push(Instr::F64Gt),
        (crate::ir::anf::OpKind::Float, crate::syntax::ast::BinOp::Ge) => instrs.push(Instr::F64Ge),

        (crate::ir::anf::OpKind::Bool, crate::syntax::ast::BinOp::Eq) => instrs.push(Instr::I32Eq),
        (crate::ir::anf::OpKind::Bool, crate::syntax::ast::BinOp::Ne) => instrs.push(Instr::I32Ne),
        (crate::ir::anf::OpKind::Bool, crate::syntax::ast::BinOp::And) => {
            instrs.push(Instr::I32And)
        }
        (crate::ir::anf::OpKind::Bool, crate::syntax::ast::BinOp::Or) => instrs.push(Instr::I32Or),

        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Add) => {
            ensure_rt_str_concat_import(ctx);
            instrs.push(Instr::Call("rt_str__concat".to_string()));
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Eq) => {
            ensure_rt_str_eq_import(ctx);
            instrs.push(Instr::Call("rt_str__eq".to_string()));
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Ne) => {
            ensure_rt_str_eq_import(ctx);
            instrs.push(Instr::Call("rt_str__eq".to_string()));
            instrs.push(Instr::I32Eqz);
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Lt) => {
            ensure_rt_str_cmp_import(ctx);
            instrs.push(Instr::Call("rt_str__cmp".to_string()));
            instrs.push(Instr::I32Const(0));
            instrs.push(Instr::I32LtS); // cmp(a,b) < 0
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Le) => {
            ensure_rt_str_cmp_import(ctx);
            instrs.push(Instr::Call("rt_str__cmp".to_string()));
            instrs.push(Instr::I32Const(0));
            instrs.push(Instr::I32LeS); // cmp(a,b) <= 0
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Gt) => {
            ensure_rt_str_cmp_import(ctx);
            instrs.push(Instr::Call("rt_str__cmp".to_string()));
            instrs.push(Instr::I32Const(0));
            instrs.push(Instr::I32GtS); // cmp(a,b) > 0
        }
        (crate::ir::anf::OpKind::String, crate::syntax::ast::BinOp::Ge) => {
            ensure_rt_str_cmp_import(ctx);
            instrs.push(Instr::Call("rt_str__cmp".to_string()));
            instrs.push(Instr::I32Const(0));
            instrs.push(Instr::I32GeS); // cmp(a,b) >= 0
        }

        _ => panic!(
            "unsupported binop {:?} for operand type {:?}",
            op, operand_ty
        ),
    }

    instrs
}

fn emit_record_literal(
    type_id: TypeId,
    fields: &[(crate::ir::FieldId, Atom)],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let field_count = record_field_count(type_id, ctx);
    let mut ordered: Vec<Option<&Atom>> = vec![None; field_count];
    for (field_id, atom) in fields {
        if field_id.0 >= field_count {
            panic!(
                "record literal field index {} out of bounds for {} fields",
                field_id.0, field_count
            );
        }
        ordered[field_id.0] = Some(atom);
    }
    if ordered.iter().any(|slot| slot.is_none()) {
        panic!(
            "record literal missing field for type_id {} in Stage 8c Step 6",
            type_id.0
        );
    }

    let mut instrs = Vec::new();
    for (idx, atom) in ordered.into_iter().flatten().enumerate() {
        let field_ty = record_field_valtype(type_id, idx, None, None, ctx);
        instrs.extend(emit_atom(atom, Some(&field_ty), ctx));
    }
    instrs.push(Instr::StructNew(user_record_type_sym(type_id)));
    instrs.extend(emit_coerce_stack(&ref_user_record(type_id), bind_ty));
    instrs
}

fn emit_record_get(
    type_id: TypeId,
    field: crate::ir::FieldId,
    target: &Atom,
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let target_mono = ctx.infer_atom_mono(target);
    let target_iter_item_state = atom_iter_item_state(target, ctx);
    let record_sym = record_struct_sym(
        type_id,
        target_mono.as_ref(),
        target_iter_item_state.as_ref(),
    );
    let field_ty = record_field_valtype(
        type_id,
        field.0,
        target_mono.as_ref(),
        target_iter_item_state.as_ref(),
        ctx,
    );
    let mut instrs = emit_atom(
        target,
        Some(&ref_record_null(
            type_id,
            target_mono.as_ref(),
            target_iter_item_state.as_ref(),
        )),
        ctx,
    );
    instrs.push(Instr::StructGet(record_sym, field.0 as u32));
    instrs.extend(emit_coerce_stack(&field_ty, bind_ty));
    instrs
}

fn emit_record_update(
    type_id: TypeId,
    field: crate::ir::FieldId,
    base: &Atom,
    value: &Atom,
    can_reuse_in_place: bool,
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let base_mono = ctx.infer_atom_mono(base);
    let base_iter_item_state = atom_iter_item_state(base, ctx);
    let record_sym = record_struct_sym(type_id, base_mono.as_ref(), base_iter_item_state.as_ref());
    let mut instrs = Vec::new();

    if can_reuse_in_place {
        instrs.extend(emit_atom(
            base,
            Some(&ref_record_null(
                type_id,
                base_mono.as_ref(),
                base_iter_item_state.as_ref(),
            )),
            ctx,
        ));
        instrs.extend(emit_atom(
            base,
            Some(&ref_record_null(
                type_id,
                base_mono.as_ref(),
                base_iter_item_state.as_ref(),
            )),
            ctx,
        ));
        let field_ty = record_field_valtype(
            type_id,
            field.0,
            base_mono.as_ref(),
            base_iter_item_state.as_ref(),
            ctx,
        );
        instrs.extend(emit_atom(value, Some(&field_ty), ctx));
        instrs.push(Instr::StructSet(record_sym.clone(), field.0 as u32));
        instrs.extend(emit_coerce_stack(
            &ref_record_null(type_id, base_mono.as_ref(), base_iter_item_state.as_ref()),
            bind_ty,
        ));
        return instrs;
    }

    let field_count = record_field_count(type_id, ctx);
    for idx in 0..field_count {
        if idx == field.0 {
            let field_ty = record_field_valtype(
                type_id,
                idx,
                base_mono.as_ref(),
                base_iter_item_state.as_ref(),
                ctx,
            );
            instrs.extend(emit_atom(value, Some(&field_ty), ctx));
        } else {
            instrs.extend(emit_atom(
                base,
                Some(&ref_record_null(
                    type_id,
                    base_mono.as_ref(),
                    base_iter_item_state.as_ref(),
                )),
                ctx,
            ));
            instrs.push(Instr::StructGet(record_sym.clone(), idx as u32));
        }
    }
    instrs.push(Instr::StructNew(record_sym));
    instrs.extend(emit_coerce_stack(
        &ref_record(type_id, base_mono.as_ref(), base_iter_item_state.as_ref()),
        bind_ty,
    ));
    instrs
}

fn record_field_mono(
    type_id: TypeId,
    field_idx: usize,
    target_mono: Option<&MonoType>,
    ctx: &EmitCtx<'_>,
) -> MonoType {
    match ctx.type_env.get_def(type_id) {
        Some(LangTypeDef::Record { fields, .. }) => fields
            .get(field_idx)
            .map(|field| field.ty.clone())
            .unwrap_or_else(|| {
                panic!(
                    "record field index {field_idx} out of bounds for type {}",
                    type_id.0
                )
            }),
        Some(LangTypeDef::Alias { target, .. }) => match target {
            MonoType::Named { type_id, .. } => {
                record_field_mono(*type_id, field_idx, target_mono, ctx)
            }
            other => panic!(
                "record alias Type#{} points to non-record type {other:?}",
                type_id.0
            ),
        },
        Some(other) => panic!("Type#{} is not a record: {other:?}", type_id.0),
        None => panic!("unknown record type id {}", type_id.0),
    }
}

fn record_field_valtype(
    type_id: TypeId,
    field_idx: usize,
    target_mono: Option<&MonoType>,
    target_iter_item_state: Option<&IteratorStateInfo>,
    ctx: &EmitCtx<'_>,
) -> ValType {
    if let Some(info) = target_iter_item_state.filter(|_| type_id == ITER_ITEM_TYPE_ID) {
        return match field_idx {
            0 => mono_to_valtype_specialized(&info.yield_ty, ctx.type_env, &ctx.concrete_func_sigs),
            1 => ValType::Ref {
                nullable: true,
                heap: HeapType::Named(typed_iterator_state_sym(info)),
            },
            _ => panic!(
                "record field index {field_idx} out of bounds for type {}",
                type_id.0
            ),
        };
    }
    let mono = record_field_mono(type_id, field_idx, target_mono, ctx);
    mono_to_valtype_specialized(&mono, ctx.type_env, &ctx.concrete_func_sigs)
}

fn record_struct_sym(
    type_id: TypeId,
    _target_mono: Option<&MonoType>,
    target_iter_item_state: Option<&IteratorStateInfo>,
) -> String {
    if let Some(info) = target_iter_item_state.filter(|_| type_id == ITER_ITEM_TYPE_ID) {
        return typed_iter_item_sym(info);
    }
    user_record_type_sym(type_id)
}

fn ref_record(
    type_id: TypeId,
    target_mono: Option<&MonoType>,
    target_iter_item_state: Option<&IteratorStateInfo>,
) -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(record_struct_sym(
            type_id,
            target_mono,
            target_iter_item_state,
        )),
    }
}

fn ref_record_null(
    type_id: TypeId,
    target_mono: Option<&MonoType>,
    target_iter_item_state: Option<&IteratorStateInfo>,
) -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(record_struct_sym(
            type_id,
            target_mono,
            target_iter_item_state,
        )),
    }
}

/// Emit instructions for a variant literal (Option, Result, user enum, etc.).
///
/// Sum-boundary rule: when the destination `bind_ty` is a typed option/result
/// struct ref, emit a typed struct directly. When it's `anyref` or `$Variant`
/// ref, emit the universal erased `$Variant` layout. The decision is driven by
/// `expected_mono` + `bind_ty`, not by inspecting the local's `SumRepr`.
fn emit_variant_literal(
    type_id: TypeId,
    variant: crate::ir::VariantId,
    args: &[Atom],
    bind_ty: &ValType,
    expected_mono: Option<&MonoType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Typed UnfoldStep path: emit a typed struct instead of universal $Variant
    if !ctx.concrete_func_sigs.is_empty() && type_id == UNFOLD_STEP_TYPE_ID {
        let unfold_types = expected_mono
            .and_then(concrete_unfold_step_types)
            .or_else(|| resolve_unfold_step_types(variant, args, ctx));
        if let Some((yield_ty, seed_ty)) = unfold_types {
            return emit_typed_unfold_step_literal(
                variant, args, &yield_ty, &seed_ty, bind_ty, ctx,
            );
        }
        panic!(
            "missing concrete UnfoldStep typing metadata for variant {} with {} args",
            variant.0,
            args.len()
        );
    }

    // Typed general Option<T> / Result<T,E> path: emit a typed struct.
    if type_id == OPTION_TYPE_ID || type_id == RESULT_TYPE_ID {
        if let Some(mono) = expected_mono {
            if is_typed_general_sum_candidate(mono)
                && typed_general_option_can_materialize_to(bind_ty, mono)
            {
                return emit_typed_general_option_literal(variant, args, mono, bind_ty, ctx);
            }
        }
    }

    let mut instrs = vec![
        Instr::I32Const(type_id.0 as i32),
        Instr::I32Const(variant.0 as i32),
    ];
    for arg in args {
        instrs.extend(emit_atom(arg, Some(&ValType::Anyref), ctx));
    }
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), args.len() as u32));
    instrs.push(Instr::StructNew(T_VARIANT.to_string()));
    instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
    instrs
}

fn typed_general_option_can_materialize_to(bind_ty: &ValType, mono: &MonoType) -> bool {
    match bind_ty {
        ValType::Anyref => true,
        ValType::Ref {
            heap: HeapType::Named(name),
            ..
        } => name == &typed_general_option_sym(mono),
        _ => false,
    }
}

/// Emit a typed general Option or Result literal.
/// Option None:    (variant_id=0, payload=default)
/// Option Some(x): (variant_id=1, payload=x)
/// Result Ok(x):   (variant_id=0, ok_payload=x, err_payload=default)
/// Result Err(e):  (variant_id=1, ok_payload=default, err_payload=e)
fn emit_typed_general_option_literal(
    variant: crate::ir::VariantId,
    args: &[Atom],
    mono: &MonoType,
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let sym = typed_general_option_sym(mono);
    ctx.request_typed_general_option(sym.clone(), mono.clone());

    match mono {
        MonoType::Named {
            type_id,
            args: type_args,
        } if *type_id == OPTION_TYPE_ID && type_args.len() == 1 => {
            let payload_ty =
                mono_to_valtype_specialized(&type_args[0], ctx.type_env, &ctx.concrete_func_sigs);
            let mut instrs = vec![Instr::I32Const(variant.0 as i32)];
            if variant.0 == 1 && args.len() == 1 {
                instrs.extend(emit_atom(&args[0], Some(&payload_ty), ctx));
            } else {
                instrs.extend(emit_default_value_instrs(&payload_ty));
            }
            instrs.push(Instr::StructNew(sym));
            instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
            instrs
        }
        MonoType::Named {
            type_id,
            args: type_args,
        } if *type_id == RESULT_TYPE_ID && type_args.len() == 2 => {
            let ok_ty =
                mono_to_valtype_specialized(&type_args[0], ctx.type_env, &ctx.concrete_func_sigs);
            let err_ty =
                mono_to_valtype_specialized(&type_args[1], ctx.type_env, &ctx.concrete_func_sigs);
            let mut instrs = vec![Instr::I32Const(variant.0 as i32)];
            if variant.0 == 0 && args.len() == 1 {
                // Ok(value): push ok payload, default err
                instrs.extend(emit_atom(&args[0], Some(&ok_ty), ctx));
                instrs.extend(emit_default_value_instrs(&err_ty));
            } else if variant.0 == 1 && args.len() == 1 {
                // Err(value): default ok, push err payload
                instrs.extend(emit_default_value_instrs(&ok_ty));
                instrs.extend(emit_atom(&args[0], Some(&err_ty), ctx));
            } else {
                panic!(
                    "unexpected Result variant {} with {} args",
                    variant.0,
                    args.len()
                );
            }
            instrs.push(Instr::StructNew(sym));
            instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
            instrs
        }
        _ => panic!("emit_typed_general_option_literal: expected Option<T> or Result<T,E>"),
    }
}

fn emit_typed_unfold_step_literal(
    variant: crate::ir::VariantId,
    args: &[Atom],
    yield_ty: &MonoType,
    seed_ty: &MonoType,
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let sym = typed_unfold_step_sym(yield_ty, seed_ty);
    let yield_valtype =
        mono_to_valtype_specialized(yield_ty, ctx.type_env, &ctx.concrete_func_sigs);
    let seed_valtype = mono_to_valtype_specialized(seed_ty, ctx.type_env, &ctx.concrete_func_sigs);

    ctx.request_typed_unfold_step(sym.clone(), yield_ty.clone(), seed_ty.clone());

    let mut instrs = vec![Instr::I32Const(variant.0 as i32)];

    if variant.0 == 1 && args.len() == 2 {
        // Yield(value, next_seed): push concrete fields
        instrs.extend(emit_atom(&args[0], Some(&yield_valtype), ctx));
        instrs.extend(emit_atom(&args[1], Some(&seed_valtype), ctx));
    } else {
        // Done: push default values for unused fields
        instrs.extend(emit_default_value(&yield_valtype));
        instrs.extend(emit_default_value(&seed_valtype));
    }

    instrs.push(Instr::StructNew(sym.clone()));
    let result_ty = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(sym),
    };
    instrs.extend(emit_coerce_stack(&result_ty, bind_ty));
    instrs
}

/// Emit a default/zero value for a given Wasm type (used for unused fields in typed structs).
fn emit_default_value(ty: &ValType) -> Vec<Instr> {
    match ty {
        ValType::I32 => vec![Instr::I32Const(0)],
        ValType::I64 => vec![Instr::I64Const(0)],
        ValType::F64 => vec![Instr::F64Const(0.0)],
        ValType::Anyref | ValType::Ref { .. } | ValType::I31ref | ValType::Funcref => {
            vec![Instr::RefNull(HeapType::None)]
        }
        ValType::F32 => vec![Instr::F64Const(0.0)], // unlikely but safe
        ValType::I8 => vec![Instr::I32Const(0)],
    }
}

fn emit_array_literal(
    elems: &[Atom],
    bind_ty: &ValType,
    elem_mono: Option<&MonoType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    use crate::runtime::types::T_VEC_INTERNAL;
    let mut instrs = Vec::new();

    if elems.is_empty() {
        // Empty vector: use the global singleton
        instrs.push(Instr::GlobalGet("rt_arr__empty_pvec".to_string()));
        instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
        return instrs;
    }

    let elem_val_ty = elem_mono
        .map(|mono| mono_to_valtype_specialized(mono, ctx.type_env, &ctx.concrete_func_sigs));

    if elems.len() <= 32 {
        // Small literal: tail-only PVec
        // Push PVec fields in order: len, shift, root, then build tail
        instrs.push(Instr::I32Const(elems.len() as i32));
        instrs.push(Instr::I32Const(0)); // shift = 0
        instrs.push(Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.to_string())));
        // Build tail: elements → ArrayNewFixed
        for elem in elems {
            if let Some(elem_ty) = elem_val_ty.as_ref() {
                instrs.extend(emit_atom(elem, Some(elem_ty), ctx));
                instrs.extend(emit_coerce_stack(elem_ty, &ValType::Anyref));
            } else {
                instrs.extend(emit_atom(elem, Some(&ValType::Anyref), ctx));
            }
        }
        instrs.push(Instr::ArrayNewFixed(
            T_ARRAY.to_string(),
            elems.len() as u32,
        ));
        instrs.push(Instr::StructNew(T_PVEC.to_string()));
    } else {
        // Large literal (>32): use repeated push
        ensure_rt_arr_push_import(ctx);
        instrs.push(Instr::GlobalGet("rt_arr__empty_pvec".to_string()));
        for elem in elems {
            if let Some(elem_ty) = elem_val_ty.as_ref() {
                instrs.extend(emit_atom(elem, Some(elem_ty), ctx));
                instrs.extend(emit_coerce_stack(elem_ty, &ValType::Anyref));
            } else {
                instrs.extend(emit_atom(elem, Some(&ValType::Anyref), ctx));
            }
            instrs.push(Instr::Call("rt_arr__push".to_string()));
        }
    }
    instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
    instrs
}

fn emit_index_op(
    base: &Atom,
    index: &Atom,
    base_ty: crate::ir::anf::IndexKind,
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let mut instrs = Vec::new();
    match base_ty {
        crate::ir::anf::IndexKind::Array => {
            ensure_rt_arr_get_import(ctx);
            instrs.extend(emit_atom(base, Some(&ref_pvec_null()), ctx));
            instrs.extend(emit_index_as_i32(index, ctx));
            instrs.push(Instr::Call("rt_arr__get".to_string()));
            instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
        }
        crate::ir::anf::IndexKind::Dict => {
            // Dict indexing returns Option<V>, so use get_option which returns a
            // proper Variant (Option.None/Some) instead of raw anyref.
            ensure_rt_dict_get_option_import(ctx);
            instrs.extend(emit_atom(base, Some(&ref_pdict_null()), ctx));
            instrs.extend(emit_atom(index, Some(&ValType::Anyref), ctx));
            instrs.push(Instr::Call("rt_dict__get_option".to_string()));
            instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
        }
        crate::ir::anf::IndexKind::String => {
            // String indexing: read byte at byte offset, return as i32 (Byte)
            // ArrayGetU on $String (array<i8>) returns i32, traps on OOB.
            // Guard in i64 domain first so large Int values cannot wrap via i32.
            instrs.extend(emit_trap_unless(emit_string_index_in_bounds(
                base, index, ctx,
            )));
            instrs.extend(emit_atom(base, Some(&ref_string_null()), ctx));
            instrs.push(Instr::RefAsNonNull);
            instrs.extend(emit_index_as_i32(index, ctx));
            instrs.push(Instr::ArrayGetU(T_STRING.to_string()));
            // ArrayGetU gives i32, which is the Byte representation
            instrs.extend(emit_coerce_stack(&ValType::I32, bind_ty));
        }
    }
    instrs
}

fn emit_index_as_i32(index: &Atom, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = emit_atom(index, Some(&ValType::I64), ctx);
    instrs.push(Instr::I32WrapI64);
    instrs
}

fn emit_string_index_in_bounds(string: &Atom, index: &Atom, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Produces i32 condition:
    //   (index >= 0) && (index < len(string))
    // in full i64 domain before any i32 narrowing.
    let mut instrs = Vec::new();
    instrs.extend(emit_atom(index, Some(&ValType::I64), ctx));
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS);
    instrs.extend(emit_atom(index, Some(&ValType::I64), ctx));
    instrs.extend(emit_atom(string, Some(&ref_string_null()), ctx));
    instrs.push(Instr::ArrayLen);
    instrs.push(Instr::I64ExtendI32U);
    instrs.push(Instr::I64LtS);
    instrs.push(Instr::I32And);
    instrs
}

fn emit_trap_unless(mut cond_instrs: Vec<Instr>) -> Vec<Instr> {
    cond_instrs.push(Instr::If {
        result: None,
        then_body: vec![],
        else_body: vec![Instr::Unreachable],
    });
    cond_instrs
}

fn record_field_count(type_id: TypeId, ctx: &EmitCtx<'_>) -> usize {
    ctx.type_env
        .get_record_fields(type_id)
        .map(|fields| fields.len())
        .unwrap_or_else(|| panic!("missing record type metadata for TypeId({})", type_id.0))
}

fn emit_call(callee: &Atom, args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    match callee {
        Atom::AGlobalFunc(func_id) => {
            if let Some(entry) = ctx.prelude.get(func_id).cloned() {
                emit_prelude_call(*func_id, &entry, args, bind_ty, ctx)
            } else {
                emit_direct_user_call(*func_id, args, bind_ty, ctx)
            }
        }
        Atom::ALocal(_) => emit_closure_call(callee, args, bind_ty, ctx),
        _ => panic!("unsupported non-global callee atom in call: {:?}", callee),
    }
}

fn emit_closure_call(
    callee: &Atom,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Typed closure path (Stage 9.6): if the callee local holds a typed closure,
    // use concrete arg types and a typed call_ref — no anyref boxing.
    // The callee may be typed as $Closure (function param) or typed closure struct.
    // We cast to the typed struct to access field 2 (typed funcref).
    if !ctx.concrete_func_sigs.is_empty() {
        if let Atom::ALocal(local_id) = callee {
            if let Some((params, ret)) = ctx.local_typed_closure_sig(*local_id) {
                let closurefunc_sym = typed_closurefunc_sym(&params, &ret);
                let closure_sym = typed_closure_struct_sym(&params, &ret);
                // Cast callee to the typed closure struct subtype.
                // The callee might be stored as (ref null $Closure) for a
                // function param, but the actual runtime value is always the
                // typed subtype struct.
                let typed_ref = ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(closure_sym.clone()),
                };
                // Push env (field 1, inherited from $Closure).
                let mut instrs = emit_atom(callee, Some(&typed_ref), ctx);
                instrs.push(Instr::StructGet(closure_sym.clone(), 1));
                // Push concrete args.
                for (arg, param_ty) in args.iter().zip(params.iter()) {
                    let wasm_ty = mono_to_valtype_specialized(
                        param_ty,
                        ctx.type_env,
                        &ctx.concrete_func_sigs,
                    );
                    instrs.extend(emit_atom(arg, Some(&wasm_ty), ctx));
                }
                // Push typed funcref (field 2) last for call_ref.
                instrs.extend(emit_atom(callee, Some(&typed_ref), ctx));
                instrs.push(Instr::StructGet(closure_sym, 2));
                instrs.push(Instr::CallRef(closurefunc_sym));
                #[cfg(debug_assertions)]
                bump_boundary!(typed_closure_calls);
                match ret {
                    MonoType::Void | MonoType::Never => {
                        instrs.extend(emit_void_value(Some(bind_ty)));
                    }
                    _ => {
                        let ret_ty = mono_to_valtype_for_user_abi_result(
                            &ret,
                            ctx.type_env,
                            &ctx.concrete_func_sigs,
                        );
                        instrs.extend(emit_coerce_stack(&ret_ty, bind_ty));
                    }
                }
                return instrs;
            }
        }
    } // end if !concrete_func_sigs.is_empty()

    // Universal closure path.
    let mut instrs = emit_atom(callee, Some(&ref_closure_null()), ctx);
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 1));

    if args.is_empty() {
        instrs.push(Instr::RefNull(HeapType::None));
    } else {
        for arg in args {
            instrs.extend(emit_atom(arg, Some(&ValType::Anyref), ctx));
        }
        instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), args.len() as u32));
    }

    instrs.extend(emit_atom(callee, Some(&ref_closure_null()), ctx));
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 0));
    instrs.push(Instr::CallRef(T_CLOSURE_FUNC.to_string()));
    #[cfg(debug_assertions)]
    bump_boundary!(universal_closure_calls);
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_direct_user_call(
    func_id: FuncId,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let abi = ctx
        .user_func_abi(func_id)
        .unwrap_or_else(|| panic!("missing ABI for function FuncId({})", func_id.0));

    if abi.params.len() != args.len() {
        panic!(
            "arity mismatch for direct call to FuncId({}): expected {}, got {}",
            func_id.0,
            abi.params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (arg, param_ty) in args.iter().zip(abi.params.iter()) {
        if let Some(specialized) = emit_specialized_closure_arg(arg, param_ty, ctx) {
            instrs.extend(specialized);
        } else {
            instrs.extend(emit_atom(arg, Some(param_ty), ctx));
        }
    }
    instrs.push(Instr::Call(user_func_sym(func_id)));

    match abi.results.first() {
        Some(result_ty) => instrs.extend(emit_coerce_stack(&result_ty, bind_ty)),
        None => {
            instrs.extend(emit_void_value(Some(bind_ty)));
        }
    }

    instrs
}

fn emit_make_closure(
    func_id: FuncId,
    free_vars: &[crate::ir::LocalId],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Sort free_vars by LocalId to match closure capture layout ordering used
    // by function signatures and trampolines.
    let mut sorted_vars = free_vars.to_vec();
    sorted_vars.sort_by_key(|id| id.0);
    sorted_vars.dedup_by_key(|id| id.0);

    // Typed closure path (Stage 9.6): if this function has a concrete signature,
    // create a typed closure struct (subtype of $Closure). This works in all
    // contexts: typed call sites use field 2 (typed funcref), universal call
    // sites use fields 0+1 (universal funcref + env) via the $Closure supertype.
    if let Some((params, ret)) = ctx.concrete_func_sigs.get(&func_id).cloned() {
        let closure_sym = typed_closure_struct_sym(&params, &ret);
        let typed_closure_ref = ValType::Ref {
            nullable: false,
            heap: HeapType::Named(closure_sym.clone()),
        };
        // field 0: universal funcref (for compatibility with $Closure dispatch)
        let mut instrs = vec![Instr::RefFunc(global_func_trampoline_sym(func_id))];
        // field 1: env
        for local_id in &sorted_vars {
            instrs.extend(emit_atom(
                &Atom::ALocal(*local_id),
                Some(&ValType::Anyref),
                ctx,
            ));
        }
        instrs.push(Instr::ArrayNewFixed(
            T_CLOSURE_ENV.to_string(),
            sorted_vars.len() as u32,
        ));
        // field 2: typed funcref
        instrs.push(Instr::RefFunc(typed_closure_trampoline_sym(func_id)));
        instrs.push(Instr::StructNew(closure_sym));
        instrs.extend(emit_coerce_stack(&typed_closure_ref, bind_ty));
        return instrs;
    }

    // Universal closure path (no concrete sig available).
    let mut instrs = vec![Instr::RefFunc(global_func_trampoline_sym(func_id))];
    for local_id in &sorted_vars {
        instrs.extend(emit_atom(
            &Atom::ALocal(*local_id),
            Some(&ValType::Anyref),
            ctx,
        ));
    }
    instrs.push(Instr::ArrayNewFixed(
        T_CLOSURE_ENV.to_string(),
        sorted_vars.len() as u32,
    ));
    instrs.push(Instr::StructNew(T_CLOSURE.to_string()));
    instrs.extend(emit_coerce_stack(&ref_closure(), bind_ty));
    instrs
}

fn emit_user_closure_trampoline(
    func: &AnfFunctionDef,
    capture_count: usize,
    ctx: &EmitCtx<'_>,
) -> FuncDef {
    let func_id = func.func_id;
    let abi = ctx
        .user_func_abi(func_id)
        .unwrap_or_else(|| panic!("missing ABI for trampoline FuncId({})", func_id.0));
    let mut body = Vec::new();
    for (idx, param_ty) in abi.params.iter().take(func.param_tys.len()).enumerate() {
        body.push(Instr::LocalGet(1));
        body.push(Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.to_string()),
        });
        body.push(Instr::I32Const(idx as i32));
        body.push(Instr::ArrayGet(T_ARRAY.to_string()));
        body.extend(emit_unbox_on_stack(param_ty));
    }
    for capture_idx in 0..capture_count {
        body.push(Instr::LocalGet(0));
        body.push(Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_CLOSURE_ENV.to_string()),
        });
        body.push(Instr::I32Const(capture_idx as i32));
        body.push(Instr::ArrayGet(T_CLOSURE_ENV.to_string()));
    }
    body.push(Instr::Call(user_func_sym(func_id)));
    let mut locals = Vec::new();
    let typed_unfold_step_result = abi
        .semantic_result_mono
        .as_ref()
        .and_then(concrete_unfold_step_types)
        .filter(|(yield_ty, seed_ty)| {
            matches!(
                abi.results.first(),
                Some(ValType::Ref {
                    heap: HeapType::Named(sym),
                    ..
                }) if sym == &typed_unfold_step_sym(yield_ty, seed_ty)
            )
        });
    match typed_unfold_step_result {
        Some((yield_ty, seed_ty)) => {
            let temp_idx = 2;
            locals.push(ValType::Ref {
                nullable: true,
                heap: HeapType::Named(typed_unfold_step_sym(&yield_ty, &seed_ty)),
            });
            body.push(Instr::LocalSet(temp_idx));
            body.extend(emit_box_typed_unfold_step_local(
                temp_idx, &yield_ty, &seed_ty, ctx,
            ));
        }
        None => match abi.results.first() {
            Some(result_ty) => body.extend(emit_coerce_stack(&result_ty, &ValType::Anyref)),
            None => body.extend(emit_void_value(Some(&ValType::Anyref))),
        },
    }

    FuncDef {
        name: global_func_trampoline_sym(func_id),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::Anyref],
        locals,
        body,
    }
}

fn emit_prelude_call(
    func_id: FuncId,
    entry: &crate::codegen::prelude::PreludeEntry,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if entry.is_runtime_call() {
        return emit_runtime_prelude_call(func_id, entry, args, bind_ty, ctx);
    }

    let Some(kind) = registry::lowering_kind(func_id) else {
        return emit_unimplemented_intrinsic_prelude_call(entry, ctx);
    };

    match kind {
        LoweringKind::StringToStringIdentity => emit_string_to_string_identity(args, bind_ty, ctx),
        LoweringKind::VectorPush => emit_array_append_intrinsic(args, bind_ty, ctx),
        LoweringKind::Range => emit_range_ctor_intrinsic(args, bind_ty, ctx),
        LoweringKind::RangeFrom => emit_range_from_intrinsic(args, bind_ty, ctx),
        LoweringKind::RangeStep => emit_range_step_intrinsic(args, bind_ty, ctx),
        LoweringKind::CellNew => emit_cell_new_intrinsic(args, bind_ty, ctx),
        LoweringKind::CellGet => emit_cell_get_intrinsic(args, bind_ty, ctx),
        LoweringKind::CellSet => emit_cell_set_intrinsic(args, bind_ty, ctx),
        LoweringKind::CellUpdate => emit_cell_update_intrinsic(args, bind_ty, ctx),
        LoweringKind::DictGetUnsafe => emit_dict_get_unsafe_intrinsic(args, bind_ty, ctx),
        LoweringKind::IteratorUnfold => emit_iterator_unfold_intrinsic(args, bind_ty, ctx),
        LoweringKind::IteratorNext => emit_iterator_next_intrinsic(args, bind_ty, ctx),
        LoweringKind::VectorMake => emit_vector_make_intrinsic(args, bind_ty, ctx),
        LoweringKind::VectorGet => emit_vector_get_intrinsic(args, bind_ty, ctx),
        LoweringKind::VectorSet => emit_vector_set_intrinsic(args, bind_ty, ctx),
        LoweringKind::VectorSetInPlace => emit_vector_set_in_place_intrinsic(args, bind_ty, ctx),
        LoweringKind::StringGet => emit_string_get_intrinsic(args, bind_ty, ctx),
        LoweringKind::StringSlice => emit_string_slice_intrinsic(args, bind_ty, ctx),
        LoweringKind::CharCodeAt => emit_char_code_at_intrinsic(args, bind_ty, ctx),
        LoweringKind::FromCharCode => emit_from_char_code_intrinsic(args, bind_ty, ctx),
        LoweringKind::FromCodePoint => emit_from_code_point_intrinsic(args, bind_ty, ctx),
        LoweringKind::StringUtf8Bytes => emit_string_utf8_bytes_intrinsic(args, bind_ty, ctx),
        LoweringKind::StringFromUtf8 => emit_string_from_utf8_intrinsic(args, bind_ty, ctx),
        LoweringKind::IntFromString => emit_int_from_string_intrinsic(args, bind_ty, ctx),
        LoweringKind::FloatFromString => emit_float_from_string_intrinsic(args, bind_ty, ctx),
        LoweringKind::ByteToInt => emit_byte_to_int_intrinsic(args, ctx),
        LoweringKind::ByteFromInt => emit_byte_from_int_intrinsic(args, ctx),
        LoweringKind::ByteToString => emit_byte_to_string_intrinsic(args, ctx),
        LoweringKind::FloatBits => emit_float_bits_intrinsic(args, ctx),
    }
}

fn emit_string_to_string_identity(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 1 {
        panic!("string_to_string expects exactly one argument");
    }
    let mut instrs = emit_atom(&args[0], Some(&ref_string_null()), ctx);
    instrs.extend(emit_coerce_stack(&ref_string_null(), bind_ty));
    instrs
}

fn emit_range_ctor_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 1 {
        panic!("range expects 1 arg, got {}", args.len());
    }
    emit_range_intrinsic(
        &[Atom::ALitInt(0), args[0].clone(), Atom::ALitInt(1)],
        bind_ty,
        ctx,
    )
}

fn emit_range_from_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 2 {
        panic!("range_from expects 2 args, got {}", args.len());
    }
    emit_range_intrinsic(
        &[args[0].clone(), args[1].clone(), Atom::ALitInt(1)],
        bind_ty,
        ctx,
    )
}

fn emit_range_step_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 3 {
        panic!("range_step expects 3 args, got {}", args.len());
    }
    emit_range_intrinsic(
        &[args[0].clone(), args[1].clone(), args[2].clone()],
        bind_ty,
        ctx,
    )
}

fn emit_dict_get_unsafe_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 2 {
        panic!("dict_get_unsafe expects 2 args, got {}", args.len());
    }
    ensure_rt_dict_get_import(ctx);
    let mut instrs = emit_atom(&args[0], Some(&ref_pdict_null()), ctx);
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::Call("rt_dict__get".to_string()));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_array_append_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 2 {
        panic!("Array.append intrinsic expects 2 args, got {}", args.len());
    }

    ensure_rt_arr_push_import(ctx);

    let mut instrs = emit_atom(&args[0], Some(&ref_pvec_null()), ctx);
    instrs.push(Instr::RefAsNonNull);
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::Call("rt_arr__push".to_string()));
    instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
    instrs
}

// --- Vector safe/make intrinsics ---

/// `Vector.make(size: Int, fill: T) -> Vector<T>`
/// Calls rt_arr__make(len_i32, fill_anyref) -> PVec
fn emit_vector_make_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    assert_eq!(args.len(), 2, "Vector.make expects 2 args");

    ensure_rt_arr_make_import(ctx);

    let mut instrs = Vec::new();
    // size (Int = i64) → i32
    instrs.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
    instrs.push(Instr::I32WrapI64);
    // fill value (anyref)
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::Call("rt_arr__make".to_string()));
    instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
    instrs
}

/// `v.get(i: Int) -> Option<T>`
/// Bounds-checked: returns Some(v[i]) or None.
/// ANF guarantees args are atoms (locals/literals), safe to re-emit.
fn emit_vector_get_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    use crate::types::ty::OPTION_TYPE_ID;
    assert_eq!(args.len(), 2, "Vector.get expects 2 args");

    ensure_rt_arr_get_import(ctx);

    let mut instrs = Vec::new();

    // condition: i_i32 < pvec.len (unsigned comparison for negative index safety)
    instrs.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    instrs.push(Instr::I32WrapI64); // lhs = i as i32
    instrs.extend(emit_atom(&args[0], Some(&ref_pvec_null()), ctx));
    instrs.push(Instr::StructGet(T_PVEC.to_string(), 0)); // rhs = pvec.len
    instrs.push(Instr::I32LtU);

    // then: Some(get(vec, i))
    let mut then_body = vec![Instr::I32Const(OPTION_TYPE_ID.0 as i32), Instr::I32Const(1)];
    then_body.extend(emit_atom(&args[0], Some(&ref_pvec_null()), ctx));
    then_body.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    then_body.push(Instr::I32WrapI64);
    then_body.push(Instr::Call("rt_arr__get".to_string()));
    then_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    then_body.push(Instr::StructNew(T_VARIANT.to_string()));

    let else_body = vec![
        Instr::I32Const(OPTION_TYPE_ID.0 as i32),
        Instr::I32Const(0), // None
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        Instr::StructNew(T_VARIANT.to_string()),
    ];

    instrs.push(Instr::If {
        result: Some(ref_variant()),
        then_body,
        else_body,
    });
    instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
    instrs
}

/// `v.set(i: Int, val: T) -> Option<Vector<T>>`
/// Bounds-checked: returns Some(updated_vec) or None.
/// ANF guarantees args are atoms (locals/literals), safe to re-emit.
fn emit_vector_set_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    use crate::types::ty::OPTION_TYPE_ID;
    assert_eq!(args.len(), 3, "Vector.set expects 3 args");

    ensure_rt_arr_set_import(ctx);

    let mut instrs = Vec::new();

    // condition: i_i32 < pvec.len
    instrs.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    instrs.push(Instr::I32WrapI64);
    instrs.extend(emit_atom(&args[0], Some(&ref_pvec_null()), ctx));
    instrs.push(Instr::StructGet(T_PVEC.to_string(), 0)); // pvec.len
    instrs.push(Instr::I32LtU);

    // then: Some(rt_arr__set(vec, i, val))
    let mut then_body = vec![Instr::I32Const(OPTION_TYPE_ID.0 as i32), Instr::I32Const(1)];
    then_body.extend(emit_atom(&args[0], Some(&ref_pvec_null()), ctx));
    then_body.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    then_body.push(Instr::I32WrapI64);
    then_body.extend(emit_atom(&args[2], Some(&ValType::Anyref), ctx));
    then_body.push(Instr::Call("rt_arr__set".to_string()));
    then_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    then_body.push(Instr::StructNew(T_VARIANT.to_string()));

    let else_body = vec![
        Instr::I32Const(OPTION_TYPE_ID.0 as i32),
        Instr::I32Const(0), // None
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        Instr::StructNew(T_VARIANT.to_string()),
    ];

    instrs.push(Instr::If {
        result: Some(ref_variant()),
        then_body,
        else_body,
    });
    instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
    instrs
}

/// Internal collect helper:
/// `__vector_set_in_place(vec: Vector<T>, i: Int, val: T) -> Vector<T>`
///
/// With persistent vector trie, this lowers to the persistent `set` (path-copy)
/// rather than raw in-place mutation. True in-place leaf mutation is a future
/// optimization.
fn emit_vector_set_in_place_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    assert_eq!(args.len(), 3, "__vector_set_in_place expects 3 args");

    ensure_rt_arr_set_import(ctx);

    // Call persistent set: rt_arr__set(vec, idx, val) -> PVec
    let mut instrs = emit_atom(&args[0], Some(&ref_pvec_null()), ctx);
    instrs.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    instrs.push(Instr::I32WrapI64);
    instrs.extend(emit_atom(&args[2], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::Call("rt_arr__set".to_string()));
    instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
    instrs
}

// --- Range intrinsics ---

fn emit_range_intrinsic(fields: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    use crate::types::ty::RANGE_TYPE_ID;
    let range_sym = user_record_type_sym(RANGE_TYPE_ID);
    let mut instrs = Vec::new();
    for (idx, atom) in fields.iter().enumerate() {
        let field_ty = record_field_valtype(RANGE_TYPE_ID, idx, None, None, ctx);
        instrs.extend(emit_atom(atom, Some(&field_ty), ctx));
    }
    instrs.push(Instr::StructNew(range_sym.clone()));
    let result_ty = ValType::Ref {
        nullable: false,
        heap: HeapType::Named(range_sym),
    };
    instrs.extend(emit_coerce_stack(&result_ty, bind_ty));
    instrs
}

// --- Cell intrinsics ---
// Fallback Cell representation is a 1-element mutable rt_types__Array (anyref[1]).

fn unfold_step_type(item_ty: MonoType, seed_ty: MonoType) -> MonoType {
    MonoType::Named {
        type_id: UNFOLD_STEP_TYPE_ID,
        args: vec![item_ty, seed_ty],
    }
}

fn atom_iter_item_state(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_iter_item_state(*local_id),
        _ => None,
    }
}

fn atom_iterator_next_state(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_iterator_next_state(*local_id).or_else(|| {
            // Fallback: recover typed iterator-next metadata from the local's
            // physical Wasm ref type when flow metadata is missing/stale.
            let (_, local_ty) = ctx.local(*local_id)?;
            typed_iter_option_info_for_valtype(local_ty, ctx)
        }),
        _ => None,
    }
}

fn atom_typed_unfold_step(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<(MonoType, MonoType)> {
    let Atom::ALocal(local_id) = atom else {
        return None;
    };
    let (_, local_ty) = ctx.local(*local_id)?;
    let mono = ctx.infer_atom_mono(atom)?;
    let (yield_ty, seed_ty) = concrete_unfold_step_types(&mono)?;
    let expected_sym = typed_unfold_step_sym(&yield_ty, &seed_ty);
    match local_ty {
        ValType::Ref {
            heap: HeapType::Named(sym),
            ..
        } if sym == &expected_sym => Some((yield_ty, seed_ty)),
        _ => None,
    }
}

fn atom_typed_general_option(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<MonoType> {
    match atom {
        Atom::ALocal(local_id) => {
            let mono = ctx.local_typed_option(*local_id)?.clone();
            local_can_store_typed_option(*local_id, &mono, ctx).then_some(mono)
        }
        _ => None,
    }
}

fn iterator_state_from_op(op: &AnfOp, ctx: &EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id) if *func_id == prelude_ids::ITERATOR_UNFOLD => {
                iterator_state_from_unfold_args(args.first()?, args.get(1)?, ctx)
            }
            Atom::ALocal(_) => None,
            _ => None,
        },
        AnfOp::AInit { value } => atom_iterator_state(value, ctx),
        _ => None,
    }
}

fn atom_typed_general_option_with_flow(
    atom: &Atom,
    ctx: &EmitCtx<'_>,
    flow: &HashMap<crate::ir::LocalId, MonoType>,
) -> Option<MonoType> {
    match atom {
        Atom::ALocal(local_id) => flow
            .get(local_id)
            .cloned()
            .or_else(|| atom_typed_general_option(atom, ctx)),
        _ => None,
    }
}

fn op_typed_general_option_source(
    local: crate::ir::LocalId,
    op: &AnfOp,
    ctx: &EmitCtx<'_>,
    flow: &HashMap<crate::ir::LocalId, MonoType>,
) -> Option<MonoType> {
    let inferred = ctx
        .infer_let_op_mono_for_emit(local, op)
        .filter(is_typed_general_sum_candidate);
    match op {
        AnfOp::AVariant { type_id, .. }
            if *type_id == OPTION_TYPE_ID || *type_id == RESULT_TYPE_ID =>
        {
            inferred
        }
        AnfOp::AInit { value } => atom_typed_general_option_with_flow(value, ctx, flow)
            .filter(|mono| inferred.as_ref() == Some(mono)),
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            let then_src = if expr_always_diverges(then_branch) {
                None
            } else {
                let mut then_flow = flow.clone();
                expr_typed_general_option_source(then_branch, ctx, &mut then_flow)
            };
            let else_src = if expr_always_diverges(else_branch) {
                None
            } else {
                let mut else_flow = flow.clone();
                expr_typed_general_option_source(else_branch, ctx, &mut else_flow)
            };
            let branch_src = match (then_src, else_src) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), None) if expr_always_diverges(else_branch) => Some(a),
                (None, Some(b)) if expr_always_diverges(then_branch) => Some(b),
                _ => None,
            };
            branch_src.filter(|mono| inferred.as_ref() == Some(mono))
        }
        AnfOp::AMatch { arms, .. } => {
            let mut arm_src: Option<MonoType> = None;
            for arm in arms.iter().filter(|arm| !expr_always_diverges(&arm.body)) {
                let mut arm_flow = flow.clone();
                let current = expr_typed_general_option_source(&arm.body, ctx, &mut arm_flow)?;
                match &arm_src {
                    None => arm_src = Some(current),
                    Some(existing) if *existing == current => {}
                    Some(_) => return None,
                }
            }
            arm_src.filter(|mono| inferred.as_ref() == Some(mono))
        }
        _ => None,
    }
}

fn expr_typed_general_option_source(
    expr: &AnfExpr,
    ctx: &EmitCtx<'_>,
    flow: &mut HashMap<crate::ir::LocalId, MonoType>,
) -> Option<MonoType> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            let prev_local = flow.get(local).cloned();
            let local_mono = op_typed_general_option_source(*local, op.as_ref(), ctx, flow);
            if let Some(mono) = local_mono {
                flow.insert(*local, mono);
            } else {
                flow.remove(local);
            }

            let assign_restore = if let AnfOp::AAssign {
                local: target,
                value,
            } = op.as_ref()
            {
                let prev = flow.get(target).cloned();
                if let Some(mono) = atom_typed_general_option_with_flow(value, ctx, flow) {
                    flow.insert(*target, mono);
                } else {
                    flow.remove(target);
                }
                Some((*target, prev))
            } else {
                None
            };

            let result = expr_typed_general_option_source(body, ctx, flow);

            if let Some((target, prev)) = assign_restore {
                if let Some(mono) = prev {
                    flow.insert(target, mono);
                } else {
                    flow.remove(&target);
                }
            }
            if let Some(mono) = prev_local {
                flow.insert(*local, mono);
            } else {
                flow.remove(local);
            }
            result
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            atom_typed_general_option_with_flow(atom, ctx, flow)
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => None,
    }
}

fn typed_general_option_from_op(
    local: crate::ir::LocalId,
    op: &AnfOp,
    ctx: &EmitCtx<'_>,
) -> Option<MonoType> {
    match op {
        AnfOp::AVariant { type_id, .. }
            if *type_id == OPTION_TYPE_ID || *type_id == RESULT_TYPE_ID =>
        {
            ctx.infer_let_op_mono_for_emit(local, op)
                .filter(is_typed_general_sum_candidate)
        }
        AnfOp::AIf { .. } => op_typed_general_option_source(local, op, ctx, &HashMap::new()),
        AnfOp::AMatch { arms, .. } => {
            let candidate = ctx
                .infer_let_op_mono_for_emit(local, op)
                .filter(is_typed_general_sum_candidate)?;
            let all_typed_sources = arms
                .iter()
                .filter(|arm| !expr_always_diverges(&arm.body))
                .all(|arm| {
                    let mut flow = HashMap::new();
                    expr_typed_general_option_source(&arm.body, ctx, &mut flow).as_ref()
                        == Some(&candidate)
                });
            all_typed_sources.then_some(candidate)
        }
        AnfOp::AInit { value } => atom_typed_general_option(value, ctx),
        AnfOp::AAssign { value, .. } => atom_typed_general_option(value, ctx),
        _ => None,
    }
}

fn atom_typed_closure_sig(atom: &Atom, ctx: &EmitCtx<'_>) -> Option<(Vec<MonoType>, MonoType)> {
    match atom {
        Atom::ALocal(local_id) => ctx.local_typed_closure_sig(*local_id),
        Atom::AGlobalFunc(func_id) => ctx.concrete_func_sig(*func_id).cloned(),
        _ => None,
    }
}

fn typed_cell_info_from_inner_mono(
    inner: &MonoType,
    ctx: &EmitCtx<'_>,
) -> Option<(String, ValType, MonoType)> {
    if ctx.concrete_func_sigs.is_empty() || !is_concrete_mono_type(inner) {
        return None;
    }
    Some((
        typed_cell_struct_sym(inner),
        mono_to_valtype_specialized(inner, ctx.type_env, &ctx.concrete_func_sigs),
        inner.clone(),
    ))
}

fn typed_cell_info_from_atom(
    atom: &Atom,
    ctx: &EmitCtx<'_>,
) -> Option<(String, ValType, MonoType)> {
    if let Atom::ALocal(local_id) = atom {
        return ctx
            .local_typed_cell_elem(*local_id)
            .and_then(|elem_ty| typed_cell_info_from_inner_mono(&elem_ty, ctx));
    }
    let MonoType::Named { type_id, args } = ctx.infer_atom_mono(atom)? else {
        return None;
    };
    if type_id != crate::types::ty::CELL_TYPE_ID || args.len() != 1 {
        return None;
    }
    typed_cell_info_from_inner_mono(&args[0], ctx)
}

fn emit_cell_new_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    if let Some(inner_mono) = ctx.infer_atom_mono(&args[0]) {
        if let Some((cell_sym, payload_ty, _)) = typed_cell_info_from_inner_mono(&inner_mono, ctx) {
            #[cfg(debug_assertions)]
            bump_boundary!(typed_cell_ops);
            let mut instrs = emit_atom(&args[0], Some(&payload_ty), ctx);
            instrs.push(Instr::StructNew(cell_sym.clone()));
            instrs.extend(emit_coerce_stack(
                &ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(cell_sym),
                },
                bind_ty,
            ));
            return instrs;
        }
    }
    let mut instrs = emit_atom(&args[0], Some(&ValType::Anyref), ctx);
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
    instrs
}

fn emit_cell_get_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    if let Some((cell_sym, payload_ty, _)) = typed_cell_info_from_atom(&args[0], ctx) {
        let cell_ref = ValType::Ref {
            nullable: true,
            heap: HeapType::Named(cell_sym.clone()),
        };
        let mut instrs = emit_atom(&args[0], Some(&cell_ref), ctx);
        instrs.push(Instr::StructGet(cell_sym, 0));
        instrs.extend(emit_coerce_stack(&payload_ty, bind_ty));
        return instrs;
    }
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_cell_set_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    if let Some((cell_sym, payload_ty, _)) = typed_cell_info_from_atom(&args[0], ctx) {
        let cell_ref = ValType::Ref {
            nullable: true,
            heap: HeapType::Named(cell_sym.clone()),
        };
        let mut instrs = emit_atom(&args[0], Some(&cell_ref), ctx);
        instrs.extend(emit_atom(&args[1], Some(&payload_ty), ctx));
        instrs.push(Instr::StructSet(cell_sym, 0));
        instrs.extend(emit_void_value(Some(bind_ty)));
        return instrs;
    }
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::ArraySet(T_ARRAY.to_string()));
    instrs.extend(emit_void_value(Some(bind_ty)));
    instrs
}

fn emit_cell_update_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    assert_eq!(args.len(), 2, "Cell.update expects 2 args");

    if let Some((cell_sym, payload_ty, inner_mono)) = typed_cell_info_from_atom(&args[0], ctx) {
        if let Some((params, ret)) = atom_typed_closure_sig(&args[1], ctx) {
            if params.len() == 1 && params[0] == inner_mono && ret == inner_mono {
                let closurefunc_sym = typed_closurefunc_sym(&params, &ret);
                let closure_sym = typed_closure_struct_sym(&params, &ret);
                let closure_ref = ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(closure_sym.clone()),
                };
                let cell_ref = ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(cell_sym.clone()),
                };

                let mut instrs = emit_atom(&args[0], Some(&cell_ref), ctx);
                instrs.extend(emit_atom(&args[1], Some(&closure_ref), ctx));
                instrs.push(Instr::StructGet(closure_sym.clone(), 1));

                instrs.extend(emit_atom(&args[0], Some(&cell_ref), ctx));
                instrs.push(Instr::StructGet(cell_sym.clone(), 0));

                instrs.extend(emit_atom(&args[1], Some(&closure_ref), ctx));
                instrs.push(Instr::StructGet(closure_sym, 2));
                instrs.push(Instr::CallRef(closurefunc_sym));
                instrs.extend(emit_coerce_stack(
                    &mono_to_valtype_specialized(&ret, ctx.type_env, &ctx.concrete_func_sigs),
                    &payload_ty,
                ));

                instrs.push(Instr::StructSet(cell_sym, 0));
                instrs.extend(emit_void_value(Some(bind_ty)));
                return instrs;
            }
        }
    }

    // Universal closure path.
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));

    instrs.extend(emit_atom(&args[1], Some(&ref_closure_null()), ctx));
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 1));

    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));

    instrs.extend(emit_atom(&args[1], Some(&ref_closure_null()), ctx));
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 0));
    instrs.push(Instr::CallRef(T_CLOSURE_FUNC.to_string()));

    instrs.push(Instr::ArraySet(T_ARRAY.to_string()));
    instrs.extend(emit_void_value(Some(bind_ty)));
    instrs
}

// --- Iterator intrinsics ---
// Iterator is represented as a 2-element rt_types__Array: [seed, step_closure]

fn emit_iterator_unfold_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if let Some(info) =
        iterator_state_from_unfold_args(args.first().unwrap(), args.get(1).unwrap(), ctx).filter(
            |info| is_concrete_mono_type(&info.yield_ty) && is_concrete_mono_type(&info.seed_ty),
        )
    {
        let state_sym = typed_iterator_state_sym(&info);
        let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
        let step_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
        let state_ref = ValType::Ref {
            nullable: true,
            heap: HeapType::Named(state_sym.clone()),
        };
        ctx.request_typed_iterator_state(state_sym.clone(), info.clone());
        #[cfg(debug_assertions)]
        bump_boundary!(typed_iterator_ops);
        let mut instrs = emit_atom(
            &args[0],
            Some(&mono_to_valtype_specialized(
                &info.seed_ty,
                ctx.type_env,
                &ctx.concrete_func_sigs,
            )),
            ctx,
        );
        instrs.extend(emit_atom(
            &args[1],
            Some(&ValType::Ref {
                nullable: true,
                heap: HeapType::Named(step_sym),
            }),
            ctx,
        ));
        instrs.push(Instr::StructNew(state_sym));
        instrs.extend(emit_coerce_stack(&state_ref, bind_ty));
        return instrs;
    }

    // Iterator.unfold(seed, step) -> IterState struct { seed: anyref, step: anyref }
    let mut instrs = emit_atom(&args[0], Some(&ValType::Anyref), ctx);
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::StructNew(T_ITER_STATE.to_string()));
    let result_ty = ref_iter_state_null();
    instrs.extend(emit_coerce_stack(&result_ty, bind_ty));
    instrs
}

fn emit_iterator_next_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let mut instrs = if let Some(info) = atom_iterator_state(&args[0], ctx).filter(|info| {
        is_concrete_mono_type(&info.yield_ty) && is_concrete_mono_type(&info.seed_ty)
    }) {
        let state_ref = ValType::Ref {
            nullable: true,
            heap: HeapType::Named(typed_iterator_state_sym(&info)),
        };
        emit_atom(&args[0], Some(&state_ref), ctx)
    } else {
        emit_atom(&args[0], Some(&ref_iter_state_null()), ctx)
    };
    if !ctx.concrete_func_sigs.is_empty() {
        if let Some(info) = atom_iterator_state(&args[0], ctx).filter(|info| {
            is_concrete_mono_type(&info.yield_ty) && is_concrete_mono_type(&info.seed_ty)
        }) {
            let helper_sym = typed_iterator_next_helper_sym(&info);
            // Register both the iterator helper and the typed UnfoldStep struct it needs
            let unfold_sym = typed_unfold_step_sym(&info.yield_ty, &info.seed_ty);
            let state_sym = typed_iterator_state_sym(&info);
            let iter_item_sym = typed_iter_item_sym(&info);
            let iter_option_sym = typed_iter_option_sym(&info);
            ctx.request_typed_iterator_state(state_sym, info.clone());
            ctx.request_typed_iter_item(iter_item_sym, info.clone());
            ctx.request_typed_iter_option(iter_option_sym.clone(), info.clone());
            ctx.request_typed_unfold_step(unfold_sym, info.yield_ty.clone(), info.seed_ty.clone());
            ctx.request_iterator_helper(helper_sym.clone(), info);
            instrs.push(Instr::Call(helper_sym));
            instrs.extend(emit_coerce_stack(
                &ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(iter_option_sym),
                },
                bind_ty,
            ));
            return instrs;
        } else {
            instrs.push(Instr::Call(ITERATOR_NEXT_HELPER.to_string()));
        }
    } else {
        instrs.push(Instr::Call(ITERATOR_NEXT_HELPER.to_string()));
    }
    instrs.extend(emit_coerce_stack(&ref_variant_null(), bind_ty));
    instrs
}

const ITERATOR_NEXT_HELPER: &str = "user____iterator_next";

fn typed_iterator_next_helper_sym(info: &IteratorStateInfo) -> String {
    format!(
        "user____iterator_next__{}__{}",
        mono_to_symbol_key(&info.yield_ty),
        mono_to_symbol_key(&info.seed_ty),
    )
}

/// Emit the `__iterator_next` Wasm helper function.
/// Takes an iterator (IterState struct { seed, step }) and returns Option<IterItem> variant.
fn emit_iterator_next_helper() -> FuncDef {
    // Locals:
    // 0: param it (anyref = IterState ref)
    // 1: step_result (variant ref)
    // 2: variant_id (i32)
    // 3: payload / temp (anyref)
    // 4: it_state (ref null $IterState = cast of param 0)

    let mut body = Vec::new();

    // Cast param 0 to IterState ref, store in local 4
    body.push(Instr::LocalGet(0));
    body.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ITER_STATE.to_string()),
    });
    body.push(Instr::LocalSet(4));

    // --- Call step(seed) ---

    // Push closure env
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(T_ITER_STATE.to_string(), 1)); // step
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(T_CLOSURE.to_string()),
    });
    body.push(Instr::StructGet(T_CLOSURE.to_string(), 1)); // env

    // Push args array (containing seed)
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(T_ITER_STATE.to_string(), 0)); // seed
    body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));

    // Push func_ref from step closure
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(T_ITER_STATE.to_string(), 1)); // step
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(T_CLOSURE.to_string()),
    });
    body.push(Instr::StructGet(T_CLOSURE.to_string(), 0)); // func_ref

    // Call step closure
    body.push(Instr::CallRef(T_CLOSURE_FUNC.to_string()));

    // Cast result to Variant
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(T_VARIANT.to_string()),
    });
    body.push(Instr::LocalSet(1)); // step_result

    // Extract variant_id
    body.push(Instr::LocalGet(1));
    body.push(Instr::StructGet(T_VARIANT.to_string(), 1)); // variant_id field
    body.push(Instr::LocalSet(2));

    // --- Branch on variant_id ---
    // If variant_id == 0 (Done): return Option.None
    // Else (Yield): construct IterItem and return Option.Some(item)

    body.push(Instr::LocalGet(2));
    body.push(Instr::I32Eqz); // variant_id == 0?

    body.push(Instr::If {
        result: Some(ref_variant_null()),
        then_body: {
            // Done -> return Option.None = Variant(OPTION_TYPE_ID, 0, [])
            vec![
                Instr::I32Const(OPTION_TYPE_ID.0 as i32),
                Instr::I32Const(0),                           // None variant
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 0), // empty payload
                Instr::StructNew(T_VARIANT.to_string()),
            ]
        },
        else_body: {
            // Yield(value, next_seed) -> Option.Some(IterItem { value, rest: next_iter })

            // Extract UnfoldStep payload
            let iter_item_sym = user_record_type_sym(ITER_ITEM_TYPE_ID);
            let mut else_instrs = vec![
                Instr::LocalGet(1),
                Instr::StructGet(T_VARIANT.to_string(), 2),
                Instr::LocalSet(3), // payload array
            ];

            // Field 0: value = payload[0]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.to_string()),
            });
            else_instrs.push(Instr::I32Const(0));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));

            // Field 1: rest iterator = IterState { next_seed, step }
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.to_string()),
            });
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            // step = original IterState field 1
            else_instrs.push(Instr::LocalGet(4));
            else_instrs.push(Instr::StructGet(T_ITER_STATE.to_string(), 1));
            else_instrs.push(Instr::StructNew(T_ITER_STATE.to_string()));

            else_instrs.push(Instr::StructNew(iter_item_sym));
            // IterItem ref on stack. Store temporarily.
            else_instrs.push(Instr::LocalSet(3));

            // --- Build Option.Some(iter_item) ---
            // Variant(OPTION_TYPE_ID, 1, [iter_item])
            else_instrs.push(Instr::I32Const(OPTION_TYPE_ID.0 as i32));
            else_instrs.push(Instr::I32Const(1)); // Some
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
            else_instrs.push(Instr::StructNew(T_VARIANT.to_string()));

            else_instrs
        },
    });

    body.push(Instr::Return);

    FuncDef {
        name: ITERATOR_NEXT_HELPER.to_string(),
        params: vec![ValType::Anyref],     // IterState ref
        results: vec![ref_variant_null()], // Option variant ref
        locals: vec![
            ref_variant_null(),    // local 1: step_result variant
            ValType::I32,          // local 2: variant_id
            ValType::Anyref,       // local 3: payload / temp
            ref_iter_state_null(), // local 4: it_state (cast of param 0)
        ],
        body,
    }
}

fn emit_typed_iterator_next_helper(
    info: &IteratorStateInfo,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> FuncDef {
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let iter_state_sym = typed_iterator_state_sym(info);
    let closure_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let closurefunc_sym = typed_closurefunc_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    let unfold_sym = typed_unfold_step_sym(&info.yield_ty, &info.seed_ty);
    let iter_item_sym = typed_iter_item_sym(info);
    let iter_option_sym = typed_iter_option_sym(info);
    let yield_ty = mono_to_valtype_specialized(&info.yield_ty, type_env, concrete_func_sigs);
    let seed_ty = mono_to_valtype_specialized(&info.seed_ty, type_env, concrete_func_sigs);
    let iter_state_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(iter_state_sym.clone()),
    };
    let iter_item_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(iter_item_sym.clone()),
    };
    let iter_option_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(iter_option_sym.clone()),
    };
    let unfold_step_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(unfold_sym.clone()),
    };

    let mut body = Vec::new();

    // Cast param 0 to IterState, store in local 4
    body.push(Instr::LocalGet(0));
    body.push(Instr::LocalSet(4));

    // Push closure env from step (IterState field 1)
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(iter_state_sym.clone(), 1)); // step
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(closure_sym.clone()),
    });
    body.push(Instr::StructGet(closure_sym.clone(), 1)); // env

    // Push seed (IterState field 0), coerce to concrete type
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(iter_state_sym.clone(), 0)); // seed

    // Push typed funcref from step closure (IterState field 1)
    body.push(Instr::LocalGet(4));
    body.push(Instr::StructGet(iter_state_sym.clone(), 1)); // step
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(closure_sym.clone()),
    });
    body.push(Instr::StructGet(closure_sym, 2)); // typed funcref
    body.push(Instr::CallRef(closurefunc_sym));

    // Boundary conversion: erased Variant -> typed UnfoldStep
    body.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_VARIANT.to_string()),
    });
    body.push(Instr::LocalSet(5));

    let mut done_step = vec![Instr::I32Const(0)];
    done_step.extend(emit_default_value_instrs(&yield_ty));
    done_step.extend(emit_default_value_instrs(&seed_ty));
    done_step.push(Instr::StructNew(unfold_sym.clone()));

    let mut yield_step = vec![
        Instr::I32Const(1),
        Instr::LocalGet(5),
        Instr::StructGet(T_VARIANT.to_string(), 2),
        Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.to_string()),
        },
        Instr::I32Const(0),
        Instr::ArrayGet(T_ARRAY.to_string()),
    ];
    yield_step.extend(emit_coerce_stack(&ValType::Anyref, &yield_ty));
    yield_step.push(Instr::LocalGet(5));
    yield_step.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    yield_step.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });
    yield_step.push(Instr::I32Const(1));
    yield_step.push(Instr::ArrayGet(T_ARRAY.to_string()));
    yield_step.extend(emit_coerce_stack(&ValType::Anyref, &seed_ty));
    yield_step.push(Instr::StructNew(unfold_sym.clone()));

    body.push(Instr::LocalGet(5));
    body.push(Instr::StructGet(T_VARIANT.to_string(), 1));
    body.push(Instr::I32Eqz);
    body.push(Instr::If {
        result: Some(unfold_step_ref.clone()),
        then_body: done_step,
        else_body: yield_step,
    });
    body.push(Instr::LocalSet(1));

    // Read variant_id from typed struct field 0
    body.push(Instr::LocalGet(1));
    body.push(Instr::StructGet(unfold_sym.clone(), 0));
    body.push(Instr::LocalSet(2));

    body.push(Instr::LocalGet(2));
    body.push(Instr::I32Eqz);
    body.push(Instr::If {
        result: Some(iter_option_ref.clone()),
        then_body: vec![
            Instr::I32Const(0),
            Instr::RefNull(HeapType::Named(iter_item_sym.clone())),
            Instr::StructNew(iter_option_sym.clone()),
        ],
        else_body: {
            let mut else_instrs = vec![
                // IterItem field 0: value = typed struct field 1 (yield value)
                Instr::LocalGet(1),
                Instr::StructGet(unfold_sym.clone(), 1),
            ];
            // IterItem field 1: rest = IterState { next_seed, step }
            // next_seed = typed struct field 2
            else_instrs.push(Instr::LocalGet(1));
            else_instrs.push(Instr::StructGet(unfold_sym, 2));
            // step = original IterState field 1
            else_instrs.push(Instr::LocalGet(4));
            else_instrs.push(Instr::StructGet(iter_state_sym.clone(), 1));
            else_instrs.push(Instr::StructNew(iter_state_sym.clone()));
            else_instrs.push(Instr::StructNew(iter_item_sym));
            else_instrs.push(Instr::LocalSet(3));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::StructNew(iter_option_sym.clone()));
            else_instrs
        },
    });
    body.push(Instr::Return);

    FuncDef {
        name: typed_iterator_next_helper_sym(info),
        params: vec![iter_state_ref.clone()],
        results: vec![iter_option_ref.clone()],
        locals: vec![
            unfold_step_ref,    // local 1: typed step_result
            ValType::I32,       // local 2: variant_id
            iter_item_ref,      // local 3: temp item
            iter_state_ref,     // local 4: it_state
            ref_variant_null(), // local 5: erased step_result
        ],
        body,
    }
}

fn emit_string_get_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    use crate::types::ty::OPTION_TYPE_ID;
    assert_eq!(args.len(), 2, "String.get expects 2 args");

    let mut instrs = Vec::new();

    // condition: 0 <= i < len(s) in full Int domain.
    instrs.extend(emit_string_index_in_bounds(&args[0], &args[1], ctx));

    // then: Some(byte) — read byte at offset, box as ref.i31
    let mut then_body = vec![Instr::I32Const(OPTION_TYPE_ID.0 as i32), Instr::I32Const(1)];
    // Read the byte via ArrayGetU
    then_body.extend(emit_atom(&args[0], Some(&ref_string_null()), ctx));
    then_body.push(Instr::RefAsNonNull);
    then_body.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    then_body.push(Instr::I32WrapI64);
    then_body.push(Instr::ArrayGetU(T_STRING.to_string()));
    // Box i32 byte value as ref.i31 → anyref for the variant payload
    then_body.push(Instr::RefI31);
    then_body.extend(emit_coerce_stack(&ValType::I31ref, &ValType::Anyref));
    then_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    then_body.push(Instr::StructNew(T_VARIANT.to_string()));

    let else_body = vec![
        Instr::I32Const(OPTION_TYPE_ID.0 as i32),
        Instr::I32Const(0), // None
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        Instr::StructNew(T_VARIANT.to_string()),
    ];

    instrs.push(Instr::If {
        result: Some(ref_variant()),
        then_body,
        else_body,
    });
    instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
    instrs
}

fn emit_string_slice_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // String.slice(s: String, start: Int, end: Int) -> String
    // Byte-offset slice with UTF-8 boundary validation.
    // Traps if: OOB, start > end, or start/end not on scalar boundary.
    //
    // Implementation: validate in Wasm, then delegate to rt_str__substring.
    // A UTF-8 continuation byte has bits 10xxxxxx, i.e. (byte & 0xC0) == 0x80.
    // A scalar boundary is: offset == 0 || offset == len || (byte & 0xC0) != 0x80.
    //
    // Atoms are re-emitted freely (they're just local.get or const — cheap).
    ensure_rt_str_substring_import(ctx);

    // Helper closures to emit start/end as i64/i32.
    let emit_start_i64 =
        |ctx: &mut EmitCtx<'_>| -> Vec<Instr> { emit_atom(&args[1], Some(&ValType::I64), ctx) };
    let emit_end_i64 =
        |ctx: &mut EmitCtx<'_>| -> Vec<Instr> { emit_atom(&args[2], Some(&ValType::I64), ctx) };
    let emit_start_i32 = |ctx: &mut EmitCtx<'_>| -> Vec<Instr> {
        let mut v = emit_start_i64(ctx);
        v.push(Instr::I32WrapI64);
        v
    };
    let emit_end_i32 = |ctx: &mut EmitCtx<'_>| -> Vec<Instr> {
        let mut v = emit_end_i64(ctx);
        v.push(Instr::I32WrapI64);
        v
    };
    let emit_str = |ctx: &mut EmitCtx<'_>| -> Vec<Instr> {
        emit_atom(&args[0], Some(&ref_string_null()), ctx)
    };

    let mut instrs = Vec::new();

    // Bounds check in full Int (i64) domain:
    // start >= 0 && end >= 0 && start <= end && end <= len.
    // This avoids i64->i32 wraparound accepting large invalid indices.
    instrs.extend(emit_start_i64(ctx));
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS);
    instrs.extend(emit_end_i64(ctx));
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS);
    instrs.push(Instr::I32And);
    instrs.extend(emit_start_i64(ctx));
    instrs.extend(emit_end_i64(ctx));
    instrs.push(Instr::I64LeS);
    instrs.push(Instr::I32And);
    instrs.extend(emit_end_i64(ctx));
    instrs.extend(emit_str(ctx));
    instrs.push(Instr::ArrayLen);
    instrs.push(Instr::I64ExtendI32U);
    instrs.push(Instr::I64LeS);
    instrs.push(Instr::I32And);
    instrs.push(Instr::If {
        result: None,
        then_body: vec![],
        else_body: vec![Instr::Unreachable],
    });

    // UTF-8 boundary check for start:
    // if start > 0 && start < len: check (s[start] & 0xC0) != 0x80
    // (start == len is a valid boundary — one-past-end)
    instrs.extend(emit_start_i32(ctx));
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::I32GtU);
    instrs.extend(emit_start_i32(ctx));
    instrs.extend(emit_str(ctx));
    instrs.push(Instr::ArrayLen);
    instrs.push(Instr::I32LtU);
    instrs.push(Instr::I32And);
    instrs.push(Instr::If {
        result: None,
        then_body: {
            let mut body = emit_str(ctx);
            body.push(Instr::RefAsNonNull);
            body.extend(emit_start_i32(ctx));
            body.push(Instr::ArrayGetU(T_STRING.to_string()));
            body.push(Instr::I32Const(0xC0));
            body.push(Instr::I32And);
            body.push(Instr::I32Const(0x80));
            body.push(Instr::I32Eq);
            body.push(Instr::If {
                result: None,
                then_body: vec![Instr::Unreachable],
                else_body: vec![],
            });
            body
        },
        else_body: vec![],
    });

    // UTF-8 boundary check for end:
    // if end < len: check (s[end] & 0xC0) != 0x80
    instrs.extend(emit_end_i32(ctx));
    instrs.extend(emit_str(ctx));
    instrs.push(Instr::ArrayLen);
    instrs.push(Instr::I32LtU);
    instrs.push(Instr::If {
        result: None,
        then_body: {
            let mut body = emit_str(ctx);
            body.push(Instr::RefAsNonNull);
            body.extend(emit_end_i32(ctx));
            body.push(Instr::ArrayGetU(T_STRING.to_string()));
            body.push(Instr::I32Const(0xC0));
            body.push(Instr::I32And);
            body.push(Instr::I32Const(0x80));
            body.push(Instr::I32Eq);
            body.push(Instr::If {
                result: None,
                then_body: vec![Instr::Unreachable],
                else_body: vec![],
            });
            body
        },
        else_body: vec![],
    });

    // All checks passed — call rt_str__substring(s, start, end)
    instrs.extend(emit_str(ctx));
    instrs.extend(emit_start_i32(ctx));
    instrs.extend(emit_end_i32(ctx));
    instrs.push(Instr::Call("rt_str__substring".to_string()));
    instrs.extend(emit_coerce_stack(&ref_string(), bind_ty));
    instrs
}

fn emit_char_code_at_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // String.char_code_at(s: String, i: Int) -> Int
    // Read byte from string array, zero-extend to i64
    let mut instrs = emit_trap_unless(emit_string_index_in_bounds(&args[0], &args[1], ctx));
    instrs.extend(emit_atom(&args[0], Some(&ref_string_null()), ctx));
    instrs.push(Instr::RefAsNonNull);
    instrs.extend(emit_atom(&args[1], Some(&ValType::I64), ctx));
    instrs.push(Instr::I32WrapI64);
    instrs.push(Instr::ArrayGetU(T_STRING.to_string()));
    instrs.push(Instr::I64ExtendI32U);
    instrs.extend(emit_coerce_stack(&ValType::I64, bind_ty));
    instrs
}

fn emit_from_char_code_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // String.from_char_code(n: Int) -> Option<String>
    // For single-byte values (0-127 ASCII), create a 1-byte string.
    // Values outside 0-127 → None (full Unicode support via host in future).
    let mut instrs = emit_atom(&args[0], Some(&ValType::I64), ctx);
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS); // code >= 0
    instrs.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
    instrs.push(Instr::I64Const(128));
    instrs.push(Instr::I64LtS); // code < 128
    instrs.push(Instr::I32And);
    instrs.push(Instr::If {
        result: Some(ValType::Anyref),
        then_body: {
            // Some(single-byte string)
            let mut v = vec![
                Instr::I32Const(0), // type_id (Option)
                Instr::I32Const(1), // variant_id (Some)
            ];
            v.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
            v.push(Instr::I32WrapI64);
            v.push(Instr::ArrayNewFixed(T_STRING.to_string(), 1));
            v.extend(emit_coerce_stack(&ref_string(), &ValType::Anyref));
            v.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
            v.push(Instr::StructNew(T_VARIANT.to_string()));
            v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
            v
        },
        else_body: {
            // None
            let mut v = vec![
                Instr::I32Const(0), // type_id
                Instr::I32Const(0), // variant_id (None)
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
                Instr::StructNew(T_VARIANT.to_string()),
            ];
            v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
            v
        },
    });
    instrs
}

fn emit_from_code_point_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    use crate::types::ty::OPTION_TYPE_ID;

    // String.from_code_point(n: Int) -> Option<String>
    // Validates the code point and encodes as UTF-8 (1-4 bytes).
    // Returns None for surrogates (0xD800..0xDFFF) and values > 0x10FFFF.
    //
    // Strategy: use a helper function emitted into the module to keep
    // the inline code manageable. The helper takes an i32 code point
    // and returns (ref null $Variant).
    // Guard the full Int range before narrowing to i32 so values outside
    // the Unicode range cannot wrap into valid code points.
    let mut instrs = emit_atom(&args[0], Some(&ValType::I64), ctx);
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS);
    instrs.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
    instrs.push(Instr::I64Const(0x10FFFF));
    instrs.push(Instr::I64LeS);
    instrs.push(Instr::I32And);
    instrs.push(Instr::If {
        result: Some(ValType::Anyref),
        then_body: {
            let mut body = emit_atom(&args[0], Some(&ValType::I64), ctx);
            body.push(Instr::I32WrapI64);
            body.push(Instr::Call("$from_code_point_helper".to_string()));
            body
        },
        else_body: {
            let mut body = vec![
                Instr::I32Const(OPTION_TYPE_ID.0 as i32),
                Instr::I32Const(0),
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
                Instr::StructNew(T_VARIANT.to_string()),
            ];
            body.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
            body
        },
    });
    instrs
}

fn emit_string_utf8_bytes_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // String.utf8_bytes(s: String) -> Vector<Byte>
    // Helper still builds a flat $Array; convert at the boundary.
    let mut instrs = emit_atom(&args[0], Some(&ref_string_null()), ctx);
    instrs.push(Instr::Call("$string_utf8_bytes_helper".to_string()));
    ensure_rt_arr_from_array_import(ctx);
    instrs.push(Instr::Call("rt_arr__from_array".to_string()));
    instrs
}

fn emit_string_from_utf8_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // String.from_utf8(bytes: Vector<Byte>) -> Option<String>
    // Helper still expects a flat byte $Array; convert at the boundary.
    let mut instrs = emit_atom(&args[0], Some(&ref_pvec_null()), ctx);
    ensure_rt_arr_to_array_import(ctx);
    instrs.push(Instr::Call("rt_arr__to_array".to_string()));
    instrs.push(Instr::Call("$string_from_utf8_helper".to_string()));
    instrs
}

fn emit_int_from_string_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let mut instrs = emit_atom(&args[0], Some(&ref_string_null()), ctx);
    instrs.push(Instr::Call("$int_from_string_helper".to_string()));
    instrs
}

fn emit_float_from_string_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    ensure_host_parse_float_import(ctx);
    let mut instrs = emit_atom(&args[0], Some(&ref_string_null()), ctx);
    instrs.push(Instr::Call("$float_from_string_helper".to_string()));
    instrs
}

fn ensure_host_parse_float_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "host".to_string(),
        name: "parse_float".to_string(),
        as_sym: "host_parse_float".to_string(),
        params: vec![ref_string_null()],
        results: vec![ValType::F64, ValType::I32],
    });
}

/// Generate helper function: $int_from_string_helper
/// Parses an integer from a string (pure Wasm, no host call).
/// Takes (ref null $String) → anyref (Option<Int> variant)
///
/// Locals layout:
///   param 0: ref null $String (input)
///   local 1: i64 (accumulator)
///   local 2: i32 (index)
///   local 3: i32 (len)
///   local 4: i64 (sign: 1 or -1)
///   local 5: i32 (byte)
///   local 6: i32 (ok flag)
fn emit_int_from_string_helper() -> FuncDef {
    // param 0: ref null $String (input)
    // local 1: i64 (accumulator)
    // local 2: i32 (index)
    // local 3: i32 (len)
    // local 4: i64 (sign: 1 or -1)
    // local 5: i32 (byte)
    // local 6: i32 (ok flag, 1=success, 0=failure)
    let done_label = "$done".to_string();
    let loop_label = "$digit_loop".to_string();

    let body = vec![
        // sign = 1, ok = 1
        Instr::I64Const(1),
        Instr::LocalSet(4),
        Instr::I32Const(1),
        Instr::LocalSet(6),
        // len = array.len(s)
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::ArrayLen,
        Instr::LocalSet(3),
        // if len == 0: ok = 0, skip to end
        Instr::LocalGet(3),
        Instr::I32Eqz,
        Instr::If {
            result: None,
            then_body: vec![Instr::I32Const(0), Instr::LocalSet(6)],
            else_body: vec![
                // Check first byte for sign
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::I32Const(0),
                Instr::ArrayGetU(T_STRING.to_string()),
                Instr::LocalSet(5),
                // if byte == 45 ('-')
                Instr::LocalGet(5),
                Instr::I32Const(45),
                Instr::I32Eq,
                Instr::If {
                    result: None,
                    then_body: vec![
                        Instr::I64Const(-1),
                        Instr::LocalSet(4),
                        Instr::I32Const(1),
                        Instr::LocalSet(2),
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Eq,
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::I32Const(0),
                                Instr::LocalSet(6), // just "-" → fail
                            ],
                            else_body: vec![],
                        },
                    ],
                    else_body: vec![
                        // Check for '+'
                        Instr::LocalGet(5),
                        Instr::I32Const(43),
                        Instr::I32Eq,
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::I32Const(1),
                                Instr::LocalSet(2),
                                Instr::LocalGet(3),
                                Instr::I32Const(1),
                                Instr::I32Eq,
                                Instr::If {
                                    result: None,
                                    then_body: vec![Instr::I32Const(0), Instr::LocalSet(6)],
                                    else_body: vec![],
                                },
                            ],
                            else_body: vec![Instr::I32Const(0), Instr::LocalSet(2)],
                        },
                    ],
                },
                // Digit loop (only if ok still 1)
                Instr::LocalGet(6),
                Instr::If {
                    result: None,
                    then_body: vec![Instr::Block {
                        label: done_label.clone(),
                        result: None,
                        body: vec![Instr::Loop {
                            label: loop_label.clone(),
                            result: None,
                            body: vec![
                                // if i >= len: break
                                Instr::LocalGet(2),
                                Instr::LocalGet(3),
                                Instr::I32GeS,
                                Instr::BrIf(done_label.clone()),
                                // byte = s[i]
                                Instr::LocalGet(0),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(2),
                                Instr::ArrayGetU(T_STRING.to_string()),
                                Instr::LocalSet(5),
                                // if byte < 48 or byte > 57: ok = 0, break
                                Instr::LocalGet(5),
                                Instr::I32Const(48),
                                Instr::I32LtS,
                                Instr::LocalGet(5),
                                Instr::I32Const(57),
                                Instr::I32GtS,
                                Instr::I32Or,
                                Instr::If {
                                    result: None,
                                    then_body: vec![
                                        Instr::I32Const(0),
                                        Instr::LocalSet(6),
                                        Instr::Br(done_label.clone()),
                                    ],
                                    else_body: vec![],
                                },
                                // value = value * 10 + (byte - 48)
                                Instr::LocalGet(1),
                                Instr::I64Const(10),
                                Instr::I64Mul,
                                Instr::LocalGet(5),
                                Instr::I32Const(48),
                                Instr::I32Sub,
                                Instr::I64ExtendI32U,
                                Instr::I64Add,
                                Instr::LocalSet(1),
                                // i++
                                Instr::LocalGet(2),
                                Instr::I32Const(1),
                                Instr::I32Add,
                                Instr::LocalSet(2),
                                Instr::Br(loop_label.clone()),
                            ],
                        }],
                    }],
                    else_body: vec![],
                },
            ],
        },
        // Check ok flag → build Some or None
        Instr::LocalGet(6),
        Instr::If {
            result: Some(ValType::Anyref),
            then_body: {
                let mut v = vec![
                    Instr::I32Const(0),
                    Instr::I32Const(1),
                    Instr::LocalGet(1),
                    Instr::LocalGet(4),
                    Instr::I64Mul,
                    Instr::StructNew(T_BOXED_INT.to_string()),
                ];
                v.extend(emit_coerce_stack(
                    &ValType::Ref {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_INT.to_string()),
                    },
                    &ValType::Anyref,
                ));
                v.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
                v.push(Instr::StructNew(T_VARIANT.to_string()));
                v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
                v
            },
            else_body: {
                let mut v = vec![
                    Instr::I32Const(0),
                    Instr::I32Const(0),
                    Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
                    Instr::StructNew(T_VARIANT.to_string()),
                ];
                v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
                v
            },
        },
    ];

    FuncDef {
        name: "$int_from_string_helper".to_string(),
        params: vec![ref_string_null()],
        results: vec![ValType::Anyref],
        locals: vec![
            ValType::I64, // 1: accumulator
            ValType::I32, // 2: index
            ValType::I32, // 3: len
            ValType::I64, // 4: sign
            ValType::I32, // 5: byte
            ValType::I32, // 6: ok flag
        ],
        body,
    }
}

/// Generate helper function: $from_code_point_helper
/// Takes i32 (code point) → anyref (Option<String> variant)
/// Validates the code point and encodes as 1-4 byte UTF-8 string.
/// Returns None for surrogates (0xD800..0xDFFF) and values > 0x10FFFF.
fn emit_from_code_point_helper() -> FuncDef {
    // param 0: i32 (code point)
    //
    // Helper to build None variant
    let mk_none = || -> Vec<Instr> {
        let mut v = vec![
            Instr::I32Const(0), // type_id (Option)
            Instr::I32Const(0), // variant_id (None)
            Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
            Instr::StructNew(T_VARIANT.to_string()),
        ];
        v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
        v
    };

    // Each mk_Nbyte sequence pushes: type_id(0), variant_id(1), then the string ref.
    // wrap_some() finishes it: cast string→anyref, wrap in fields array, StructNew, cast.

    // Build body that produces a 1-byte string (ASCII: 0..0x7F)
    let mk_1byte = vec![
        Instr::I32Const(0),
        Instr::I32Const(1),
        Instr::LocalGet(0), // code point (already fits in i8)
        Instr::ArrayNewFixed(T_STRING.to_string(), 1),
    ];

    // Build body that produces a 2-byte string (0x80..0x7FF)
    let mk_2byte = vec![
        Instr::I32Const(0),
        Instr::I32Const(1),
        // byte 0: (cp >> 6) | 0xC0
        Instr::LocalGet(0),
        Instr::I32Const(6),
        Instr::I32ShrU,
        Instr::I32Const(0xC0),
        Instr::I32Or,
        // byte 1: (cp & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        Instr::ArrayNewFixed(T_STRING.to_string(), 2),
    ];

    // Build body that produces a 3-byte string (0x800..0xFFFF, excluding surrogates)
    let mk_3byte = vec![
        Instr::I32Const(0),
        Instr::I32Const(1),
        // byte 0: (cp >> 12) | 0xE0
        Instr::LocalGet(0),
        Instr::I32Const(12),
        Instr::I32ShrU,
        Instr::I32Const(0xE0),
        Instr::I32Or,
        // byte 1: ((cp >> 6) & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(6),
        Instr::I32ShrU,
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        // byte 2: (cp & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        Instr::ArrayNewFixed(T_STRING.to_string(), 3),
    ];

    // Build body that produces a 4-byte string (0x10000..0x10FFFF)
    let mk_4byte = vec![
        Instr::I32Const(0),
        Instr::I32Const(1),
        // byte 0: (cp >> 18) | 0xF0
        Instr::LocalGet(0),
        Instr::I32Const(18),
        Instr::I32ShrU,
        Instr::I32Const(0xF0),
        Instr::I32Or,
        // byte 1: ((cp >> 12) & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(12),
        Instr::I32ShrU,
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        // byte 2: ((cp >> 6) & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(6),
        Instr::I32ShrU,
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        // byte 3: (cp & 0x3F) | 0x80
        Instr::LocalGet(0),
        Instr::I32Const(0x3F),
        Instr::I32And,
        Instr::I32Const(0x80),
        Instr::I32Or,
        Instr::ArrayNewFixed(T_STRING.to_string(), 4),
    ];

    // After ArrayNewFixed, the string ref is on stack. We need to:
    // cast to anyref, wrap in fields array, StructNew, cast to anyref
    let wrap_some = |mut prefix: Vec<Instr>| -> Vec<Instr> {
        prefix.extend(emit_coerce_stack(&ref_string(), &ValType::Anyref));
        prefix.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
        prefix.push(Instr::StructNew(T_VARIANT.to_string()));
        prefix.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
        prefix
    };

    let body = vec![
        // Validate: cp < 0 → None
        Instr::LocalGet(0),
        Instr::I32Const(0),
        Instr::I32LtS,
        Instr::If {
            result: Some(ValType::Anyref),
            then_body: mk_none(),
            else_body: vec![
                // cp < 0x80 → 1-byte
                Instr::LocalGet(0),
                Instr::I32Const(0x80),
                Instr::I32LtU,
                Instr::If {
                    result: Some(ValType::Anyref),
                    then_body: wrap_some(mk_1byte),
                    else_body: vec![
                        // cp < 0x800 → 2-byte
                        Instr::LocalGet(0),
                        Instr::I32Const(0x800),
                        Instr::I32LtU,
                        Instr::If {
                            result: Some(ValType::Anyref),
                            then_body: wrap_some(mk_2byte),
                            else_body: vec![
                                // Check surrogates: 0xD800 <= cp <= 0xDFFF → None
                                Instr::LocalGet(0),
                                Instr::I32Const(0xD800_u32 as i32),
                                Instr::I32GeU,
                                Instr::LocalGet(0),
                                Instr::I32Const(0xDFFF_u32 as i32),
                                Instr::I32LeU,
                                Instr::I32And,
                                Instr::If {
                                    result: Some(ValType::Anyref),
                                    then_body: mk_none(),
                                    else_body: vec![
                                        // cp <= 0xFFFF → 3-byte
                                        Instr::LocalGet(0),
                                        Instr::I32Const(0xFFFF_u32 as i32),
                                        Instr::I32LeU,
                                        Instr::If {
                                            result: Some(ValType::Anyref),
                                            then_body: wrap_some(mk_3byte),
                                            else_body: vec![
                                                // cp <= 0x10FFFF → 4-byte
                                                Instr::LocalGet(0),
                                                Instr::I32Const(0x10FFFF),
                                                Instr::I32LeU,
                                                Instr::If {
                                                    result: Some(ValType::Anyref),
                                                    then_body: wrap_some(mk_4byte),
                                                    else_body: mk_none(),
                                                },
                                            ],
                                        },
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        },
    ];

    FuncDef {
        name: "$from_code_point_helper".to_string(),
        params: vec![ValType::I32],
        results: vec![ValType::Anyref],
        locals: vec![],
        body,
    }
}

/// Generate helper function: $string_utf8_bytes_helper
/// Takes (ref null $String) → (ref $Array)
/// Copies each byte of the string into a $Array (Vector<Byte>) with i31-boxed values.
fn emit_string_utf8_bytes_helper() -> FuncDef {
    use crate::runtime::types::{T_ARRAY, T_STRING};

    // param 0: (ref null $String) — the input string
    // local 1: i32 — len
    // local 2: i32 — idx (loop counter)
    // local 3: (ref $Array) — result array

    let body = vec![
        // local 1 = array.len(param 0)
        Instr::LocalGet(0),
        Instr::ArrayLen,
        Instr::LocalSet(1),
        // local 3 = array.new_default $Array (len)
        Instr::LocalGet(1),
        Instr::ArrayNewDefault(T_ARRAY.to_string()),
        Instr::LocalSet(3),
        // local 2 = 0
        Instr::I32Const(0),
        Instr::LocalSet(2),
        // loop: copy each byte
        Instr::Block {
            label: "$break".to_string(),
            result: None,
            body: vec![Instr::Loop {
                label: "$continue".to_string(),
                result: None,
                body: vec![
                    // if idx >= len, break
                    Instr::LocalGet(2),
                    Instr::LocalGet(1),
                    Instr::I32GeU,
                    Instr::BrIf("$break".to_string()),
                    // result[idx] = ref.i31(array.get_u $String (str, idx))
                    Instr::LocalGet(3),
                    Instr::LocalGet(2),
                    Instr::LocalGet(0),
                    Instr::LocalGet(2),
                    Instr::ArrayGetU(T_STRING.to_string()),
                    Instr::RefI31,
                    Instr::ArraySet(T_ARRAY.to_string()),
                    // idx += 1
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalSet(2),
                    Instr::Br("$continue".to_string()),
                ],
            }],
        },
        // return the result array
        Instr::LocalGet(3),
    ];

    FuncDef {
        name: "$string_utf8_bytes_helper".to_string(),
        params: vec![ref_string_null()],
        results: vec![ref_array()],
        locals: vec![ValType::I32, ValType::I32, ref_array()],
        body,
    }
}

/// Generate helper function: $string_from_utf8_helper
/// Takes (ref null $Array) → anyref (Option<String> variant)
/// Validates UTF-8 and copies bytes into a $String array.
fn emit_string_from_utf8_helper() -> FuncDef {
    use crate::runtime::types::{T_ARRAY, T_STRING, T_VARIANT};

    // param 0: (ref null $Array) — input Vector<Byte>
    // local 1: i32 — len
    // local 2: i32 — idx (validation + copy loop counter)
    // local 3: (ref null $String) — result string (allocated after validation)
    // local 4: i32 — current byte value

    let mk_none = || -> Vec<Instr> {
        let mut v = vec![
            Instr::I32Const(0), // type_id (Option)
            Instr::I32Const(0), // variant_id (None)
            Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
            Instr::StructNew(T_VARIANT.to_string()),
        ];
        v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
        v
    };

    // UTF-8 validation: walk through bytes checking lead byte patterns.
    // For each lead byte, verify the correct number of continuation bytes follow.
    // A continuation byte is 0x80..0xBF (top 2 bits = 10).

    // Strategy:
    // 1. Validation loop: walk the bytes checking UTF-8 structure
    // 2. If valid, copy all bytes into a new $String and return Some
    // 3. If invalid, return None
    //
    // We use a two-pass approach:
    //   Pass 1: validate UTF-8 (sets a flag if invalid)
    //   Pass 2: copy bytes into $String (only if valid)
    //
    // Actually, to keep it simpler, we do a single validation pass.
    // If it passes, we do a copy pass. This is O(2n) but straightforward.

    // local 5: i32 — valid flag (1 = valid so far)
    // local 6: i32 — expected continuation bytes remaining

    let body = vec![
        // len = array.len(param 0)
        Instr::LocalGet(0),
        Instr::ArrayLen,
        Instr::LocalSet(1),
        // valid = 1
        Instr::I32Const(1),
        Instr::LocalSet(5),
        // idx = 0
        Instr::I32Const(0),
        Instr::LocalSet(2),
        // Validation loop
        Instr::Block {
            label: "$vbreak".to_string(),
            result: None,
            body: vec![Instr::Loop {
                label: "$vcont".to_string(),
                result: None,
                body: vec![
                    // if idx >= len, break
                    Instr::LocalGet(2),
                    Instr::LocalGet(1),
                    Instr::I32GeU,
                    Instr::BrIf("$vbreak".to_string()),
                    // byte = i31.get_u(ref.cast (ref i31) (array.get $Array (bytes, idx)))
                    Instr::LocalGet(0),
                    Instr::LocalGet(2),
                    Instr::ArrayGet(T_ARRAY.to_string()),
                    Instr::RefCast {
                        nullable: false,
                        heap: crate::wasm::ir::HeapType::I31,
                    },
                    Instr::I31GetU,
                    Instr::LocalSet(4),
                    // Determine expected byte length from lead byte
                    Instr::LocalGet(4),
                    Instr::I32Const(0x80),
                    Instr::I32LtU,
                    Instr::If {
                        result: None,
                        // ASCII: 0x00..0x7F — single byte, advance by 1
                        then_body: vec![
                            Instr::LocalGet(2),
                            Instr::I32Const(1),
                            Instr::I32Add,
                            Instr::LocalSet(2),
                        ],
                        else_body: vec![
                            // Check 2-byte lead: 0xC0..0xDF
                            Instr::LocalGet(4),
                            Instr::I32Const(0xC0),
                            Instr::I32GeU,
                            Instr::LocalGet(4),
                            Instr::I32Const(0xDF),
                            Instr::I32LeU,
                            Instr::I32And,
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    // Need 1 continuation byte; also reject overlong (< 0xC2)
                                    Instr::LocalGet(4),
                                    Instr::I32Const(0xC2),
                                    Instr::I32LtU,
                                    Instr::If {
                                        result: None,
                                        then_body: vec![
                                            Instr::I32Const(0),
                                            Instr::LocalSet(5),
                                            Instr::Br("$vbreak".to_string()),
                                        ],
                                        else_body: vec![],
                                    },
                                    // Check idx+1 < len
                                    Instr::LocalGet(2),
                                    Instr::I32Const(1),
                                    Instr::I32Add,
                                    Instr::LocalGet(1),
                                    Instr::I32GeU,
                                    Instr::If {
                                        result: None,
                                        then_body: vec![
                                            Instr::I32Const(0),
                                            Instr::LocalSet(5),
                                            Instr::Br("$vbreak".to_string()),
                                        ],
                                        else_body: vec![],
                                    },
                                    // Check continuation byte at idx+1
                                    Instr::LocalGet(0),
                                    Instr::LocalGet(2),
                                    Instr::I32Const(1),
                                    Instr::I32Add,
                                    Instr::ArrayGet(T_ARRAY.to_string()),
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: crate::wasm::ir::HeapType::I31,
                                    },
                                    Instr::I31GetU,
                                    Instr::LocalSet(6),
                                    // continuation = (byte & 0xC0) == 0x80
                                    Instr::LocalGet(6),
                                    Instr::I32Const(0xC0),
                                    Instr::I32And,
                                    Instr::I32Const(0x80),
                                    Instr::I32Ne,
                                    Instr::If {
                                        result: None,
                                        then_body: vec![
                                            Instr::I32Const(0),
                                            Instr::LocalSet(5),
                                            Instr::Br("$vbreak".to_string()),
                                        ],
                                        else_body: vec![],
                                    },
                                    // advance by 2
                                    Instr::LocalGet(2),
                                    Instr::I32Const(2),
                                    Instr::I32Add,
                                    Instr::LocalSet(2),
                                ],
                                else_body: vec![
                                    // Check 3-byte lead: 0xE0..0xEF
                                    Instr::LocalGet(4),
                                    Instr::I32Const(0xE0),
                                    Instr::I32GeU,
                                    Instr::LocalGet(4),
                                    Instr::I32Const(0xEF),
                                    Instr::I32LeU,
                                    Instr::I32And,
                                    Instr::If {
                                        result: None,
                                        then_body: emit_utf8_validate_multibyte(3),
                                        else_body: vec![
                                            // Check 4-byte lead: 0xF0..0xF4
                                            Instr::LocalGet(4),
                                            Instr::I32Const(0xF0),
                                            Instr::I32GeU,
                                            Instr::LocalGet(4),
                                            Instr::I32Const(0xF4),
                                            Instr::I32LeU,
                                            Instr::I32And,
                                            Instr::If {
                                                result: None,
                                                then_body: emit_utf8_validate_multibyte(4),
                                                // Not a valid lead byte
                                                else_body: vec![
                                                    Instr::I32Const(0),
                                                    Instr::LocalSet(5),
                                                    Instr::Br("$vbreak".to_string()),
                                                ],
                                            },
                                        ],
                                    },
                                ],
                            },
                        ],
                    },
                    Instr::Br("$vcont".to_string()),
                ],
            }],
        },
        // After validation: if !valid, return None
        Instr::LocalGet(5),
        Instr::I32Eqz,
        Instr::If {
            result: Some(ValType::Anyref),
            then_body: mk_none(),
            else_body: {
                // Valid! Copy bytes into a new $String
                let mut some_body = vec![
                    // Allocate string: array.new_default $String (len)
                    Instr::LocalGet(1),
                    Instr::ArrayNewDefault(T_STRING.to_string()),
                    Instr::LocalSet(3),
                    // idx = 0
                    Instr::I32Const(0),
                    Instr::LocalSet(2),
                    // Copy loop
                    Instr::Block {
                        label: "$cbreak".to_string(),
                        result: None,
                        body: vec![Instr::Loop {
                            label: "$ccont".to_string(),
                            result: None,
                            body: vec![
                                Instr::LocalGet(2),
                                Instr::LocalGet(1),
                                Instr::I32GeU,
                                Instr::BrIf("$cbreak".to_string()),
                                // string[idx] = i31.get_u(array[idx])
                                Instr::LocalGet(3),
                                Instr::LocalGet(2),
                                Instr::LocalGet(0),
                                Instr::LocalGet(2),
                                Instr::ArrayGet(T_ARRAY.to_string()),
                                Instr::RefCast {
                                    nullable: false,
                                    heap: crate::wasm::ir::HeapType::I31,
                                },
                                Instr::I31GetU,
                                Instr::ArraySet(T_STRING.to_string()),
                                // idx += 1
                                Instr::LocalGet(2),
                                Instr::I32Const(1),
                                Instr::I32Add,
                                Instr::LocalSet(2),
                                Instr::Br("$ccont".to_string()),
                            ],
                        }],
                    },
                    // Build Some(string) variant
                    Instr::I32Const(0), // type_id (Option)
                    Instr::I32Const(1), // variant_id (Some)
                    Instr::LocalGet(3),
                ];
                some_body.extend(emit_coerce_stack(&ref_string_null(), &ValType::Anyref));
                some_body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
                some_body.push(Instr::StructNew(T_VARIANT.to_string()));
                some_body.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
                some_body
            },
        },
    ];

    FuncDef {
        name: "$string_from_utf8_helper".to_string(),
        params: vec![ref_array_null()],
        results: vec![ValType::Anyref],
        locals: vec![
            ValType::I32,      // local 1: len
            ValType::I32,      // local 2: idx
            ref_string_null(), // local 3: result string
            ValType::I32,      // local 4: current byte
            ValType::I32,      // local 5: valid flag
            ValType::I32,      // local 6: temp for continuation byte check
        ],
        body,
    }
}

/// Emit validation for a 3-byte or 4-byte UTF-8 sequence.
/// Checks that the required number of continuation bytes (n-1) follow the lead byte.
/// Uses locals: 0=bytes, 1=len, 2=idx, 4=lead_byte, 5=valid, 6=temp.
fn emit_utf8_validate_multibyte(n: u32) -> Vec<Instr> {
    use crate::runtime::types::T_ARRAY;
    let mut instrs = Vec::new();

    // Check that idx + n - 1 < len (enough bytes remaining)
    instrs.push(Instr::LocalGet(2));
    instrs.push(Instr::I32Const(n as i32 - 1));
    instrs.push(Instr::I32Add);
    instrs.push(Instr::LocalGet(1));
    instrs.push(Instr::I32GeU);
    instrs.push(Instr::If {
        result: None,
        then_body: vec![
            Instr::I32Const(0),
            Instr::LocalSet(5),
            Instr::Br("$vbreak".to_string()),
        ],
        else_body: vec![],
    });

    // Check each continuation byte
    for offset in 1..n {
        instrs.push(Instr::LocalGet(0));
        instrs.push(Instr::LocalGet(2));
        instrs.push(Instr::I32Const(offset as i32));
        instrs.push(Instr::I32Add);
        instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
        instrs.push(Instr::RefCast {
            nullable: false,
            heap: crate::wasm::ir::HeapType::I31,
        });
        instrs.push(Instr::I31GetU);
        instrs.push(Instr::I32Const(0xC0));
        instrs.push(Instr::I32And);
        instrs.push(Instr::I32Const(0x80));
        instrs.push(Instr::I32Ne);
        instrs.push(Instr::If {
            result: None,
            then_body: vec![
                Instr::I32Const(0),
                Instr::LocalSet(5),
                Instr::Br("$vbreak".to_string()),
            ],
            else_body: vec![],
        });
    }

    // Additional checks for 3-byte sequences: reject surrogates and overlongs
    if n == 3 {
        // If lead byte == 0xE0, second byte must be >= 0xA0 (reject overlong)
        instrs.push(Instr::LocalGet(4));
        instrs.push(Instr::I32Const(0xE0));
        instrs.push(Instr::I32Eq);
        instrs.push(Instr::If {
            result: None,
            then_body: vec![
                Instr::LocalGet(0),
                Instr::LocalGet(2),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::ArrayGet(T_ARRAY.to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: crate::wasm::ir::HeapType::I31,
                },
                Instr::I31GetU,
                Instr::I32Const(0xA0),
                Instr::I32LtU,
                Instr::If {
                    result: None,
                    then_body: vec![
                        Instr::I32Const(0),
                        Instr::LocalSet(5),
                        Instr::Br("$vbreak".to_string()),
                    ],
                    else_body: vec![],
                },
            ],
            else_body: vec![],
        });
        // If lead byte == 0xED, second byte must be < 0xA0 (reject surrogates)
        instrs.push(Instr::LocalGet(4));
        instrs.push(Instr::I32Const(0xED));
        instrs.push(Instr::I32Eq);
        instrs.push(Instr::If {
            result: None,
            then_body: vec![
                Instr::LocalGet(0),
                Instr::LocalGet(2),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::ArrayGet(T_ARRAY.to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: crate::wasm::ir::HeapType::I31,
                },
                Instr::I31GetU,
                Instr::I32Const(0xA0),
                Instr::I32GeU,
                Instr::If {
                    result: None,
                    then_body: vec![
                        Instr::I32Const(0),
                        Instr::LocalSet(5),
                        Instr::Br("$vbreak".to_string()),
                    ],
                    else_body: vec![],
                },
            ],
            else_body: vec![],
        });
    }

    // Additional checks for 4-byte sequences
    if n == 4 {
        // If lead byte == 0xF0, second byte must be >= 0x90 (reject overlong)
        instrs.push(Instr::LocalGet(4));
        instrs.push(Instr::I32Const(0xF0));
        instrs.push(Instr::I32Eq);
        instrs.push(Instr::If {
            result: None,
            then_body: vec![
                Instr::LocalGet(0),
                Instr::LocalGet(2),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::ArrayGet(T_ARRAY.to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: crate::wasm::ir::HeapType::I31,
                },
                Instr::I31GetU,
                Instr::I32Const(0x90),
                Instr::I32LtU,
                Instr::If {
                    result: None,
                    then_body: vec![
                        Instr::I32Const(0),
                        Instr::LocalSet(5),
                        Instr::Br("$vbreak".to_string()),
                    ],
                    else_body: vec![],
                },
            ],
            else_body: vec![],
        });
        // If lead byte == 0xF4, second byte must be < 0x90 (reject > U+10FFFF)
        instrs.push(Instr::LocalGet(4));
        instrs.push(Instr::I32Const(0xF4));
        instrs.push(Instr::I32Eq);
        instrs.push(Instr::If {
            result: None,
            then_body: vec![
                Instr::LocalGet(0),
                Instr::LocalGet(2),
                Instr::I32Const(1),
                Instr::I32Add,
                Instr::ArrayGet(T_ARRAY.to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: crate::wasm::ir::HeapType::I31,
                },
                Instr::I31GetU,
                Instr::I32Const(0x90),
                Instr::I32GeU,
                Instr::If {
                    result: None,
                    then_body: vec![
                        Instr::I32Const(0),
                        Instr::LocalSet(5),
                        Instr::Br("$vbreak".to_string()),
                    ],
                    else_body: vec![],
                },
            ],
            else_body: vec![],
        });
    }

    // advance by n
    instrs.push(Instr::LocalGet(2));
    instrs.push(Instr::I32Const(n as i32));
    instrs.push(Instr::I32Add);
    instrs.push(Instr::LocalSet(2));

    instrs
}

/// Generate helper function: $float_from_string_helper
/// Takes (ref null $String) → anyref (Option<Float> variant)
fn emit_float_from_string_helper() -> FuncDef {
    let body = vec![
        Instr::LocalGet(0),
        Instr::Call("host_parse_float".to_string()),
        // Stack: [f64, i32]
        Instr::LocalSet(2), // save ok
        Instr::LocalSet(1), // save value
        Instr::LocalGet(2),
        Instr::If {
            result: Some(ValType::Anyref),
            then_body: {
                let mut v = vec![
                    Instr::I32Const(0),
                    Instr::I32Const(1),
                    Instr::LocalGet(1),
                    Instr::StructNew(T_BOXED_FLOAT.to_string()),
                ];
                v.extend(emit_coerce_stack(
                    &ValType::Ref {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_FLOAT.to_string()),
                    },
                    &ValType::Anyref,
                ));
                v.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
                v.push(Instr::StructNew(T_VARIANT.to_string()));
                v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
                v
            },
            else_body: {
                let mut v = vec![
                    Instr::I32Const(0),
                    Instr::I32Const(0),
                    Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
                    Instr::StructNew(T_VARIANT.to_string()),
                ];
                v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
                v
            },
        },
    ];

    FuncDef {
        name: "$float_from_string_helper".to_string(),
        params: vec![ref_string_null()],
        results: vec![ValType::Anyref],
        locals: vec![ValType::F64, ValType::I32],
        body,
    }
}

fn emit_float_bits_intrinsic(args: &[Atom], ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Float.bits(f: Float) -> Int  (IEEE 754 bit pattern)
    // Float is f64, Int is i64
    let mut instrs = emit_atom(&args[0], Some(&ValType::F64), ctx);
    instrs.push(Instr::I64ReinterpretF64);
    instrs
}

fn emit_byte_to_int_intrinsic(args: &[Atom], ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Byte.to_int(b: Byte) -> Int
    // Byte is i32, Int is i64 — zero-extend
    let mut instrs = emit_atom(&args[0], Some(&ValType::I32), ctx);
    instrs.push(Instr::I64ExtendI32U);
    instrs
}

fn emit_byte_from_int_intrinsic(args: &[Atom], ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Byte.from_int(n: Int) -> Option<Byte>
    // If 0 <= n <= 255, return Some(n as i32); else None.
    // Check: n >= 0
    let mut instrs = emit_atom(&args[0], Some(&ValType::I64), ctx);
    instrs.push(Instr::I64Const(0));
    instrs.push(Instr::I64GeS);
    // Check: n <= 255
    instrs.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
    instrs.push(Instr::I64Const(256));
    instrs.push(Instr::I64LtS);
    // AND both conditions
    instrs.push(Instr::I32And);
    instrs.push(Instr::If {
        result: Some(ValType::Anyref),
        then_body: {
            // Some(byte_val)
            let mut v = vec![
                Instr::I32Const(0), // type_id (Option)
                Instr::I32Const(1), // variant_id (Some)
            ];
            v.extend(emit_atom(&args[0], Some(&ValType::I64), ctx));
            v.push(Instr::I32WrapI64);
            // Box i32 as anyref (ref.i31)
            v.push(Instr::RefI31);
            v.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
            v.push(Instr::StructNew(T_VARIANT.to_string()));
            v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
            v
        },
        else_body: {
            // None
            let mut v = vec![
                Instr::I32Const(0), // type_id
                Instr::I32Const(0), // variant_id (None)
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
                Instr::StructNew(T_VARIANT.to_string()),
            ];
            v.extend(emit_coerce_stack(&ref_variant(), &ValType::Anyref));
            v
        },
    });
    instrs
}

fn emit_byte_to_string_intrinsic(args: &[Atom], ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Byte.to_string(b: Byte) -> String
    // Reuse int_to_string runtime: extend i32 to i64, call rt_str__from_i64
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "from_i64".to_string(),
        as_sym: "rt_str__from_i64".to_string(),
        params: vec![ValType::I64],
        results: vec![ref_string_null()],
    });
    let mut instrs = emit_atom(&args[0], Some(&ValType::I32), ctx);
    instrs.push(Instr::I64ExtendI32U);
    instrs.push(Instr::Call("rt_str__from_i64".to_string()));
    instrs
}

fn emit_unimplemented_intrinsic_prelude_call(
    entry: &crate::codegen::prelude::PreludeEntry,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    ensure_rt_core_trap_import(ctx);
    let mut instrs = emit_pooled_string_literal_atom(
        &format!(
            "unimplemented intrinsic prelude call: {}",
            entry.twinkle_name
        ),
        ctx,
    );
    instrs.push(Instr::Call("rt_core__trap".to_string()));
    instrs.push(Instr::Unreachable);
    instrs
}

fn emit_runtime_prelude_call(
    func_id: FuncId,
    entry: &crate::codegen::prelude::PreludeEntry,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != entry.runtime_params.len() {
        panic!(
            "arity mismatch for prelude call '{}': expected {}, got {}",
            entry.twinkle_name,
            entry.runtime_params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (i, (arg, param_ty)) in args.iter().zip(entry.runtime_params.iter()).enumerate() {
        if is_host_vector_arg(func_id, i) {
            // Host boundary: emit as PVec, then convert PVec → flat Array
            instrs.extend(emit_atom(arg, Some(&ref_pvec_null()), ctx));
            ensure_rt_arr_to_array_import(ctx);
            instrs.push(Instr::Call("rt_arr__to_array".to_string()));
        } else {
            instrs.extend(emit_atom(arg, Some(param_ty), ctx));
        }
    }
    let sym = entry.runtime_sym.as_ref().cloned().unwrap_or_else(|| {
        panic!(
            "runtime prelude entry missing symbol: {}",
            entry.twinkle_name
        )
    });

    if is_host_vector_returning(func_id) || has_host_vector_args(func_id) {
        // For host vector boundary functions, add the import with the
        // actual host-side types ($Array), not the language-side types ($PVec).
        let host_params: Vec<ValType> = entry
            .runtime_params
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                if is_host_vector_arg(func_id, i) {
                    ref_array_null()
                } else {
                    ty.clone()
                }
            })
            .collect();
        let host_results = if is_host_vector_returning(func_id) {
            vec![ref_array()]
        } else {
            entry.runtime_results.clone()
        };
        ctx.add_import(ImportDef {
            module: entry.runtime_module.unwrap().to_string(),
            name: entry.runtime_name.unwrap().to_string(),
            as_sym: sym.clone(),
            params: host_params,
            results: host_results,
        });
    } else {
        ctx.add_runtime_import(entry);
    }

    instrs.push(Instr::Call(sym));

    if is_host_vector_returning(func_id) {
        // Host returned flat $Array, convert to $PVec
        ensure_rt_arr_from_array_import(ctx);
        instrs.push(Instr::Call("rt_arr__from_array".to_string()));
        instrs.extend(emit_coerce_stack(&ref_pvec(), bind_ty));
    } else if is_host_read_file(func_id) {
        ensure_rt_arr_from_read_file_result_import(ctx);
        instrs.push(Instr::Call("rt_arr__from_read_file_result".to_string()));
        match entry.runtime_results.as_slice() {
            [] => instrs.extend(emit_void_value(Some(bind_ty))),
            [single] => instrs.extend(emit_coerce_stack(single, bind_ty)),
            _ => panic!(
                "multi-value runtime prelude return not supported yet: {}",
                entry.twinkle_name
            ),
        }
    } else {
        match entry.runtime_results.as_slice() {
            [] => instrs.extend(emit_void_value(Some(bind_ty))),
            [single] => instrs.extend(emit_coerce_stack(single, bind_ty)),
            _ => panic!(
                "multi-value runtime prelude return not supported yet: {}",
                entry.twinkle_name
            ),
        }
    }

    instrs
}

/// Host functions that return a flat $Array representing a Vector.
fn is_host_vector_returning(func_id: FuncId) -> bool {
    use crate::ir::lower::prelude as ids;
    func_id == ids::HOST_ARGS || func_id == ids::HOST_LIST_DIR || func_id == ids::HOST_ENV
}

fn is_host_read_file(func_id: FuncId) -> bool {
    use crate::ir::lower::prelude as ids;
    func_id == ids::HOST_READ_FILE
}

/// Whether arg at `index` for `func_id` is a host-boundary vector that needs
/// PVec → flat Array conversion before the host call.
fn is_host_vector_arg(func_id: FuncId, index: usize) -> bool {
    use crate::ir::lower::prelude as ids;
    func_id == ids::HOST_WRITE_BYTES && index == 1
}

/// Whether any arg of `func_id` needs PVec → Array conversion.
fn has_host_vector_args(func_id: FuncId) -> bool {
    use crate::ir::lower::prelude as ids;
    func_id == ids::HOST_WRITE_BYTES
}

fn ensure_rt_arr_from_array_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "from_array".to_string(),
        as_sym: "rt_arr__from_array".to_string(),
        params: vec![ref_array()],
        results: vec![ref_pvec()],
    });
}

fn ensure_rt_arr_to_array_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "to_array".to_string(),
        as_sym: "rt_arr__to_array".to_string(),
        params: vec![ref_pvec_null()],
        results: vec![ref_array()],
    });
}

fn ensure_rt_arr_from_read_file_result_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "from_read_file_result".to_string(),
        as_sym: "rt_arr__from_read_file_result".to_string(),
        params: vec![ValType::Ref {
            nullable: true,
            heap: HeapType::Named(T_VARIANT.into()),
        }],
        results: vec![ValType::Ref {
            nullable: true,
            heap: HeapType::Named(T_VARIANT.into()),
        }],
    });
}

fn emit_unop(
    op: crate::syntax::ast::UnOp,
    expr: &Atom,
    operand_ty: crate::ir::anf::OpKind,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    match op {
        crate::syntax::ast::UnOp::Neg => match operand_ty {
            crate::ir::anf::OpKind::Int => {
                let mut instrs = vec![Instr::I64Const(0)];
                instrs.extend(emit_atom(expr, Some(&ValType::I64), ctx));
                instrs.push(Instr::I64Sub);
                instrs
            }
            crate::ir::anf::OpKind::Float => {
                let mut instrs = emit_atom(expr, Some(&ValType::F64), ctx);
                instrs.push(Instr::F64Neg);
                instrs
            }
            _ => panic!("unsupported unary negation operand type {:?}", operand_ty),
        },
        crate::syntax::ast::UnOp::Not => {
            let mut instrs = emit_atom(expr, Some(&ValType::I32), ctx);
            instrs.push(Instr::I32Eqz);
            instrs
        }
        crate::syntax::ast::UnOp::BitNot => {
            // ~x  ⟹  i64.xor(x, -1)  (i.e. all-ones mask)
            let mut instrs = emit_atom(expr, Some(&ValType::I64), ctx);
            instrs.push(Instr::I64Const(-1));
            instrs.push(Instr::I64Xor);
            instrs
        }
    }
}

fn operand_valtype(kind: crate::ir::anf::OpKind) -> ValType {
    match kind {
        crate::ir::anf::OpKind::Int => ValType::I64,
        crate::ir::anf::OpKind::Float => ValType::F64,
        crate::ir::anf::OpKind::Bool => ValType::I32,
        crate::ir::anf::OpKind::String => ref_string_null(),
    }
}

fn ref_closure() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_CLOSURE.to_string()),
    }
}

fn ref_closure_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_CLOSURE.to_string()),
    }
}

fn ref_user_record(type_id: TypeId) -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(user_record_type_sym(type_id)),
    }
}

fn ref_variant() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_VARIANT.to_string()),
    }
}

fn ref_variant_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_VARIANT.to_string()),
    }
}

fn ensure_rt_str_eq_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "eq".to_string(),
        as_sym: "rt_str__eq".to_string(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ValType::I32],
    });
}

fn ensure_rt_str_cmp_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "cmp".to_string(),
        as_sym: "rt_str__cmp".to_string(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ValType::I32],
    });
}

fn ensure_rt_str_concat_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "concat".to_string(),
        as_sym: "rt_str__concat".to_string(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ref_string()],
    });
}

fn ensure_rt_str_substring_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "substring".to_string(),
        as_sym: "rt_str__substring".to_string(),
        params: vec![ref_string_null(), ValType::I32, ValType::I32],
        results: vec![ref_string()],
    });
}

fn ensure_rt_arr_get_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "get".to_string(),
        as_sym: "rt_arr__get".to_string(),
        params: vec![ref_pvec_null(), ValType::I32],
        results: vec![ValType::Anyref],
    });
}

fn ensure_rt_arr_set_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "set".to_string(),
        as_sym: "rt_arr__set".to_string(),
        params: vec![ref_pvec_null(), ValType::I32, ValType::Anyref],
        results: vec![ref_pvec()],
    });
}

fn ensure_rt_arr_push_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "push".to_string(),
        as_sym: "rt_arr__push".to_string(),
        params: vec![ref_pvec(), ValType::Anyref],
        results: vec![ref_pvec()],
    });
}

fn ensure_rt_arr_make_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "make".to_string(),
        as_sym: "rt_arr__make".to_string(),
        params: vec![ValType::I32, ValType::Anyref],
        results: vec![ref_pvec()],
    });
}

fn ensure_rt_dict_get_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.dict".to_string(),
        name: "get".to_string(),
        as_sym: "rt_dict__get".to_string(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
    });
}

fn ensure_rt_dict_get_option_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.dict".to_string(),
        name: "get_option".to_string(),
        as_sym: "rt_dict__get_option".to_string(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_variant()],
    });
}

fn ensure_rt_core_trap_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.core".to_string(),
        name: "trap".to_string(),
        as_sym: "rt_core__trap".to_string(),
        params: vec![ref_string_null()],
        results: vec![],
    });
}

fn atom_produces_value(atom: &Atom) -> bool {
    !matches!(atom, Atom::ALitVoid)
}

fn global_func_trampoline_sym(func_id: FuncId) -> String {
    format!("{}__closure", user_func_sym(func_id))
}

fn typed_closure_trampoline_sym(func_id: FuncId) -> String {
    format!("{}__typed_closure", user_func_sym(func_id))
}

// ─── Prelude closure trampolines ──────────────────────────────────────────────

/// Collect prelude FuncIds that appear as first-class values (not just call
/// targets) anywhere in the ANF module.
fn collect_prelude_func_refs(anf: &AnfModule) -> Vec<FuncId> {
    let user_func_ids: HashSet<FuncId> = anf.functions.iter().map(|f| f.func_id).collect();
    let mut prelude_refs = HashSet::new();
    for func in &anf.functions {
        collect_prelude_refs_expr(&func.body, &user_func_ids, &mut prelude_refs);
    }
    let mut sorted: Vec<FuncId> = prelude_refs.into_iter().collect();
    sorted.sort_by_key(|f| f.0);
    sorted
}

fn collect_prelude_refs_expr(
    expr: &AnfExpr,
    user_funcs: &HashSet<FuncId>,
    out: &mut HashSet<FuncId>,
) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_prelude_refs_op(op, user_funcs, out);
            collect_prelude_refs_expr(body, user_funcs, out);
        }
        AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) | AnfExpr::Atom(atom) => {
            collect_prelude_refs_atom(atom, user_funcs, out);
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
    }
}

fn collect_prelude_refs_atom(atom: &Atom, user_funcs: &HashSet<FuncId>, out: &mut HashSet<FuncId>) {
    if let Atom::AGlobalFunc(func_id) = atom {
        if !user_funcs.contains(func_id) {
            out.insert(*func_id);
        }
    }
}

fn collect_prelude_refs_op(op: &AnfOp, user_funcs: &HashSet<FuncId>, out: &mut HashSet<FuncId>) {
    match op {
        AnfOp::ACall { args, .. } => {
            for arg in args {
                collect_prelude_refs_atom(arg, user_funcs, out);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_prelude_refs_atom(cond, user_funcs, out);
            collect_prelude_refs_expr(then_branch, user_funcs, out);
            collect_prelude_refs_expr(else_branch, user_funcs, out);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            collect_prelude_refs_atom(scrutinee, user_funcs, out);
            for arm in arms {
                collect_prelude_refs_expr(&arm.body, user_funcs, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_prelude_refs_expr(body, user_funcs, out);
        }
        AnfOp::AMakeClosure { .. } => {}
        AnfOp::AInit { value } | AnfOp::AAssign { value, .. } => {
            collect_prelude_refs_atom(value, user_funcs, out);
        }
        _ => {}
    }
}

/// Emit a universal closure trampoline for a prelude (runtime) function.
/// The trampoline signature is `(anyref, anyref) -> anyref` — matching the
/// universal `$ClosureFunc` type.  It unpacks args from the anyref array,
/// calls the runtime function, and boxes the result back to anyref.
fn emit_prelude_closure_trampoline(
    func_id: FuncId,
    entry: &crate::codegen::prelude::PreludeEntry,
) -> FuncDef {
    let mut body = Vec::new();

    // Unbox each parameter from the anyref arg-array (local 1).
    for (idx, param_ty) in entry.runtime_params.iter().enumerate() {
        body.push(Instr::LocalGet(1)); // arg array (anyref)
        body.push(Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_ARRAY.to_string()),
        });
        body.push(Instr::I32Const(idx as i32));
        body.push(Instr::ArrayGet(T_ARRAY.to_string()));
        body.extend(emit_unbox_on_stack(param_ty));
    }

    // Call the runtime function.
    if let Some(ref sym) = entry.runtime_sym {
        body.push(Instr::Call(sym.clone()));
    } else {
        // Intrinsic with no runtime sym (e.g. string_to_string = identity).
        // The single arg is already on the stack after unboxing; just coerce.
        // For intrinsics that are identity functions, the arg is already there.
    }

    // Box the result to anyref.
    match entry.runtime_results.first() {
        Some(result_ty) => body.extend(emit_coerce_stack(result_ty, &ValType::Anyref)),
        None => body.extend(emit_void_value(Some(&ValType::Anyref))),
    }

    FuncDef {
        name: global_func_trampoline_sym(func_id),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::Anyref],
        locals: vec![],
        body,
    }
}

// ─── Stage 9.6: Typed Closure Specialization ─────────────────────────────────

/// Collect user functions that appear as first-class function values and have
/// fully concrete (non-generic) param and return types. This includes both
/// `AMakeClosure`-originated functions and plain named function values that
/// flow through non-callee positions.
fn collect_concrete_func_signatures(
    anf: &AnfModule,
) -> std::collections::HashMap<FuncId, (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType)>
{
    let func_lookup: HashMap<FuncId, &AnfFunctionDef> =
        anf.functions.iter().map(|f| (f.func_id, f)).collect();
    let mut sigs = std::collections::HashMap::new();
    for func in &anf.functions {
        collect_concrete_sigs_expr(&func.body, &func_lookup, &mut sigs);
    }
    sigs
}

fn collect_concrete_sigs_expr(
    expr: &AnfExpr,
    func_lookup: &HashMap<FuncId, &AnfFunctionDef>,
    sigs: &mut std::collections::HashMap<
        FuncId,
        (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType),
    >,
) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            collect_concrete_sigs_op(op, func_lookup, sigs);
            collect_concrete_sigs_expr(body, func_lookup, sigs);
        }
        AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) | AnfExpr::Atom(atom) => {
            collect_concrete_sigs_atom(atom, func_lookup, sigs);
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
    }
}

fn maybe_insert_concrete_sig(
    func_id: FuncId,
    func_lookup: &HashMap<FuncId, &AnfFunctionDef>,
    sigs: &mut std::collections::HashMap<
        FuncId,
        (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType),
    >,
) {
    if let Some(func) = func_lookup.get(&func_id) {
        if func.param_tys.iter().all(is_concrete_mono_type)
            && is_concrete_mono_type(&func.return_ty)
        {
            sigs.insert(func_id, (func.param_tys.clone(), func.return_ty.clone()));
        }
    }
}

fn collect_concrete_sigs_atom(
    atom: &Atom,
    func_lookup: &HashMap<FuncId, &AnfFunctionDef>,
    sigs: &mut std::collections::HashMap<
        FuncId,
        (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType),
    >,
) {
    if let Atom::AGlobalFunc(func_id) = atom {
        maybe_insert_concrete_sig(*func_id, func_lookup, sigs);
    }
}

fn collect_concrete_sigs_op(
    op: &AnfOp,
    func_lookup: &HashMap<FuncId, &AnfFunctionDef>,
    sigs: &mut std::collections::HashMap<
        FuncId,
        (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType),
    >,
) {
    match op {
        AnfOp::ACall { args, .. } => {
            for arg in args {
                collect_concrete_sigs_atom(arg, func_lookup, sigs);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_concrete_sigs_atom(cond, func_lookup, sigs);
            collect_concrete_sigs_expr(then_branch, func_lookup, sigs);
            collect_concrete_sigs_expr(else_branch, func_lookup, sigs);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            collect_concrete_sigs_atom(scrutinee, func_lookup, sigs);
            for arm in arms {
                collect_concrete_sigs_expr(&arm.body, func_lookup, sigs);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_concrete_sigs_expr(body, func_lookup, sigs);
        }
        AnfOp::ABinOp { left, right, .. } => {
            collect_concrete_sigs_atom(left, func_lookup, sigs);
            collect_concrete_sigs_atom(right, func_lookup, sigs);
        }
        AnfOp::AUnOp { expr, .. } => {
            collect_concrete_sigs_atom(expr, func_lookup, sigs);
        }
        AnfOp::AMakeClosure { func_id, .. } => {
            maybe_insert_concrete_sig(*func_id, func_lookup, sigs);
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, atom) in fields {
                collect_concrete_sigs_atom(atom, func_lookup, sigs);
            }
        }
        AnfOp::ARecordGet { target, .. } => {
            collect_concrete_sigs_atom(target, func_lookup, sigs);
        }
        AnfOp::ARecordUpdate { base, value, .. } => {
            collect_concrete_sigs_atom(base, func_lookup, sigs);
            collect_concrete_sigs_atom(value, func_lookup, sigs);
        }
        AnfOp::AVariant { args, .. } | AnfOp::AArrayLit(args) => {
            for atom in args {
                collect_concrete_sigs_atom(atom, func_lookup, sigs);
            }
        }
        AnfOp::AIndex { base, index, .. } => {
            collect_concrete_sigs_atom(base, func_lookup, sigs);
            collect_concrete_sigs_atom(index, func_lookup, sigs);
        }
        AnfOp::AInit { value } => {
            collect_concrete_sigs_atom(value, func_lookup, sigs);
        }
        AnfOp::AAssign { value, .. } => {
            collect_concrete_sigs_atom(value, func_lookup, sigs);
        }
    }
}

/// Build the WAT `FuncType` definition for a typed closure func type.
/// e.g. `(type $closurefunc_i64_i64_i64 (func (param (ref null $ClosureEnv)) (param i64) (param i64) (result i64)))`
fn emit_typed_closurefunc_def(
    params: &[crate::types::ty::MonoType],
    ret: &crate::types::ty::MonoType,
    abi_results: &[ValType],
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> crate::wasm::ir::TypeDef {
    let sym = typed_closurefunc_sym(params, ret);
    let mut wasm_params = vec![ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_CLOSURE_ENV.to_string()),
    }];
    wasm_params.extend(
        params
            .iter()
            .map(|p| mono_to_valtype_for_user_abi_param(p, type_env, concrete_func_sigs)),
    );
    crate::wasm::ir::TypeDef::FuncType {
        name: sym,
        params: wasm_params,
        results: abi_results.to_vec(),
    }
}

/// Build the WAT `Struct` definition for a typed closure struct.
///
/// The typed closure struct is a **subtype** of the universal `$Closure`:
///   field 0 = func_ref  (ref null $ClosureFunc)         — universal funcref (inherited)
///   field 1 = env       (ref null $ClosureEnv)           — capture env (inherited)
///   field 2 = typed_ref (ref null $closurefunc_i64_...) — typed funcref (new)
///
/// Because it's a subtype, a typed closure can be stored in `anyref` / passed
/// as `(ref null $Closure)` and dispatched via the universal path. Typed call
/// sites access field 2 for the concrete `call_ref`.
fn emit_typed_closure_struct_def(
    params: &[crate::types::ty::MonoType],
    ret: &crate::types::ty::MonoType,
) -> crate::wasm::ir::TypeDef {
    let closurefunc_sym = typed_closurefunc_sym(params, ret);
    let closure_sym = typed_closure_struct_sym(params, ret);
    crate::wasm::ir::TypeDef::Struct {
        name: closure_sym,
        supertype: Some(format!("{T_CLOSURE}")),
        non_final: false,
        fields: vec![
            // field 0: universal funcref (must match $Closure field 0)
            crate::wasm::ir::FieldDef {
                name: Some("func_ref".to_string()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(T_CLOSURE_FUNC.to_string()),
                },
            },
            // field 1: env (must match $Closure field 1)
            crate::wasm::ir::FieldDef {
                name: Some("env".to_string()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(T_CLOSURE_ENV.to_string()),
                },
            },
            // field 2: typed funcref (new field)
            crate::wasm::ir::FieldDef {
                name: Some("typed_ref".to_string()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(closurefunc_sym),
                },
            },
        ],
    }
}

fn emit_typed_cell_struct_def(
    elem: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> crate::wasm::ir::TypeDef {
    crate::wasm::ir::TypeDef::Struct {
        name: typed_cell_struct_sym(elem),
        supertype: None,
        non_final: false,
        fields: vec![crate::wasm::ir::FieldDef {
            name: Some("value".to_string()),
            mutable: true,
            ty: mono_to_valtype_specialized(elem, type_env, concrete_func_sigs),
        }],
    }
}

fn emit_typed_unfold_step_struct_def(
    yield_ty: &MonoType,
    seed_ty: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> WasmTypeDef {
    WasmTypeDef::Struct {
        name: typed_unfold_step_sym(yield_ty, seed_ty),
        supertype: None,
        non_final: false,
        fields: vec![
            WasmFieldDef {
                name: Some("variant_id".to_string()),
                mutable: false,
                ty: ValType::I32,
            },
            WasmFieldDef {
                name: Some("f0".to_string()),
                mutable: false,
                ty: mono_to_valtype_specialized(yield_ty, type_env, concrete_func_sigs),
            },
            WasmFieldDef {
                name: Some("f1".to_string()),
                mutable: false,
                ty: mono_to_valtype_specialized(seed_ty, type_env, concrete_func_sigs),
            },
        ],
    }
}

fn emit_typed_iterator_state_struct_def(
    info: &IteratorStateInfo,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> WasmTypeDef {
    let step_ret = unfold_step_type(info.yield_ty.clone(), info.seed_ty.clone());
    let closure_sym = typed_closure_struct_sym(std::slice::from_ref(&info.seed_ty), &step_ret);
    WasmTypeDef::Struct {
        name: typed_iterator_state_sym(info),
        supertype: None,
        non_final: false,
        fields: vec![
            WasmFieldDef {
                name: Some("seed".to_string()),
                mutable: false,
                ty: mono_to_valtype_specialized(&info.seed_ty, type_env, concrete_func_sigs),
            },
            WasmFieldDef {
                name: Some("step".to_string()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(closure_sym),
                },
            },
        ],
    }
}

fn emit_typed_iter_item_struct_def(
    info: &IteratorStateInfo,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> WasmTypeDef {
    WasmTypeDef::Struct {
        name: typed_iter_item_sym(info),
        supertype: None,
        non_final: false,
        fields: vec![
            WasmFieldDef {
                name: Some("value".to_string()),
                mutable: true,
                ty: mono_to_valtype_specialized(&info.yield_ty, type_env, concrete_func_sigs),
            },
            WasmFieldDef {
                name: Some("rest".to_string()),
                mutable: true,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(typed_iterator_state_sym(info)),
                },
            },
        ],
    }
}

/// Emit a typed struct definition for a general `Option<T>` or `Result<T, E>`.
/// Option layout: (variant_id: i32, payload: T_wasm)
/// Result layout: (variant_id: i32, ok_payload: T_wasm, err_payload: E_wasm)
fn emit_typed_general_option_struct_def(
    mono: &MonoType,
    type_env: &TypeEnv,
    concrete_func_sigs: &HashMap<FuncId, (Vec<MonoType>, MonoType)>,
) -> WasmTypeDef {
    match mono {
        MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID && args.len() == 1 => {
            WasmTypeDef::Struct {
                name: typed_general_option_sym(mono),
                supertype: None,
                non_final: false,
                fields: vec![
                    WasmFieldDef {
                        name: Some("variant_id".to_string()),
                        mutable: false,
                        ty: ValType::I32,
                    },
                    WasmFieldDef {
                        name: Some("payload".to_string()),
                        mutable: false,
                        ty: mono_to_valtype_specialized(&args[0], type_env, concrete_func_sigs),
                    },
                ],
            }
        }
        MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID && args.len() == 2 => {
            WasmTypeDef::Struct {
                name: typed_general_option_sym(mono),
                supertype: None,
                non_final: false,
                fields: vec![
                    WasmFieldDef {
                        name: Some("variant_id".to_string()),
                        mutable: false,
                        ty: ValType::I32,
                    },
                    WasmFieldDef {
                        name: Some("ok_payload".to_string()),
                        mutable: false,
                        ty: mono_to_valtype_specialized(&args[0], type_env, concrete_func_sigs),
                    },
                    WasmFieldDef {
                        name: Some("err_payload".to_string()),
                        mutable: false,
                        ty: mono_to_valtype_specialized(&args[1], type_env, concrete_func_sigs),
                    },
                ],
            }
        }
        _ => panic!(
            "emit_typed_general_option_struct_def: expected Option<T> or Result<T,E>, got {:?}",
            mono
        ),
    }
}

fn emit_typed_iter_option_struct_def(info: &IteratorStateInfo) -> WasmTypeDef {
    WasmTypeDef::Struct {
        name: typed_iter_option_sym(info),
        supertype: None,
        non_final: false,
        fields: vec![
            WasmFieldDef {
                name: Some("variant_id".to_string()),
                mutable: false,
                ty: ValType::I32,
            },
            WasmFieldDef {
                name: Some("payload".to_string()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named(typed_iter_item_sym(info)),
                },
            },
        ],
    }
}

fn prioritize_specialized_iterator_types(module: &mut ModuleIR) {
    fn type_priority(name: &str) -> u8 {
        if name.starts_with("unfold_step__") {
            0
        } else if name.starts_with("closurefunc_") || name.starts_with("closure_") {
            1
        } else if name.starts_with("iter_state__") {
            2
        } else if name.starts_with("iter_item__") {
            3
        } else if name.starts_with("option__iter_item__") {
            4
        } else {
            5
        }
    }
    module.types.sort_by_key(|ty| type_priority(ty.name()));
}

fn topologically_order_local_type_defs(module: &mut ModuleIR) {
    let original = std::mem::take(&mut module.types);
    let mut name_to_index = HashMap::new();
    for (idx, ty) in original.iter().enumerate() {
        name_to_index.insert(ty.name().to_string(), idx);
    }

    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); original.len()];
    let mut indegree = vec![0_usize; original.len()];
    for (idx, ty) in original.iter().enumerate() {
        let mut local_deps = HashSet::new();
        collect_local_type_deps(ty, &name_to_index, &mut local_deps);
        local_deps.remove(&idx);
        indegree[idx] = local_deps.len();
        for dep_idx in local_deps {
            dependents[dep_idx].push(idx);
        }
    }

    let mut ready = VecDeque::new();
    for (idx, degree) in indegree.iter().enumerate() {
        if *degree == 0 {
            ready.push_back(idx);
        }
    }

    let mut ordered = Vec::with_capacity(original.len());
    while let Some(idx) = ready.pop_front() {
        ordered.push(idx);
        for dependent in &dependents[idx] {
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.push_back(*dependent);
            }
        }
    }

    if ordered.len() != original.len() {
        let cycle = indegree
            .iter()
            .enumerate()
            .filter_map(|(idx, degree)| (*degree > 0).then(|| original[idx].name().to_string()))
            .collect::<Vec<_>>();
        panic!(
            "cyclic local Wasm type dependencies are not supported yet: {}",
            cycle.join(", ")
        );
    }

    module.types = ordered
        .into_iter()
        .map(|idx| original[idx].clone())
        .collect();
}

fn collect_local_type_deps(
    ty: &WasmTypeDef,
    name_to_index: &HashMap<String, usize>,
    out: &mut HashSet<usize>,
) {
    match ty {
        WasmTypeDef::Struct {
            fields, supertype, ..
        } => {
            if let Some(parent) = supertype {
                if let Some(dep_idx) = name_to_index.get(parent) {
                    out.insert(*dep_idx);
                }
            }
            for field in fields {
                collect_local_valtype_deps(&field.ty, name_to_index, out);
            }
        }
        WasmTypeDef::Array { elem, .. } => {
            collect_local_valtype_deps(&elem.ty, name_to_index, out);
        }
        WasmTypeDef::FuncType {
            params, results, ..
        } => {
            for param in params {
                collect_local_valtype_deps(param, name_to_index, out);
            }
            for result in results {
                collect_local_valtype_deps(result, name_to_index, out);
            }
        }
    }
}

fn collect_local_valtype_deps(
    ty: &ValType,
    name_to_index: &HashMap<String, usize>,
    out: &mut HashSet<usize>,
) {
    if let ValType::Ref {
        heap: HeapType::Named(name),
        ..
    } = ty
    {
        if let Some(dep_idx) = name_to_index.get(name) {
            out.insert(*dep_idx);
        }
    }
}

/// Emit a typed closure trampoline for `func`.
///
/// The trampoline signature is:
///   `(param (ref null $ClosureEnv)) (param p0_ty) (param p1_ty) ... (result ret_ty)`
///
/// It directly passes concrete args to the underlying user function, then
/// loads captures from the env array.  No anyref boxing/unboxing.
fn emit_typed_closure_trampoline(
    func: &AnfFunctionDef,
    capture_count: usize,
    params: &[crate::types::ty::MonoType],
    ret: &crate::types::ty::MonoType,
    ctx: &EmitCtx<'_>,
) -> FuncDef {
    let mut trampoline_params = vec![ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_CLOSURE_ENV.to_string()),
    }];
    trampoline_params.extend(
        params
            .iter()
            .map(|p| mono_to_valtype_for_user_abi_param(p, ctx.type_env, &ctx.concrete_func_sigs)),
    );

    let mut body = Vec::new();

    // Push concrete args (params 1..N in the trampoline — param 0 is env).
    for i in 0..params.len() {
        body.push(crate::wasm::ir::Instr::LocalGet((i + 1) as u32));
    }

    // Load captures from env (param 0).
    for capture_idx in 0..capture_count {
        body.push(crate::wasm::ir::Instr::LocalGet(0));
        body.push(crate::wasm::ir::Instr::RefCast {
            nullable: true,
            heap: HeapType::Named(T_CLOSURE_ENV.to_string()),
        });
        body.push(crate::wasm::ir::Instr::I32Const(capture_idx as i32));
        body.push(crate::wasm::ir::Instr::ArrayGet(T_CLOSURE_ENV.to_string()));
    }

    body.push(crate::wasm::ir::Instr::Call(user_func_sym(func.func_id)));

    // No result coercion needed — trampoline returns same type as user func.
    if matches!(
        ret,
        crate::types::ty::MonoType::Void | crate::types::ty::MonoType::Never
    ) {
        // Void functions: produce an i32 0 as the functype result (closurefunc for void
        // still returns nothing, handled by `mono_result_types`).
    }

    let results = ctx
        .user_func_abi(func.func_id)
        .map(|abi| abi.results)
        .unwrap_or_else(|| {
            panic!(
                "missing ABI for typed trampoline FuncId({})",
                func.func_id.0
            )
        });

    FuncDef {
        name: typed_closure_trampoline_sym(func.func_id),
        params: trampoline_params,
        results,
        locals: Vec::new(),
        body,
    }
}

/// Builds user function signature map, mapping concrete `MonoType::Function` params
/// to typed closure struct ValTypes instead of the universal `$Closure`.
fn build_user_sig_map_typed(
    anf: &AnfModule,
    type_env: &TypeEnv,
    closure_capture_layouts: &HashMap<FuncId, Vec<crate::ir::LocalId>>,
    concrete_func_sigs: &HashMap<
        FuncId,
        (Vec<crate::types::ty::MonoType>, crate::types::ty::MonoType),
    >,
) -> HashMap<FuncId, FuncSigInfo> {
    anf.functions
        .iter()
        .map(|func| {
            let capture_count = closure_capture_layouts
                .get(&func.func_id)
                .map_or(0, Vec::len);
            let mut params = func
                .param_tys
                .iter()
                .map(|ty| mono_to_valtype_for_user_abi_param(ty, type_env, concrete_func_sigs))
                .collect::<Vec<_>>();
            params.extend(vec![ValType::Anyref; capture_count]);
            let result = match &func.return_ty {
                crate::types::ty::MonoType::Void | crate::types::ty::MonoType::Never => None,
                other => Some(mono_to_valtype_for_user_abi_result(
                    other,
                    type_env,
                    concrete_func_sigs,
                )),
            };
            (
                func.func_id,
                FuncSigInfo {
                    params,
                    result,
                    result_mono: match &func.return_ty {
                        crate::types::ty::MonoType::Void | crate::types::ty::MonoType::Never => {
                            None
                        }
                        other => Some(other.clone()),
                    },
                },
            )
        })
        .collect()
}

fn collect_cell_payloads_from_mono(
    ty: &MonoType,
    out: &mut std::collections::BTreeMap<String, MonoType>,
) {
    match ty {
        MonoType::Named { type_id, args }
            if *type_id == crate::types::ty::CELL_TYPE_ID && args.len() == 1 =>
        {
            if is_concrete_mono_type(&args[0]) {
                out.entry(typed_cell_struct_sym(&args[0]))
                    .or_insert_with(|| args[0].clone());
            }
            collect_cell_payloads_from_mono(&args[0], out);
        }
        MonoType::Named { args, .. } => {
            for arg in args {
                collect_cell_payloads_from_mono(arg, out);
            }
        }
        MonoType::Vector(inner) => collect_cell_payloads_from_mono(inner, out),
        MonoType::Dict(k, v) => {
            collect_cell_payloads_from_mono(k, out);
            collect_cell_payloads_from_mono(v, out);
        }
        MonoType::Function { params, ret } => {
            for param in params {
                collect_cell_payloads_from_mono(param, out);
            }
            collect_cell_payloads_from_mono(ret, out);
        }
        MonoType::Int
        | MonoType::Float
        | MonoType::Bool
        | MonoType::Byte
        | MonoType::String
        | MonoType::Void
        | MonoType::Never
        | MonoType::Var(_)
        | MonoType::MetaVar(_) => {}
    }
}

fn collect_typed_cell_payloads_from_type_defs(
    type_env: &TypeEnv,
    out: &mut std::collections::BTreeMap<String, MonoType>,
) {
    let mut next_type_id = 0_u32;
    loop {
        let type_id = TypeId(next_type_id);
        let Some(def) = type_env.get_def(type_id) else {
            break;
        };
        match def {
            LangTypeDef::Record { fields, .. } => {
                for field in fields {
                    collect_cell_payloads_from_mono(&field.ty, out);
                }
            }
            LangTypeDef::Sum { variants, .. } => {
                for variant in variants {
                    for field_ty in &variant.fields {
                        collect_cell_payloads_from_mono(field_ty, out);
                    }
                }
            }
            LangTypeDef::Alias { target, .. } => {
                collect_cell_payloads_from_mono(target, out);
            }
        }
        next_type_id += 1;
    }
}

fn collect_typed_cell_payloads(
    anf: &AnfModule,
    type_env: &TypeEnv,
    closure_capture_layouts: &HashMap<FuncId, Vec<crate::ir::LocalId>>,
    ctx: &mut EmitCtx<'_>,
) -> std::collections::BTreeMap<String, MonoType> {
    let mut out = std::collections::BTreeMap::new();
    collect_typed_cell_payloads_from_type_defs(type_env, &mut out);
    for func in &anf.functions {
        for ty in func
            .param_tys
            .iter()
            .chain(std::iter::once(&func.return_ty))
        {
            collect_cell_payloads_from_mono(ty, &mut out);
        }
        let capture_locals = closure_capture_layouts
            .get(&func.func_id)
            .cloned()
            .unwrap_or_default();
        let extra_params = capture_locals
            .iter()
            .copied()
            .map(|local_id| (local_id, ValType::Anyref))
            .collect::<Vec<_>>();
        let _locals = ctx.setup_locals_with_extra(func, &extra_params);
        for mono in ctx.local_mono.values() {
            collect_cell_payloads_from_mono(mono, &mut out);
        }
        // Init-function cell locals can be intentionally erased from local_mono
        // for backend layout reasons; collect from op_result_mono as well so
        // typed cell payloads used by intrinsic emission are still declared.
        for mono in ctx.op_result_mono.values() {
            collect_cell_payloads_from_mono(mono, &mut out);
        }
    }
    out
}

fn collect_user_func_iterator_states(
    anf: &AnfModule,
    closure_capture_layouts: &HashMap<FuncId, Vec<crate::ir::LocalId>>,
    ctx: &mut EmitCtx<'_>,
) -> HashMap<FuncId, IteratorStateInfo> {
    let mut out = HashMap::new();
    for func in &anf.functions {
        let capture_locals = closure_capture_layouts
            .get(&func.func_id)
            .cloned()
            .unwrap_or_default();
        let extra_params = capture_locals
            .iter()
            .copied()
            .map(|local_id| (local_id, ValType::Anyref))
            .collect::<Vec<_>>();
        let _locals = ctx.setup_locals_with_extra(func, &extra_params);
        if let Some(info) = infer_expr_iterator_state(&func.body, ctx) {
            out.insert(func.func_id, info);
        }
    }
    out
}

fn infer_expr_iterator_state(expr: &AnfExpr, ctx: &mut EmitCtx<'_>) -> Option<IteratorStateInfo> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            let mut restores = Vec::new();
            let iter_info = iterator_state_from_inference_op(op, ctx);
            ctx.push_flow_iterator_binding(*local, iter_info, &mut restores);
            if let AnfOp::AAssign {
                local: target,
                value,
            } = op.as_ref()
            {
                let assign_iter = atom_iterator_state(value, ctx);
                ctx.push_flow_iterator_binding(*target, assign_iter, &mut restores);
            }
            let result = infer_expr_iterator_state(body, ctx);
            while let Some((local_id, prev)) = restores.pop() {
                ctx.restore_flow_iterator_binding(local_id, prev);
            }
            result
        }
        AnfExpr::Return(Some(atom)) | AnfExpr::Atom(atom) | AnfExpr::Break(Some(atom)) => {
            atom_iterator_state(atom, ctx)
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => None,
    }
}

fn iterator_state_from_inference_op(
    op: &AnfOp,
    ctx: &mut EmitCtx<'_>,
) -> Option<IteratorStateInfo> {
    match op {
        AnfOp::ACall { callee, args } => match callee {
            Atom::AGlobalFunc(func_id) if *func_id == prelude_ids::ITERATOR_UNFOLD => {
                iterator_state_from_unfold_args(args.first()?, args.get(1)?, ctx)
            }
            _ => None,
        },
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            let then_info = infer_expr_iterator_state(then_branch, ctx);
            let else_info = infer_expr_iterator_state(else_branch, ctx);
            match (then_info, else_info) {
                (Some(a), Some(b)) if a == b => Some(a),
                _ => None,
            }
        }
        AnfOp::AMatch { arms, .. } => {
            let mut state = None;
            for arm in arms {
                let arm_state = infer_expr_iterator_state(&arm.body, ctx)?;
                match &state {
                    None => state = Some(arm_state),
                    Some(existing) if *existing == arm_state => {}
                    Some(_) => return None,
                }
            }
            state
        }
        AnfOp::AInit { value } => atom_iterator_state(value, ctx),
        AnfOp::AAssign { .. }
        | AnfOp::ALoop { .. }
        | AnfOp::ABinOp { .. }
        | AnfOp::AUnOp { .. }
        | AnfOp::AMakeClosure { .. }
        | AnfOp::ARecord { .. }
        | AnfOp::ARecordGet { .. }
        | AnfOp::ARecordUpdate { .. }
        | AnfOp::AVariant { .. }
        | AnfOp::AArrayLit(_)
        | AnfOp::AIndex { .. }
        | AnfOp::ADefer(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::prelude::build_prelude_map;
    use crate::ir::{FieldId, LocalId, VariantId};
    use crate::types::ty::{MonoType, OPTION_TYPE_ID, RANGE_TYPE_ID};

    fn ref_user_record_null(type_id: TypeId) -> ValType {
        ValType::Ref {
            nullable: true,
            heap: HeapType::Named(user_record_type_sym(type_id)),
        }
    }

    fn instr_tree_any(instrs: &[Instr], pred: &impl Fn(&Instr) -> bool) -> bool {
        instrs.iter().any(|instr| {
            pred(instr)
                || match instr {
                    Instr::If {
                        then_body,
                        else_body,
                        ..
                    } => instr_tree_any(then_body, pred) || instr_tree_any(else_body, pred),
                    Instr::Block { body, .. } | Instr::Loop { body, .. } => {
                        instr_tree_any(body, pred)
                    }
                    _ => false,
                }
        })
    }

    #[test]
    fn emit_string_literal_uses_utf8_bytes() {
        let instrs = emit_string_literal_atom("Aé");
        assert_eq!(
            instrs,
            vec![
                Instr::I32Const(65),
                Instr::I32Const(195),
                Instr::I32Const(169),
                Instr::ArrayNewFixed(T_STRING.to_string(), 3),
            ]
        );
    }

    #[test]
    fn emit_string_literals_use_pooled_getter_and_dedup_by_bytes() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let instrs_a = emit_atom(&Atom::ALitStr("same".to_string()), None, &mut ctx);
        let instrs_b = emit_atom(&Atom::ALitStr("same".to_string()), None, &mut ctx);

        assert_eq!(ctx.requested_string_literals().len(), 1);
        let getter = ctx
            .requested_string_literals()
            .values()
            .next()
            .expect("pooled literal missing")
            .getter_sym
            .clone();
        assert_eq!(instrs_a, vec![Instr::Call(getter.clone())]);
        assert_eq!(instrs_b, vec![Instr::Call(getter)]);
    }

    #[test]
    fn emit_string_literal_pool_getter_lazy_initializes_global() {
        let mut literals = BTreeMap::new();
        literals.insert(
            b"ok".to_vec(),
            StringLiteralPoolEntry {
                global_sym: "__str_lit_global_6f6b".to_string(),
                getter_sym: "__str_lit_get_6f6b".to_string(),
            },
        );
        let getter = emit_string_literal_pool_getters(&literals)
            .pop()
            .expect("missing pooled getter");
        assert_eq!(getter.results, vec![ref_string()]);
        assert_eq!(
            getter.body.first(),
            Some(&Instr::GlobalGet("__str_lit_global_6f6b".to_string()))
        );
        assert_eq!(getter.body.get(1), Some(&Instr::RefIsNull));
        let Instr::If { then_body, .. } = getter.body.get(2).expect("missing lazy-init branch")
        else {
            panic!("expected lazy-init branch in pooled getter");
        };
        assert!(
            then_body
                .iter()
                .any(|instr| instr == &Instr::GlobalSet("__str_lit_global_6f6b".to_string()))
        );
    }

    #[test]
    fn emit_binop_int_add_lowers_to_i64_add() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let instrs = emit_binop(
            crate::syntax::ast::BinOp::Add,
            &Atom::ALocal(LocalId(1)),
            &Atom::ALocal(LocalId(2)),
            crate::ir::anf::OpKind::Int,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![Instr::LocalGet(0), Instr::LocalGet(1), Instr::I64Add]
        );
    }

    #[test]
    fn emit_bool_literal_to_anyref_uses_ref_i31() {
        let instrs = emit_bool_literal(true, Some(&ValType::Anyref));
        assert_eq!(instrs, vec![Instr::I32Const(1), Instr::RefI31]);
    }

    #[test]
    fn emit_local_int_to_anyref_boxes_with_boxed_int() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(7), (0, ValType::I64));

        let instrs = emit_atom(&Atom::ALocal(LocalId(7)), Some(&ValType::Anyref), &mut ctx);
        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::StructNew(T_BOXED_INT.to_string()),
            ]
        );
    }

    #[test]
    fn emit_local_anyref_to_int_unboxes_boxed_int() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(3), (2, ValType::Anyref));

        let instrs = emit_atom(&Atom::ALocal(LocalId(3)), Some(&ValType::I64), &mut ctx);
        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(2),
                Instr::RefCast {
                    nullable: false,
                    heap: HeapType::Named(T_BOXED_INT.to_string()),
                },
                Instr::StructGet(T_BOXED_INT.to_string(), 0),
            ]
        );
    }

    #[test]
    fn emit_runtime_prelude_call_int_to_string_adds_import_and_call() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        let entry = ctx
            .prelude
            .get(&prelude_ids::INT_TO_STRING)
            .cloned()
            .expect("missing prelude entry");

        let instrs = emit_prelude_call(
            prelude_ids::INT_TO_STRING,
            &entry,
            &[Atom::ALitInt(42)],
            &ref_string_null(),
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::I64Const(42),
                Instr::Call("rt_str__from_i64".to_string()),
            ]
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_str__from_i64"));
    }

    #[test]
    fn emit_array_append_intrinsic_lowers_to_push() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_pvec_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let entry = ctx
            .prelude
            .get(&prelude_ids::VECTOR_APPEND)
            .cloned()
            .expect("missing prelude entry");
        let instrs = emit_prelude_call(
            prelude_ids::VECTOR_APPEND,
            &entry,
            &[Atom::ALocal(LocalId(1)), Atom::ALocal(LocalId(2))],
            &ref_pvec_null(),
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::RefAsNonNull,
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::Call("rt_arr__push".to_string()),
            ]
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_arr__push"));
    }

    #[test]
    fn emit_unimplemented_intrinsic_uses_runtime_trap_not_compiler_panic() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        // Use a fake FuncId that doesn't match any implemented intrinsic
        let fake_id = FuncId(9999);
        let entry =
            crate::codegen::prelude::PreludeEntry::intrinsic("fake_unimplemented_intrinsic");
        let instrs = emit_prelude_call(fake_id, &entry, &[], &ValType::Anyref, &mut ctx);

        assert_eq!(instrs.last(), Some(&Instr::Unreachable));
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::Call(sym) if sym == "rt_core__trap"))
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_core__trap"));
    }

    #[test]
    fn emit_expr_value_return_uses_function_return_type_not_branch_value_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let expr = AnfExpr::Return(Some(Atom::ALitInt(7)));
        let instrs = emit_expr_value(&expr, &ValType::I32, Some(&ValType::I64), &mut ctx);
        assert_eq!(instrs, vec![Instr::I64Const(7), Instr::Return]);
    }

    #[test]
    fn emit_if_result_stays_i64_when_else_branch_returns_nested_local() {
        let type_env = TypeEnv::new();
        let fib_like = AnfFunctionDef {
            func_id: FuncId(99),
            name: "fib_like".to_string(),
            params: vec![LocalId(0)],
            param_tys: vec![MonoType::Int],
            body: AnfExpr::Let {
                local: LocalId(1),
                op: Box::new(AnfOp::ABinOp {
                    op: crate::syntax::ast::BinOp::Lt,
                    left: Atom::ALocal(LocalId(0)),
                    right: Atom::ALitInt(2),
                    operand_ty: crate::ir::anf::OpKind::Int,
                }),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(7),
                    op: Box::new(AnfOp::AIf {
                        cond: Atom::ALocal(LocalId(1)),
                        then_branch: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(0)))),
                        else_branch: Box::new(AnfExpr::Let {
                            local: LocalId(2),
                            op: Box::new(AnfOp::ABinOp {
                                op: crate::syntax::ast::BinOp::Sub,
                                left: Atom::ALocal(LocalId(0)),
                                right: Atom::ALitInt(1),
                                operand_ty: crate::ir::anf::OpKind::Int,
                            }),
                            body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
                        }),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(7)))),
                }),
            },
            return_ty: MonoType::Int,
            op_result_mono: HashMap::new(),
        };
        let anf = AnfModule {
            functions: vec![fib_like],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env);
        let func = module
            .funcs
            .iter()
            .find(|f| f.name == "func_99")
            .expect("missing emitted fib_like function");

        assert!(instr_tree_any(&func.body, &|i| matches!(
            i,
            Instr::If {
                result: Some(ValType::I64),
                ..
            }
        )));
        assert!(!instr_tree_any(
            &func.body,
            &|i| matches!(i, Instr::StructNew(name) if name == T_BOXED_INT)
        ));
    }

    #[test]
    fn emit_make_closure_boxes_free_vars_into_env() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(9),
            FuncSigInfo {
                params: vec![],
                result: Some(ValType::I64),
                result_mono: Some(MonoType::Int),
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_closure_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMakeClosure {
                func_id: FuncId(9),
                free_vars: vec![LocalId(2)],
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::RefFunc("func_9__closure".to_string()),
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::ArrayNewFixed(T_CLOSURE_ENV.to_string(), 1),
                Instr::StructNew(T_CLOSURE.to_string()),
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_make_closure_sorts_free_vars_for_env_layout() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(9),
            FuncSigInfo {
                params: vec![],
                result: Some(ValType::I64),
                result_mono: Some(MonoType::Int),
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));
        ctx.local_map.insert(LocalId(3), (2, ref_closure_null()));

        let instrs = emit_let_binding(
            LocalId(3),
            &AnfOp::AMakeClosure {
                func_id: FuncId(9),
                // Purposely out-of-order to verify sorting in emission.
                free_vars: vec![LocalId(2), LocalId(1)],
            },
            None,
            &mut ctx,
        );

        assert_eq!(instrs[1], Instr::LocalGet(0));
        assert_eq!(instrs[3], Instr::LocalGet(1));
    }

    #[test]
    fn emit_closure_call_boxes_args_and_uses_call_ref() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_closure_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let instrs = emit_call(
            &Atom::ALocal(LocalId(1)),
            &[Atom::ALocal(LocalId(2))],
            &ValType::I64,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::StructGet(T_CLOSURE.to_string(), 1),
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 1),
                Instr::LocalGet(0),
                Instr::StructGet(T_CLOSURE.to_string(), 0),
                Instr::CallRef(T_CLOSURE_FUNC.to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: HeapType::Named(T_BOXED_INT.to_string()),
                },
                Instr::StructGet(T_BOXED_INT.to_string(), 0),
            ]
        );
    }

    #[test]
    fn emit_closure_call_uses_local_backend_typed_closure_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(88),
            (vec![MonoType::Int], MonoType::Int),
        )]));
        ctx.local_map.insert(LocalId(1), (0, ref_closure_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));
        ctx.set_local_typed_closure_sig(LocalId(1), Some((vec![MonoType::Int], MonoType::Int)));

        let instrs = emit_call(
            &Atom::ALocal(LocalId(1)),
            &[Atom::ALocal(LocalId(2))],
            &ValType::I64,
            &mut ctx,
        );

        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::CallRef(sym) if sym == "closurefunc_i64_i64"))
        );
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::CallRef(sym) if sym == T_CLOSURE_FUNC))
        );
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::ArrayNewFixed(sym, 1) if sym == T_ARRAY))
        );
    }

    #[test]
    fn emit_closure_call_does_not_use_local_mono_fallback_without_backend_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(91),
            (vec![MonoType::Int], MonoType::Int),
        )]));
        ctx.local_map.insert(LocalId(1), (0, ref_closure_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));
        ctx.local_mono.insert(
            LocalId(1),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::Int),
            },
        );

        let instrs = emit_call(
            &Atom::ALocal(LocalId(1)),
            &[Atom::ALocal(LocalId(2))],
            &ValType::I64,
            &mut ctx,
        );

        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::CallRef(sym) if sym == T_CLOSURE_FUNC))
        );
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::ArrayNewFixed(sym, 1) if sym == T_ARRAY))
        );
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::CallRef(sym) if sym == "closurefunc_i64_i64"))
        );
    }

    #[test]
    fn emit_cell_get_uses_local_backend_typed_cell_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(89),
            (vec![MonoType::Int], MonoType::Int),
        )]));
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.set_local_typed_cell_elem(LocalId(1), Some(MonoType::Int));

        let instrs = emit_cell_get_intrinsic(&[Atom::ALocal(LocalId(1))], &ValType::I64, &mut ctx);
        let cell_sym = typed_cell_struct_sym(&MonoType::Int);
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::StructGet(sym, 0) if sym == &cell_sym))
        );
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::ArrayGet(sym) if sym == T_ARRAY))
        );
    }

    #[test]
    fn emit_cell_get_does_not_use_local_mono_fallback_without_backend_repr() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.set_concrete_func_sigs(HashMap::from([(
            FuncId(90),
            (vec![MonoType::Int], MonoType::Int),
        )]));
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_mono.insert(
            LocalId(1),
            MonoType::Named {
                type_id: crate::types::ty::CELL_TYPE_ID,
                args: vec![MonoType::Int],
            },
        );

        let instrs = emit_cell_get_intrinsic(&[Atom::ALocal(LocalId(1))], &ValType::I64, &mut ctx);
        let cell_sym = typed_cell_struct_sym(&MonoType::Int);
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::ArrayGet(sym) if sym == T_ARRAY))
        );
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::StructGet(sym, 0) if sym == &cell_sym))
        );
    }

    #[test]
    fn emit_tail_direct_user_call_uses_return_call() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(100),
            FuncSigInfo {
                params: vec![ValType::I64],
                result: Some(ValType::I64),
                result_mono: Some(MonoType::Int),
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let expr = AnfExpr::Let {
            local: LocalId(2),
            op: Box::new(AnfOp::ACall {
                callee: Atom::AGlobalFunc(FuncId(100)),
                args: vec![Atom::ALocal(LocalId(1))],
            }),
            body: Box::new(AnfExpr::Return(Some(Atom::ALocal(LocalId(2))))),
        };

        let instrs = emit_expr(&expr, Some(&ValType::I64), &mut ctx);
        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::ReturnCall("func_100".to_string())
            ]
        );
    }

    #[test]
    fn emit_tail_closure_call_uses_return_call_ref_when_return_is_anyref() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_closure_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));
        ctx.local_map.insert(LocalId(3), (2, ValType::Anyref));

        let expr = AnfExpr::Let {
            local: LocalId(3),
            op: Box::new(AnfOp::ACall {
                callee: Atom::ALocal(LocalId(1)),
                args: vec![Atom::ALocal(LocalId(2))],
            }),
            body: Box::new(AnfExpr::Return(Some(Atom::ALocal(LocalId(3))))),
        };

        let instrs = emit_expr(&expr, Some(&ValType::Anyref), &mut ctx);
        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::StructGet(T_CLOSURE.to_string(), 1),
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 1),
                Instr::LocalGet(0),
                Instr::StructGet(T_CLOSURE.to_string(), 0),
                Instr::ReturnCallRef(T_CLOSURE_FUNC.to_string()),
            ]
        );
    }

    #[test]
    fn emit_tail_call_falls_back_when_result_coercion_is_required() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(100),
            FuncSigInfo {
                params: vec![ValType::I64],
                result: Some(ValType::Anyref),
                result_mono: None,
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));

        let expr = AnfExpr::Let {
            local: LocalId(2),
            op: Box::new(AnfOp::ACall {
                callee: Atom::AGlobalFunc(FuncId(100)),
                args: vec![Atom::ALocal(LocalId(1))],
            }),
            body: Box::new(AnfExpr::Return(Some(Atom::ALocal(LocalId(2))))),
        };

        let instrs = emit_expr(&expr, Some(&ValType::I64), &mut ctx);
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::Call(sym) if sym == "func_100"))
        );
        assert!(instrs.iter().any(|i| matches!(i, Instr::Return)));
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::ReturnCall(_) | Instr::ReturnCallRef(_)))
        );
    }

    #[test]
    fn emit_expr_value_does_not_tail_call_non_tail_let_call() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(100),
            FuncSigInfo {
                params: vec![ValType::I64],
                result: Some(ValType::I64),
                result_mono: Some(MonoType::Int),
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let expr = AnfExpr::Let {
            local: LocalId(2),
            op: Box::new(AnfOp::ACall {
                callee: Atom::AGlobalFunc(FuncId(100)),
                args: vec![Atom::ALocal(LocalId(1))],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
        };

        let instrs = emit_expr_value(&expr, &ValType::I64, Some(&ValType::I64), &mut ctx);
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::Call(sym) if sym == "func_100"))
        );
        assert!(instrs.iter().any(|i| matches!(i, Instr::LocalSet(1))));
        assert!(instrs.iter().any(|i| matches!(i, Instr::LocalGet(1))));
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::ReturnCall(_) | Instr::ReturnCallRef(_)))
        );
    }

    #[test]
    fn emit_loop_body_expr_does_not_tail_call_non_tail_let_call() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(100),
            FuncSigInfo {
                params: vec![ValType::I64],
                result: Some(ValType::I64),
                result_mono: Some(MonoType::Int),
            },
        );
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let expr = AnfExpr::Let {
            local: LocalId(2),
            op: Box::new(AnfOp::ACall {
                callee: Atom::AGlobalFunc(FuncId(100)),
                args: vec![Atom::ALocal(LocalId(1))],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
        };

        let instrs = emit_loop_body_expr(&expr, Some(&ValType::I64), &mut ctx);
        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::Call(sym) if sym == "func_100"))
        );
        assert!(instrs.iter().any(|i| matches!(i, Instr::LocalSet(1))));
        assert!(instrs.iter().any(|i| matches!(i, Instr::LocalGet(1))));
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, Instr::ReturnCall(_) | Instr::ReturnCallRef(_)))
        );
    }

    #[test]
    fn emit_user_module_adds_closure_trampoline_for_user_funcs() {
        let type_env = TypeEnv::new();
        let func = AnfFunctionDef {
            func_id: FuncId(1),
            name: "id".to_string(),
            params: vec![LocalId(1)],
            param_tys: vec![MonoType::Int],
            body: AnfExpr::Atom(Atom::ALocal(LocalId(1))),
            return_ty: MonoType::Int,
            op_result_mono: HashMap::new(),
        };
        let anf = AnfModule {
            functions: vec![func],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env);
        assert!(module.funcs.iter().any(|f| f.name == "func_1"));
        let trampoline = module
            .funcs
            .iter()
            .find(|f| f.name == "func_1__closure")
            .expect("missing closure trampoline");
        assert_eq!(trampoline.params, vec![ValType::Anyref, ValType::Anyref]);
        assert_eq!(trampoline.results, vec![ValType::Anyref]);
        assert!(
            trampoline
                .body
                .iter()
                .any(|i| matches!(i, Instr::Call(name) if name == "func_1"))
        );
    }

    #[test]
    fn infer_capture_locals_finds_undeclared_local_refs() {
        let func = AnfFunctionDef {
            func_id: FuncId(2),
            name: "capturing".to_string(),
            params: vec![LocalId(1)],
            param_tys: vec![MonoType::Int],
            body: AnfExpr::Atom(Atom::ALocal(LocalId(42))),
            return_ty: MonoType::Int,
            op_result_mono: HashMap::new(),
        };

        let captures = infer_capture_locals(&func);
        assert_eq!(captures, vec![LocalId(42)]);
    }

    #[test]
    fn captured_closure_function_gets_hidden_anyref_param_and_trampoline_loads_env() {
        let type_env = TypeEnv::new();
        let callee = AnfFunctionDef {
            func_id: FuncId(2),
            name: "capturing".to_string(),
            params: vec![LocalId(1)],
            param_tys: vec![MonoType::Int],
            // Simulate a post-optimization body where the captured local is no longer read.
            body: AnfExpr::Atom(Atom::ALocal(LocalId(1))),
            return_ty: MonoType::Int,
            op_result_mono: HashMap::new(),
        };
        let caller = AnfFunctionDef {
            func_id: FuncId(3),
            name: "mk".to_string(),
            params: vec![],
            param_tys: vec![],
            body: AnfExpr::Let {
                local: LocalId(42),
                op: Box::new(AnfOp::AInit {
                    value: Atom::ALitInt(7),
                }),
                body: Box::new(AnfExpr::Let {
                    local: LocalId(100),
                    op: Box::new(AnfOp::AMakeClosure {
                        func_id: FuncId(2),
                        free_vars: vec![LocalId(42)],
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
                }),
            },
            return_ty: MonoType::Void,
            op_result_mono: HashMap::new(),
        };
        let anf = AnfModule {
            functions: vec![callee, caller],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env);
        let func_2 = module
            .funcs
            .iter()
            .find(|f| f.name == "func_2")
            .expect("missing user function");
        assert_eq!(func_2.params, vec![ValType::I64, ValType::Anyref]);

        let trampoline = module
            .funcs
            .iter()
            .find(|f| f.name == "func_2__closure")
            .expect("missing closure trampoline");
        assert!(
            trampoline
                .body
                .iter()
                .any(|i| matches!(i, Instr::ArrayGet(ty) if ty == T_CLOSURE_ENV))
        );
        assert!(
            trampoline
                .body
                .iter()
                .any(|i| matches!(i, Instr::Call(name) if name == "func_2"))
        );
    }

    #[test]
    fn emit_user_module_emits_user_record_type_defs() {
        let type_env = TypeEnv::new();
        let anf = AnfModule {
            functions: vec![],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env);
        assert!(module.types.iter().any(|t| matches!(
            t,
            WasmTypeDef::Struct { name, .. } if name == "UserRecord_3"
        )));
        assert!(module.types.iter().any(|t| matches!(
            t,
            WasmTypeDef::Struct { name, fields, .. }
                if name == "UserRecord_3"
                    && fields.len() == 3
                    && fields.iter().all(|field| field.ty == ValType::I64)
        )));
    }

    #[test]
    fn emit_user_module_registers_typed_cells_referenced_only_from_record_fields() {
        use crate::types::ty::{CELL_TYPE_ID, RecordField, TypeDef};

        let mut type_env = TypeEnv::new();
        let inner_id = type_env.add_type(TypeDef::Record {
            name: "Inner".to_string(),
            type_params: vec![],
            fields: vec![RecordField {
                name: "value".to_string(),
                ty: MonoType::Int,
            }],
            doc: None,
        });
        let wrapper_id = type_env.add_type(TypeDef::Record {
            name: "Wrapper".to_string(),
            type_params: vec![],
            fields: vec![RecordField {
                name: "slot".to_string(),
                ty: MonoType::Named {
                    type_id: CELL_TYPE_ID,
                    args: vec![MonoType::Named {
                        type_id: inner_id,
                        args: vec![],
                    }],
                },
            }],
            doc: None,
        });
        let anf = AnfModule {
            functions: vec![
                AnfFunctionDef {
                    func_id: FuncId(1),
                    name: "id".to_string(),
                    params: vec![LocalId(1)],
                    param_tys: vec![MonoType::Int],
                    body: AnfExpr::Atom(Atom::ALocal(LocalId(1))),
                    return_ty: MonoType::Int,
                    op_result_mono: HashMap::new(),
                },
                AnfFunctionDef {
                    func_id: FuncId(2),
                    name: "capture_id".to_string(),
                    params: vec![],
                    param_tys: vec![],
                    body: AnfExpr::Atom(Atom::AGlobalFunc(FuncId(1))),
                    return_ty: MonoType::Function {
                        params: vec![MonoType::Int],
                        ret: Box::new(MonoType::Int),
                    },
                    op_result_mono: HashMap::new(),
                },
            ],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env);
        let typed_cell_sym = typed_cell_struct_sym(&MonoType::Named {
            type_id: inner_id,
            args: vec![],
        });

        assert!(module.types.iter().any(|t| matches!(
            t,
            WasmTypeDef::Struct { name, .. } if name == &typed_cell_sym
        )));
        assert!(module.types.iter().any(|t| matches!(
            t,
            WasmTypeDef::Struct { name, fields, .. }
                if name == &user_record_type_sym(wrapper_id)
                    && fields.iter().any(|field| field.ty == ValType::Ref {
                        nullable: true,
                        heap: HeapType::Named(typed_cell_sym.clone()),
                    })
        )));
    }

    #[test]
    fn emit_record_get_reads_typed_i64_field_directly() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map
            .insert(LocalId(2), (1, ref_user_record_null(RANGE_TYPE_ID)));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::ARecordGet {
                target: Atom::ALocal(LocalId(2)),
                field: FieldId(0),
                type_id: RANGE_TYPE_ID,
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(1),
                Instr::StructGet("UserRecord_3".to_string(), 0),
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_record_update_copy_rebuilds_struct() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map
            .insert(LocalId(1), (0, ref_user_record_null(RANGE_TYPE_ID)));
        ctx.local_map
            .insert(LocalId(2), (1, ref_user_record_null(RANGE_TYPE_ID)));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::ARecordUpdate {
                base: Atom::ALocal(LocalId(2)),
                field: FieldId(1),
                value: Atom::ALitInt(9),
                can_reuse_in_place: false,
                type_id: RANGE_TYPE_ID,
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(1),
                Instr::StructGet("UserRecord_3".to_string(), 0),
                Instr::I64Const(9),
                Instr::LocalGet(1),
                Instr::StructGet("UserRecord_3".to_string(), 2),
                Instr::StructNew("UserRecord_3".to_string()),
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_user_module_preserves_function_record_field_types() {
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/run/capability_records.tw")
            .to_string_lossy()
            .to_string();
        let wat = crate::cli::build::build_wat(&path).expect("build_wat failed");

        assert!(
            wat.contains("(field $f0 (mut (ref null $user__closure_i64_void)))")
                && wat.contains("(field $f0 (mut (ref null $user__closure_str_void)))"),
            "expected capability record fields to preserve both typed closure refs:\n{wat}"
        );
    }

    #[test]
    fn emit_variant_literal_boxes_payload_and_constructs_variant() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_variant()));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AVariant {
                type_id: OPTION_TYPE_ID,
                variant: VariantId(1),
                args: vec![Atom::ALitInt(7)],
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::I32Const(0),
                Instr::I32Const(1),
                Instr::I64Const(7),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 1),
                Instr::StructNew(T_VARIANT.to_string()),
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_array_lit_boxes_elements() {
        use crate::runtime::types::T_VEC_INTERNAL;
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_pvec_null()));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AArrayLit(vec![Atom::ALitInt(1), Atom::ALitBool(true)]),
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::I32Const(2), // len
                Instr::I32Const(0), // shift
                Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.to_string())),
                Instr::I64Const(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::I32Const(1),
                Instr::RefI31,
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 2),
                Instr::StructNew(T_PVEC.to_string()),
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_index_array_calls_runtime_get_and_unboxes() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ref_pvec_null()));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AIndex {
                base: Atom::ALocal(LocalId(2)),
                index: Atom::ALitInt(3),
                base_ty: crate::ir::anf::IndexKind::Array,
                result_ty: MonoType::Int,
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(1),
                Instr::I64Const(3),
                Instr::I32WrapI64,
                Instr::Call("rt_arr__get".to_string()),
                Instr::RefCast {
                    nullable: false,
                    heap: HeapType::Named(T_BOXED_INT.to_string()),
                },
                Instr::StructGet(T_BOXED_INT.to_string(), 0),
                Instr::LocalSet(0),
            ]
        );
        assert!(ctx.imports().iter().any(|i| i.as_sym == "rt_arr__get"));
    }

    #[test]
    fn emit_index_dict_calls_runtime_get() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ref_pdict_null()));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AIndex {
                base: Atom::ALocal(LocalId(2)),
                index: Atom::ALitStr("k".to_string()),
                base_ty: crate::ir::anf::IndexKind::Dict,
                result_ty: MonoType::Int,
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(1),
                Instr::Call("__str_lit_get_6b".to_string()),
                Instr::Call("rt_dict__get_option".to_string()),
                Instr::LocalSet(0),
            ]
        );
        assert!(
            ctx.imports()
                .iter()
                .any(|i| i.as_sym == "rt_dict__get_option")
        );
    }

    #[test]
    fn emit_loop_with_break_value_lowers_to_block_and_loop() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(Some(Atom::ALitInt(5)))),
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::Block {
                    label: "break_0".to_string(),
                    result: Some(ValType::I64),
                    body: vec![
                        Instr::Loop {
                            label: "cont_0".to_string(),
                            result: None,
                            body: vec![
                                Instr::I64Const(5),
                                Instr::Br("break_0".to_string()),
                                Instr::Br("cont_0".to_string()),
                            ],
                        },
                        Instr::Unreachable,
                    ],
                },
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_loop_with_break_none_materializes_default_result() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(None)),
            },
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::Block {
                    label: "break_0".to_string(),
                    result: Some(ValType::Anyref),
                    body: vec![
                        Instr::Loop {
                            label: "cont_0".to_string(),
                            result: None,
                            body: vec![
                                Instr::RefNull(HeapType::None),
                                Instr::Br("break_0".to_string()),
                                Instr::Br("cont_0".to_string()),
                            ],
                        },
                        Instr::Unreachable,
                    ],
                },
                Instr::LocalSet(0),
            ]
        );
    }

    #[test]
    fn emit_match_int_literal_chain_uses_if_and_i64_eq() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMatch {
                scrutinee: Atom::ALocal(LocalId(2)),
                arms: vec![
                    AnfMatchArm {
                        pattern: CorePattern::LitInt(1),
                        body: AnfExpr::Atom(Atom::ALitInt(10)),
                    },
                    AnfMatchArm {
                        pattern: CorePattern::Wildcard,
                        body: AnfExpr::Atom(Atom::ALitInt(20)),
                    },
                ],
            },
            None,
            &mut ctx,
        );

        assert!(instr_tree_any(&instrs, &|i| matches!(i, Instr::I64Eq)));
        assert!(instrs.iter().any(|i| matches!(
            i,
            Instr::If {
                result: Some(ValType::I64),
                ..
            }
        )));
    }

    #[test]
    fn emit_let_expr_match_option_seeds_typed_sum_metadata() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let option_int = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Int],
        };

        // let-bound AMatch result local + two branch locals that both carry Option<Int>.
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));
        ctx.local_map.insert(LocalId(3), (2, ValType::Anyref));
        ctx.local_mono.insert(LocalId(2), option_int.clone());
        ctx.local_mono.insert(LocalId(3), option_int.clone());

        let op = AnfOp::AMatch {
            scrutinee: Atom::ALitBool(true),
            arms: vec![
                AnfMatchArm {
                    pattern: CorePattern::LitBool(true),
                    body: AnfExpr::Let {
                        local: LocalId(2),
                        op: Box::new(AnfOp::AVariant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(1),
                            args: vec![Atom::ALitInt(7)],
                        }),
                        body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
                    },
                },
                AnfMatchArm {
                    pattern: CorePattern::Wildcard,
                    body: AnfExpr::Let {
                        local: LocalId(3),
                        op: Box::new(AnfOp::AVariant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(1),
                            args: vec![Atom::ALitInt(8)],
                        }),
                        body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(3)))),
                    },
                },
            ],
        };

        let mut observed_sum_repr: Option<SumRepr> = None;
        let mut body_instrs: Vec<Instr> = Vec::new();
        let body = AnfExpr::Atom(Atom::ALitVoid);
        let _instrs = emit_let_expr(LocalId(1), &op, &body, None, &mut ctx, |ctx, _| {
            observed_sum_repr = ctx.local_sum_repr(LocalId(1)).cloned();
            body_instrs = emit_atom(&Atom::ALocal(LocalId(1)), Some(&ValType::Anyref), ctx);
            body_instrs.clone()
        });

        assert!(
            matches!(
                observed_sum_repr,
                Some(SumRepr::TypedOption(MonoType::Named { type_id, ref args }))
                if type_id == OPTION_TYPE_ID && args.as_slice() == [MonoType::Int]
            ),
            "expected AMatch let-binding to seed TypedOption metadata, got {:?}",
            observed_sum_repr
        );
        assert!(
            instr_tree_any(&body_instrs, &|i| matches!(
                i,
                Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(name),
                } if name == T_VARIANT
            )),
            "anyref AMatch typed-sum local should runtime-dispatch between erased Variant and typed Option reprs: {:?}",
            body_instrs
        );
    }

    #[test]
    fn typed_general_option_from_match_supports_nested_if_sources() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let nested_if_some = |target: LocalId, lhs: i64, rhs: i64| AnfExpr::Let {
            local: target,
            op: Box::new(AnfOp::AIf {
                cond: Atom::ALitBool(true),
                then_branch: Box::new(AnfExpr::Let {
                    local: LocalId(target.0 + 10),
                    op: Box::new(AnfOp::AVariant {
                        type_id: OPTION_TYPE_ID,
                        variant: VariantId(1),
                        args: vec![Atom::ALitInt(lhs)],
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(target.0 + 10)))),
                }),
                else_branch: Box::new(AnfExpr::Let {
                    local: LocalId(target.0 + 20),
                    op: Box::new(AnfOp::AVariant {
                        type_id: OPTION_TYPE_ID,
                        variant: VariantId(1),
                        args: vec![Atom::ALitInt(rhs)],
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(target.0 + 20)))),
                }),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(target))),
        };

        let op = AnfOp::AMatch {
            scrutinee: Atom::ALitBool(true),
            arms: vec![
                AnfMatchArm {
                    pattern: CorePattern::LitBool(true),
                    body: nested_if_some(LocalId(1), 1, 2),
                },
                AnfMatchArm {
                    pattern: CorePattern::Wildcard,
                    body: nested_if_some(LocalId(2), 3, 4),
                },
            ],
        };
        let option_int = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Int],
        };
        ctx.op_result_mono.insert(LocalId(1), option_int.clone());
        ctx.op_result_mono.insert(LocalId(2), option_int.clone());
        ctx.op_result_mono.insert(LocalId(99), option_int);

        assert!(
            matches!(
                typed_general_option_from_op(LocalId(99), &op, &ctx),
                Some(MonoType::Named { type_id, ref args })
                if type_id == OPTION_TYPE_ID && args.as_slice() == [MonoType::Int]
            ),
            "expected nested AIf Option sources in AMatch arms to preserve typed Option metadata"
        );
    }

    #[test]
    fn emit_if_typed_option_binding_avoids_erased_variant_boundary() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let option_int = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Int],
        };

        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));
        ctx.local_map.insert(LocalId(3), (2, ValType::Anyref));
        ctx.op_result_mono.insert(LocalId(1), option_int.clone());
        ctx.op_result_mono.insert(LocalId(2), option_int.clone());
        ctx.op_result_mono.insert(LocalId(3), option_int.clone());

        let op = AnfOp::AIf {
            cond: Atom::ALitBool(true),
            then_branch: Box::new(AnfExpr::Let {
                local: LocalId(2),
                op: Box::new(AnfOp::AVariant {
                    type_id: OPTION_TYPE_ID,
                    variant: VariantId(1),
                    args: vec![Atom::ALitInt(7)],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(2)))),
            }),
            else_branch: Box::new(AnfExpr::Let {
                local: LocalId(3),
                op: Box::new(AnfOp::AVariant {
                    type_id: OPTION_TYPE_ID,
                    variant: VariantId(1),
                    args: vec![Atom::ALitInt(8)],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(LocalId(3)))),
            }),
        };

        let mut observed_sum_repr: Option<SumRepr> = None;
        let body = AnfExpr::Atom(Atom::ALitVoid);
        let instrs = emit_let_expr(LocalId(1), &op, &body, None, &mut ctx, |ctx, _| {
            observed_sum_repr = ctx.local_sum_repr(LocalId(1)).cloned();
            vec![]
        });

        assert!(
            matches!(
                observed_sum_repr,
                Some(SumRepr::TypedOption(MonoType::Named { type_id, ref args }))
                if type_id == OPTION_TYPE_ID && args.as_slice() == [MonoType::Int]
            ),
            "expected AIf Option let-binding to seed TypedOption metadata, got {:?}",
            observed_sum_repr
        );
        assert!(
            !instr_tree_any(&instrs, &|i| matches!(
                i,
                Instr::RefTest {
                    nullable: true,
                    heap: HeapType::Named(name),
                } if name == T_VARIANT
            )),
            "typed Option-producing if should not emit Option->Variant dispatch in branch join: {:?}",
            instrs
        );
    }

    #[test]
    fn emit_match_variant_binds_payload_var_local() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ref_variant_null()));
        ctx.local_map.insert(LocalId(3), (2, ValType::Anyref));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMatch {
                scrutinee: Atom::ALocal(LocalId(2)),
                arms: vec![
                    AnfMatchArm {
                        pattern: CorePattern::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(1),
                            fields: vec![CorePattern::Var(LocalId(3))],
                        },
                        body: AnfExpr::Atom(Atom::ALocal(LocalId(3))),
                    },
                    AnfMatchArm {
                        pattern: CorePattern::Wildcard,
                        body: AnfExpr::Atom(Atom::ALitVoid),
                    },
                ],
            },
            None,
            &mut ctx,
        );

        assert!(instr_tree_any(
            &instrs,
            &|i| matches!(i, Instr::ArrayGet(ty) if ty == T_ARRAY)
        ));
        assert!(instr_tree_any(&instrs, &|i| matches!(
            i,
            Instr::LocalSet(2)
        )));
    }

    #[test]
    fn emit_match_empty_arms_uses_runtime_trap() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMatch {
                scrutinee: Atom::ALocal(LocalId(2)),
                arms: vec![],
            },
            None,
            &mut ctx,
        );

        assert!(
            instrs
                .iter()
                .any(|i| matches!(i, Instr::Call(sym) if sym == "rt_core__trap"))
        );
        assert_eq!(instrs.last(), Some(&Instr::Unreachable));
        assert!(!instrs.iter().any(|i| matches!(i, Instr::LocalSet(0))));
        assert!(ctx.imports().iter().any(|i| i.as_sym == "rt_core__trap"));
    }

    #[test]
    fn emit_match_empty_arms_trap_includes_current_function_context() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.current_func_id = Some(FuncId(42));
        ctx.current_func_name = Some("find_id_by_span".to_string());
        ctx.local_map.insert(LocalId(1), (0, ValType::Anyref));
        ctx.local_map.insert(LocalId(2), (1, ValType::Anyref));

        let _instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMatch {
                scrutinee: Atom::ALocal(LocalId(2)),
                arms: vec![],
            },
            None,
            &mut ctx,
        );

        let expected = b"non-exhaustive match in find_id_by_span (FuncId(42))".to_vec();
        assert!(
            ctx.requested_string_literals().contains_key(&expected),
            "expected trap message to include function context; literals={:?}",
            ctx.requested_string_literals().keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn emit_match_all_diverging_arms_emits_if_without_result_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AMatch {
                scrutinee: Atom::ALocal(LocalId(2)),
                arms: vec![AnfMatchArm {
                    pattern: CorePattern::Wildcard,
                    body: AnfExpr::Return(None),
                }],
            },
            None,
            &mut ctx,
        );

        assert!(instr_tree_any(&instrs, &|i| matches!(
            i,
            Instr::If { result: None, .. }
        )));
        assert!(!instrs.iter().any(|i| matches!(i, Instr::LocalSet(0))));
    }

    #[test]
    fn emit_if_all_diverging_branches_emits_if_without_result_type() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ValType::I64));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AIf {
                cond: Atom::ALitBool(true),
                then_branch: Box::new(AnfExpr::Return(None)),
                else_branch: Box::new(AnfExpr::Return(None)),
            },
            None,
            &mut ctx,
        );

        assert!(instr_tree_any(&instrs, &|i| matches!(
            i,
            Instr::If { result: None, .. }
        )));
        assert!(!instrs.iter().any(|i| matches!(i, Instr::LocalSet(0))));
    }

    #[test]
    fn emit_coerce_stack_supports_i32_i64_numeric_widening() {
        assert_eq!(
            emit_coerce_stack(&ValType::I32, &ValType::I64),
            vec![Instr::I64ExtendI32S]
        );
    }

    #[test]
    fn emit_user_module_synthesizes_user_init_start() {
        let type_env = TypeEnv::new();
        let anf = AnfModule {
            functions: vec![
                AnfFunctionDef {
                    func_id: FuncId(1),
                    name: "a".to_string(),
                    params: vec![],
                    param_tys: vec![],
                    body: AnfExpr::Atom(Atom::ALitVoid),
                    return_ty: MonoType::Void,
                    op_result_mono: HashMap::new(),
                },
                AnfFunctionDef {
                    func_id: FuncId(2),
                    name: "b".to_string(),
                    params: vec![],
                    param_tys: vec![],
                    body: AnfExpr::Atom(Atom::ALitVoid),
                    return_ty: MonoType::Void,
                    op_result_mono: HashMap::new(),
                },
            ],
            init_func_id: Some(FuncId(2)),
            all_init_func_ids: vec![FuncId(1), FuncId(2)],
        };

        let module = emit_user_module(&anf, &type_env);
        assert_eq!(module.start.as_deref(), Some("__user_init"));
        let init = module
            .funcs
            .iter()
            .find(|f| f.name == "__user_init")
            .expect("missing __user_init");
        assert_eq!(
            init.body,
            vec![
                Instr::Call("func_1".to_string()),
                Instr::Call("func_2".to_string())
            ]
        );
    }
}
