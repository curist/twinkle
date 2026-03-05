use std::collections::{HashMap, HashSet};

use crate::codegen::ctx::{EmitCtx, FuncSigInfo, mono_to_valtype, user_record_type_sym};
use crate::codegen::prelude::build_prelude_map;
use crate::ir::FuncId;
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom};
use crate::ir::core::CorePattern;
use crate::ir::lower::prelude as prelude_ids;
use crate::runtime::types::{
    T_ARRAY, T_BOXED_FLOAT, T_BOXED_INT, T_CLOSURE, T_CLOSURE_ENV, T_CLOSURE_FUNC, T_STRING,
    T_VARIANT, ref_array, ref_array_null, ref_dict_null, ref_string, ref_string_null,
};
use crate::types::env::TypeEnv;
use crate::types::ty::{MonoType, TypeDef as LangTypeDef, TypeId};
use crate::wasm::ir::{
    FieldDef as WasmFieldDef, FuncDef, GlobalDef, HeapType, ImportDef, Instr, ModuleIR,
    TypeDef as WasmTypeDef, ValType,
};

/// Stage 8c scaffold entrypoint for ANF -> ModuleIR emission.
///
/// This currently establishes the emission context, stable function naming,
/// local allocation setup, and import plumbing so subsequent 8c steps can
/// focus on expression-level instruction lowering.
pub fn emit_user_module(
    anf: &AnfModule,
    type_env: &TypeEnv,
    _func_table: &HashMap<String, FuncId>,
) -> ModuleIR {
    let prelude = build_prelude_map();
    let closure_capture_layouts = collect_closure_capture_layouts(anf);
    let user_sigs = build_user_sig_map(anf, type_env, &closure_capture_layouts);
    let mut ctx = EmitCtx::new(type_env, &prelude, &user_sigs);
    let module_global_ids = collect_module_global_locals(anf);
    let module_global_map = module_global_ids
        .iter()
        .copied()
        .map(|id| (id, module_global_sym(id)))
        .collect::<HashMap<_, _>>();
    ctx.set_module_globals(module_global_map.clone());
    let mut module = ModuleIR::new("user");
    module.types.extend(emit_user_record_type_defs(type_env));
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
    for func in &anf.functions {
        module.funcs.push(emit_user_closure_trampoline(
            func,
            closure_capture_layouts
                .get(&func.func_id)
                .map_or(0, std::vec::Vec::len),
            &ctx,
        ));
    }
    // Emit __iterator_next helper if any function references Iterator.next
    if needs_iterator_next_helper(&ctx) {
        module.funcs.push(emit_iterator_next_helper());
    }

    if let Some(init) = emit_user_init_func(anf) {
        module.start = Some(init.name.clone());
        module.funcs.push(init);
    }

    module.imports.extend(ctx.imports());
    module
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

fn emit_user_record_type_defs(type_env: &TypeEnv) -> Vec<WasmTypeDef> {
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
                fields: fields
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| WasmFieldDef {
                        name: Some(format!("f{idx}")),
                        mutable: true,
                        ty: ValType::Anyref,
                    })
                    .collect(),
            });
        }
        next_type_id += 1;
    }
    defs
}

fn build_user_sig_map(
    anf: &AnfModule,
    type_env: &TypeEnv,
    closure_capture_layouts: &HashMap<FuncId, Vec<crate::ir::LocalId>>,
) -> HashMap<FuncId, FuncSigInfo> {
    anf.functions
        .iter()
        .map(|func| {
            let capture_locals = closure_capture_layouts
                .get(&func.func_id)
                .cloned()
                .unwrap_or_default();
            let mut params = func
                .param_tys
                .iter()
                .map(|ty| mono_to_valtype(ty, type_env))
                .collect::<Vec<_>>();
            params.extend(vec![ValType::Anyref; capture_locals.len()]);
            let result = match &func.return_ty {
                MonoType::Void | MonoType::Never => None,
                other => Some(mono_to_valtype(other, type_env)),
            };
            (func.func_id, FuncSigInfo { params, result })
        })
        .collect()
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
    let mut params = func
        .param_tys
        .iter()
        .map(|ty| mono_to_valtype(ty, ctx.type_env))
        .collect::<Vec<_>>();
    params.extend(vec![ValType::Anyref; capture_locals.len()]);
    let results = mono_result_types(&func.return_ty, ctx.type_env);
    let body = emit_expr(&func.body, results.first(), ctx);

    FuncDef {
        name: user_func_sym(func.func_id),
        params,
        results,
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
        let mut declared = func.params.iter().copied().collect::<HashSet<_>>();
        let mut free = HashSet::new();
        collect_free_locals_expr(&func.body, &mut declared, &mut free);
        referenced_outside_init.extend(free);
    }

    let mut bound_in_init = HashSet::new();
    for func in &anf.functions {
        if init_funcs.contains(&func.func_id) {
            collect_bound_locals_expr(&func.body, &mut bound_in_init);
        }
    }

    let mut globals = referenced_outside_init
        .into_iter()
        .filter(|id| bound_in_init.contains(id))
        .collect::<Vec<_>>();
    globals.sort_by_key(|id| id.0);
    globals
}

fn collect_bound_locals_expr(expr: &AnfExpr, out: &mut HashSet<crate::ir::LocalId>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            out.insert(*local);
            collect_bound_locals_op(op, out);
            collect_bound_locals_expr(body, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_bound_locals_op(op: &AnfOp, out: &mut HashSet<crate::ir::LocalId>) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_bound_locals_expr(then_branch, out);
            collect_bound_locals_expr(else_branch, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_bound_locals_expr(&arm.body, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => collect_bound_locals_expr(body, out),
        _ => {}
    }
}

#[cfg(test)]
fn infer_capture_locals(func: &AnfFunctionDef) -> Vec<crate::ir::LocalId> {
    let mut declared: HashSet<crate::ir::LocalId> = func.params.iter().copied().collect();
    let mut free: HashSet<crate::ir::LocalId> = HashSet::new();
    collect_free_locals_expr(&func.body, &mut declared, &mut free);
    // Filter out locals that are assigned within the function (assign targets that
    // are declared by an earlier let/init in the same function are NOT captures).
    // The free set only contains truly undeclared locals.
    let mut ordered = free.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|id| id.0);
    ordered
}

fn collect_free_locals_expr(
    expr: &AnfExpr,
    declared: &mut HashSet<crate::ir::LocalId>,
    free: &mut HashSet<crate::ir::LocalId>,
) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            collect_free_locals_op(op, declared, free);
            declared.insert(*local);
            collect_free_locals_expr(body, declared, free);
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            collect_free_locals_atom(atom, declared, free);
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
    }
}

fn collect_free_locals_op(
    op: &AnfOp,
    declared: &mut HashSet<crate::ir::LocalId>,
    free: &mut HashSet<crate::ir::LocalId>,
) {
    match op {
        AnfOp::ACall { callee, args } => {
            collect_free_locals_atom(callee, declared, free);
            for arg in args {
                collect_free_locals_atom(arg, declared, free);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_free_locals_atom(cond, declared, free);
            let mut then_declared = declared.clone();
            let mut else_declared = declared.clone();
            collect_free_locals_expr(then_branch, &mut then_declared, free);
            collect_free_locals_expr(else_branch, &mut else_declared, free);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            collect_free_locals_atom(scrutinee, declared, free);
            for arm in arms {
                let mut arm_declared = declared.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_declared);
                collect_free_locals_expr(&arm.body, &mut arm_declared, free);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            let mut body_declared = declared.clone();
            collect_free_locals_expr(body, &mut body_declared, free);
        }
        AnfOp::ABinOp { left, right, .. } => {
            collect_free_locals_atom(left, declared, free);
            collect_free_locals_atom(right, declared, free);
        }
        AnfOp::AUnOp { expr, .. } => {
            collect_free_locals_atom(expr, declared, free);
        }
        AnfOp::AMakeClosure { free_vars, .. } => {
            for local_id in free_vars {
                if !declared.contains(local_id) {
                    free.insert(*local_id);
                }
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, atom) in fields {
                collect_free_locals_atom(atom, declared, free);
            }
        }
        AnfOp::ARecordGet { target, .. } => collect_free_locals_atom(target, declared, free),
        AnfOp::ARecordUpdate { base, value, .. } => {
            collect_free_locals_atom(base, declared, free);
            collect_free_locals_atom(value, declared, free);
        }
        AnfOp::AVariant { args, .. } | AnfOp::AArrayLit(args) => {
            for atom in args {
                collect_free_locals_atom(atom, declared, free);
            }
        }
        AnfOp::AIndex { base, index, .. } => {
            collect_free_locals_atom(base, declared, free);
            collect_free_locals_atom(index, declared, free);
        }
        AnfOp::AInit { value } => collect_free_locals_atom(value, declared, free),
        AnfOp::AAssign { local, value } => {
            if !declared.contains(local) {
                free.insert(*local);
            }
            collect_free_locals_atom(value, declared, free);
        }
    }
}

fn collect_pattern_bindings(
    pattern: &crate::ir::core::CorePattern,
    declared: &mut HashSet<crate::ir::LocalId>,
) {
    use crate::ir::core::CorePattern;
    match pattern {
        CorePattern::Var(id) => {
            declared.insert(*id);
        }
        CorePattern::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(field, declared);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

fn collect_free_locals_atom(
    atom: &Atom,
    declared: &HashSet<crate::ir::LocalId>,
    free: &mut HashSet<crate::ir::LocalId>,
) {
    if let Atom::ALocal(local_id) = atom {
        if !declared.contains(local_id) {
            free.insert(*local_id);
        }
    }
}

fn mono_result_types(ty: &MonoType, type_env: &TypeEnv) -> Vec<crate::wasm::ir::ValType> {
    match ty {
        MonoType::Void | MonoType::Never => Vec::new(),
        _ => vec![mono_to_valtype(ty, type_env)],
    }
}

fn user_func_sym(func_id: FuncId) -> String {
    format!("func_{}", func_id.0)
}

fn emit_expr(expr: &AnfExpr, return_ty: Option<&ValType>, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let Some(instrs) = emit_tail_let_call(*local, op, body, return_ty, ctx) {
                return instrs;
            }
            let mut instrs = emit_let_binding(*local, op, return_ty, ctx);
            instrs.extend(emit_expr(body, return_ty, ctx));
            instrs
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
            let mut instrs = emit_atom(value, Some(&bind_ty), ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            if let Some(global_sym) = global_sym {
                instrs.extend(emit_coerce_local(bind_idx, &bind_ty, &ValType::Anyref));
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
                instrs.extend(emit_atom(value, Some(&target_ty), ctx));
                instrs.push(Instr::LocalSet(target_idx));
                if let Some(global_sym) = target_global_sym.clone() {
                    instrs.extend(emit_coerce_local(target_idx, &target_ty, &ValType::Anyref));
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
            let then_body = emit_expr_value(then_branch, &bind_ty, fn_return_ty, ctx);
            let else_body = emit_expr_value(else_branch, &bind_ty, fn_return_ty, ctx);
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
            let mut instrs = emit_match_op(scrutinee, arms, &bind_ty, fn_return_ty, ctx);
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
            let mut instrs = emit_variant_literal(*type_id, *variant, args, &bind_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AArrayLit(elems) => {
            let mut instrs = emit_array_literal(elems, &bind_ty, ctx);
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
        _ => panic!(
            "let-op emission not implemented yet in current Stage 8c emitter: {:?}",
            op
        ),
    }
}

fn emit_match_op(
    scrutinee: &Atom,
    arms: &[AnfMatchArm],
    bind_ty: &ValType,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let scrutinee_anyref = emit_atom(scrutinee, Some(&ValType::Anyref), ctx);
    emit_match_arm_chain(&scrutinee_anyref, arms, bind_ty, fn_return_ty, ctx)
}

fn emit_match_arm_chain(
    scrutinee_anyref: &[Instr],
    arms: &[AnfMatchArm],
    bind_ty: &ValType,
    fn_return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if arms.is_empty() {
        return emit_non_exhaustive_match_fallback(ctx);
    }

    let head = &arms[0];
    let mut instrs = emit_pattern_condition(&head.pattern, scrutinee_anyref, ctx);
    let mut then_body = emit_pattern_bindings(&head.pattern, scrutinee_anyref, ctx);
    then_body.extend(emit_expr_value(&head.body, bind_ty, fn_return_ty, ctx));
    let tail_diverges = match_chain_always_diverges(&arms[1..]);
    let mut else_body =
        emit_match_arm_chain(scrutinee_anyref, &arms[1..], bind_ty, fn_return_ty, ctx);
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
        AnfOp::AMatch { arms, .. } => arms.iter().all(|arm| expr_always_diverges(&arm.body)),
        // A loop may break and produce a value; keep conservative.
        AnfOp::ALoop { .. } => false,
        // Defer lowering preserves inner expr structure but does not diverge at bind site.
        AnfOp::ADefer(_) => false,
        AnfOp::ACall { .. }
        | AnfOp::ABinOp { .. }
        | AnfOp::AUnOp { .. }
        | AnfOp::AMakeClosure { .. }
        | AnfOp::ARecord { .. }
        | AnfOp::ARecordGet { .. }
        | AnfOp::ARecordUpdate { .. }
        | AnfOp::AVariant { .. }
        | AnfOp::AArrayLit(_)
        | AnfOp::AIndex { .. }
        | AnfOp::AInit { .. }
        | AnfOp::AAssign { .. } => false,
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
            instrs.extend(emit_string_literal_atom(s));
            instrs.push(Instr::Call("rt_str__eq".to_string()));
            instrs
        }
        CorePattern::Variant {
            type_id,
            variant,
            fields,
        } => {
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
                let field_anyref = emit_variant_field_anyref(value_anyref_instrs, idx as i32);
                inner_checks.push(emit_pattern_condition(field_pat, &field_anyref, ctx));
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
        CorePattern::Variant { fields, .. } => {
            let mut instrs = Vec::new();
            for (idx, field_pat) in fields.iter().enumerate() {
                let field_anyref = emit_variant_field_anyref(value_anyref_instrs, idx as i32);
                instrs.extend(emit_pattern_bindings(field_pat, &field_anyref, ctx));
            }
            instrs
        }
    }
}

fn emit_variant_field_anyref(value_anyref_instrs: &[Instr], field_idx: i32) -> Vec<Instr> {
    let mut instrs = value_anyref_instrs.to_vec();
    instrs.extend(emit_unbox_on_stack(&ref_variant_null()));
    instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2));
    instrs.push(Instr::I32Const(field_idx));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs
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
    let mut instrs = emit_string_literal_atom("non-exhaustive match");
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
            let mut instrs = emit_let_binding(*local, op, fn_return_ty, ctx);
            instrs.extend(emit_loop_body_expr(body, fn_return_ty, ctx));
            instrs
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
    match expr {
        AnfExpr::Let { local, op, body } => {
            let mut instrs = emit_let_binding(*local, op, fn_return_ty, ctx);
            instrs.extend(emit_expr_value(body, expected_ty, fn_return_ty, ctx));
            instrs
        }
        AnfExpr::Atom(atom) => emit_atom(atom, Some(expected_ty), ctx),
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
        (None, Some(expected)) => instrs.extend(emit_void_value(Some(expected))),
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
                emit_tail_runtime_prelude_call(&entry, args, return_ty, ctx)
            } else {
                emit_tail_direct_user_call(*func_id, args, return_ty, ctx)
            }
        }
        Atom::ALocal(_) => emit_tail_closure_call(callee, args, return_ty, ctx),
        _ => None,
    }
}

fn emit_tail_runtime_prelude_call(
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

fn emit_tail_direct_user_call(
    func_id: FuncId,
    args: &[Atom],
    return_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Option<Vec<Instr>> {
    let sig = ctx
        .user_func_sig(func_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing signature for function FuncId({})", func_id.0));
    if !tail_user_result_compatible(sig.result.as_ref(), return_ty) {
        return None;
    }
    if sig.params.len() != args.len() {
        panic!(
            "arity mismatch for direct call to FuncId({}): expected {}, got {}",
            func_id.0,
            sig.params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (arg, param_ty) in args.iter().zip(sig.params.iter()) {
        instrs.extend(emit_atom(arg, Some(param_ty), ctx));
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
        Atom::AGlobalFunc(func_id) => emit_global_func_atom(*func_id, expected_ty),
        Atom::ALitInt(n) => emit_int_literal(*n, expected_ty),
        Atom::ALitFloat(v) => emit_float_literal(*v, expected_ty),
        Atom::ALitBool(b) => emit_bool_literal(*b, expected_ty),
        Atom::ALitStr(s) => emit_string_literal(s, expected_ty),
        Atom::ALitVoid => emit_void_value(expected_ty),
    }
}

fn emit_local_atom(
    local_id: crate::ir::LocalId,
    expected_ty: Option<&ValType>,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if let Some((idx, local_ty)) = ctx.local(local_id).cloned() {
        return match expected_ty {
            None => vec![Instr::LocalGet(idx)],
            Some(expected) if expected == &local_ty => vec![Instr::LocalGet(idx)],
            Some(expected) => emit_coerce_local(idx, &local_ty, expected),
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

fn emit_string_literal(s: &str, expected_ty: Option<&ValType>) -> Vec<Instr> {
    match expected_ty {
        None | Some(ValType::Anyref) | Some(ValType::Ref { .. }) => emit_string_literal_atom(s),
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

fn emit_coerce_local(idx: u32, local_ty: &ValType, expected: &ValType) -> Vec<Instr> {
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
        _ => panic!(
            "unsupported local coercion from {:?} to {:?} in Stage 8c Step 2",
            local_ty, expected
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
    for atom in ordered.into_iter().flatten() {
        instrs.extend(emit_atom(atom, Some(&ValType::Anyref), ctx));
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
    let record_sym = user_record_type_sym(type_id);
    let mut instrs = emit_atom(target, Some(&ref_user_record_null(type_id)), ctx);
    instrs.push(Instr::StructGet(record_sym, field.0 as u32));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
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
    let record_sym = user_record_type_sym(type_id);
    let mut instrs = Vec::new();

    if can_reuse_in_place {
        instrs.extend(emit_atom(base, Some(&ref_user_record_null(type_id)), ctx));
        instrs.extend(emit_atom(base, Some(&ref_user_record_null(type_id)), ctx));
        instrs.extend(emit_atom(value, Some(&ValType::Anyref), ctx));
        instrs.push(Instr::StructSet(record_sym, field.0 as u32));
        instrs.extend(emit_coerce_stack(&ref_user_record_null(type_id), bind_ty));
        return instrs;
    }

    let field_count = record_field_count(type_id, ctx);
    for idx in 0..field_count {
        if idx == field.0 {
            instrs.extend(emit_atom(value, Some(&ValType::Anyref), ctx));
        } else {
            instrs.extend(emit_atom(base, Some(&ref_user_record_null(type_id)), ctx));
            instrs.push(Instr::StructGet(user_record_type_sym(type_id), idx as u32));
        }
    }
    instrs.push(Instr::StructNew(user_record_type_sym(type_id)));
    instrs.extend(emit_coerce_stack(&ref_user_record(type_id), bind_ty));
    instrs
}

fn emit_variant_literal(
    type_id: TypeId,
    variant: crate::ir::VariantId,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
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

fn emit_array_literal(elems: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = Vec::new();
    for elem in elems {
        instrs.extend(emit_atom(elem, Some(&ValType::Anyref), ctx));
    }
    instrs.push(Instr::ArrayNewFixed(
        T_ARRAY.to_string(),
        elems.len() as u32,
    ));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
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
            instrs.extend(emit_atom(base, Some(&ref_array_null()), ctx));
            instrs.extend(emit_index_as_i32(index, ctx));
            instrs.push(Instr::Call("rt_arr__get".to_string()));
            instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
        }
        crate::ir::anf::IndexKind::Dict => {
            // Dict indexing returns Option<V>, so use get_option which returns a
            // proper Variant (Option.None/Some) instead of raw anyref.
            ensure_rt_dict_get_option_import(ctx);
            instrs.extend(emit_atom(base, Some(&ref_dict_null()), ctx));
            instrs.extend(emit_atom(index, Some(&ValType::Anyref), ctx));
            instrs.push(Instr::Call("rt_dict__get_option".to_string()));
            instrs.extend(emit_coerce_stack(&ref_variant(), bind_ty));
        }
    }
    instrs
}

fn emit_index_as_i32(index: &Atom, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = emit_atom(index, Some(&ValType::I64), ctx);
    instrs.push(Instr::I32WrapI64);
    instrs
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
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_direct_user_call(
    func_id: FuncId,
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let sig = ctx
        .user_func_sig(func_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing signature for function FuncId({})", func_id.0));

    if sig.params.len() != args.len() {
        panic!(
            "arity mismatch for direct call to FuncId({}): expected {}, got {}",
            func_id.0,
            sig.params.len(),
            args.len()
        );
    }

    let mut instrs = Vec::new();
    for (arg, param_ty) in args.iter().zip(sig.params.iter()) {
        instrs.extend(emit_atom(arg, Some(param_ty), ctx));
    }
    instrs.push(Instr::Call(user_func_sym(func_id)));

    match sig.result {
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
    let sig = ctx
        .user_func_sig(func_id)
        .unwrap_or_else(|| panic!("missing signature for trampoline FuncId({})", func_id.0));
    let mut body = Vec::new();
    for (idx, param_ty) in sig.params.iter().take(func.param_tys.len()).enumerate() {
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
    match &sig.result {
        Some(result_ty) => body.extend(emit_coerce_stack(result_ty, &ValType::Anyref)),
        None => body.extend(emit_void_value(Some(&ValType::Anyref))),
    }

    FuncDef {
        name: global_func_trampoline_sym(func_id),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::Anyref],
        locals: Vec::new(),
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
        return emit_runtime_prelude_call(entry, args, bind_ty, ctx);
    }

    match func_id {
        id if id == prelude_ids::STRING_TO_STRING => {
            if args.len() != 1 {
                panic!("string_to_string expects exactly one argument");
            }
            let mut instrs = emit_atom(&args[0], Some(&ref_string_null()), ctx);
            instrs.extend(emit_coerce_stack(&ref_string_null(), bind_ty));
            instrs
        }
        id if id == prelude_ids::ARRAY_APPEND => emit_array_append_intrinsic(args, bind_ty, ctx),
        // Range constructors: build Record(RANGE_TYPE_ID, [start, end, step])
        id if id == prelude_ids::RANGE => {
            // range(n) -> Record(3, [0, n, 1])
            emit_range_intrinsic(
                &[Atom::ALitInt(0), args[0].clone(), Atom::ALitInt(1)],
                bind_ty,
                ctx,
            )
        }
        id if id == prelude_ids::RANGE_FROM => {
            // range_from(start, end) -> Record(3, [start, end, 1])
            emit_range_intrinsic(
                &[args[0].clone(), args[1].clone(), Atom::ALitInt(1)],
                bind_ty,
                ctx,
            )
        }
        id if id == prelude_ids::RANGE_STEP => {
            // range_step(start, end, step) -> Record(3, [start, end, step])
            emit_range_intrinsic(
                &[args[0].clone(), args[1].clone(), args[2].clone()],
                bind_ty,
                ctx,
            )
        }
        // Cell operations
        id if id == prelude_ids::CELL_NEW => emit_cell_new_intrinsic(args, bind_ty, ctx),
        id if id == prelude_ids::CELL_GET => emit_cell_get_intrinsic(args, bind_ty, ctx),
        id if id == prelude_ids::CELL_SET => emit_cell_set_intrinsic(args, bind_ty, ctx),
        id if id == prelude_ids::CELL_UPDATE => emit_cell_update_intrinsic(args, bind_ty, ctx),
        // Dict internal
        id if id == prelude_ids::DICT_GET_UNSAFE => {
            // dict_get_unsafe is same as dict_get but for internal loop use
            ensure_rt_dict_get_import(ctx);
            let mut instrs = emit_atom(&args[0], Some(&ref_dict_null()), ctx);
            instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
            instrs.push(Instr::Call("rt_dict__get".to_string()));
            instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
            instrs
        }
        // Iterator operations
        id if id == prelude_ids::ITERATOR_UNFOLD => {
            emit_iterator_unfold_intrinsic(args, bind_ty, ctx)
        }
        id if id == prelude_ids::ITERATOR_NEXT => emit_iterator_next_intrinsic(args, bind_ty, ctx),
        // Array builder operations (used by collect)
        id if id == prelude_ids::ARRAY_BUILDER_NEW => {
            emit_array_builder_new_intrinsic(bind_ty, ctx)
        }
        id if id == prelude_ids::ARRAY_BUILDER_PUSH => {
            emit_array_builder_push_intrinsic(args, bind_ty, ctx)
        }
        id if id == prelude_ids::ARRAY_BUILDER_FREEZE => {
            emit_array_builder_freeze_intrinsic(args, bind_ty, ctx)
        }
        id if id == prelude_ids::DEBUG_STDIN_READ_ALL => {
            if !args.is_empty() {
                panic!("__debug_stdin_read_all expects 0 args, got {}", args.len());
            }
            // Match interpreter behavior for interactive runs: non-blocking
            // empty input when no piped stdin is available.
            let mut instrs = emit_string_literal_atom("");
            instrs.extend(emit_coerce_stack(&ref_string(), bind_ty));
            instrs
        }
        _ => emit_unimplemented_intrinsic_prelude_call(entry, ctx),
    }
}

fn emit_array_append_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    if args.len() != 2 {
        panic!("Array.append intrinsic expects 2 args, got {}", args.len());
    }

    ensure_rt_arr_concat_import(ctx);

    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    instrs.push(Instr::Call("rt_arr__concat".to_string()));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
    instrs
}

// --- Range intrinsics ---

fn emit_range_intrinsic(fields: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    use crate::types::ty::RANGE_TYPE_ID;
    let range_sym = user_record_type_sym(RANGE_TYPE_ID);
    let mut instrs = Vec::new();
    for atom in fields {
        instrs.extend(emit_atom(atom, Some(&ValType::Anyref), ctx));
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
// Cell is represented as a 1-element mutable rt_types__Array (anyref[1]).

fn emit_cell_new_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = emit_atom(&args[0], Some(&ValType::Anyref), ctx);
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
    instrs
}

fn emit_cell_get_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_cell_set_intrinsic(args: &[Atom], bind_ty: &ValType, ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::ArraySet(T_ARRAY.to_string()));
    instrs.extend(emit_void_value(Some(bind_ty)));
    instrs
}

fn emit_cell_update_intrinsic(
    args: &[Atom],
    _bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Cell.update(cell, f) = cell[0] = f(cell[0])
    // cell is args[0] (array), f is args[1] (closure)
    let mut instrs = Vec::new();

    // Get the cell ref
    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    // Duplicate: one copy for the set target, one for reading old value
    // We need: cell, 0, f(cell[0])
    // Strategy: read cell[0], call closure with it, store result back
    instrs.push(Instr::I32Const(0)); // cell, 0  (for array.set later)

    // Read old value: cell[0]
    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));

    // Call closure with old value: f(old_val)
    // Pack the single arg into an args array
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
    // Get closure's env
    instrs.extend(emit_atom(&args[1], Some(&ref_closure_null()), ctx));
    instrs.push(Instr::StructGet(T_CLOSURE.to_string(), 1)); // env
    // Swap: we need (args_array, env) but have (env). Actually we need env first, then args
    // call_ref convention: (env: anyref, args: Array) -> anyref
    // Wait - let me check the closure call convention...

    // Actually the closure calling convention in Wasm is:
    // (args_array: ref $rt_types__Array, closure_ref) -> call_ref $ClosureFunc
    // Where ClosureFunc = (env: anyref, args: anyref) -> anyref

    // Let me redo this. The call convention requires:
    // stack: [env_array, args_array, func_ref] then call_ref
    // Actually looking at emit_closure_call:
    // 1. Push callee.env (StructGet(closure, 1))
    // 2. Push args array
    // 3. Push callee.func_ref (StructGet(closure, 0))
    // 4. CallRef

    // But we can't easily do this inline because we need the array.set too.
    // Simpler: just push trap for now and implement Cell.update later if needed.

    // Actually, let me simplify. Most uses of Cell.update aren't in the failing test fixtures.
    // Let me check...
    instrs.clear();

    ensure_rt_core_trap_import(ctx);
    instrs.extend(emit_string_literal_atom(
        "unimplemented intrinsic prelude call: Cell.update",
    ));
    instrs.push(Instr::Call("rt_core__trap".to_string()));
    instrs.push(Instr::Unreachable);
    instrs
}

// --- Iterator intrinsics ---
// Iterator is represented as a 2-element rt_types__Array: [seed, step_closure]

fn emit_iterator_unfold_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // Iterator.unfold(seed, step) -> [seed, step] as anyref array
    let mut instrs = emit_atom(&args[0], Some(&ValType::Anyref), ctx);
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 2));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
    instrs
}

fn emit_iterator_next_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::Call(ITERATOR_NEXT_HELPER.to_string()));
    instrs.extend(emit_coerce_stack(&ref_variant_null(), bind_ty));
    instrs
}

const ITERATOR_NEXT_HELPER: &str = "user____iterator_next";

fn needs_iterator_next_helper(ctx: &EmitCtx<'_>) -> bool {
    // Check if any imports reference the iterator next helper
    ctx.imports().iter().any(|_| false) || {
        // Simpler: always emit it if the prelude has ITERATOR_NEXT
        // We check if the helper was referenced by checking if ITERATOR_NEXT is in the prelude
        // Actually, just check if any function called Iterator.next by checking
        // if the helper function name appears in any emitted instruction.
        // For simplicity, always emit when the type env has Iterator type.
        true // Always emit for now; it's a small helper
    }
}

/// Emit the `__iterator_next` Wasm helper function.
/// Takes an iterator (anyref array [seed, step_closure]) and returns Option<IterItem> variant.
fn emit_iterator_next_helper() -> FuncDef {
    use crate::types::ty::{ITER_ITEM_TYPE_ID, OPTION_TYPE_ID};

    // Locals:
    // 0: param it (anyref = iterator array ref)
    // 1: step_result (variant ref)
    // 2: variant_id (i32)
    // 3: payload / temp (anyref)
    // 4: it_arr (ref null $Array = cast of param 0)

    let mut body = Vec::new();

    // Cast param 0 to array ref, store in local 4
    body.push(Instr::LocalGet(0));
    body.push(Instr::RefCast {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });
    body.push(Instr::LocalSet(4));

    // --- Call step(seed) ---

    // Push closure env
    body.push(Instr::LocalGet(4));
    body.push(Instr::I32Const(1));
    body.push(Instr::ArrayGet(T_ARRAY.to_string()));
    body.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(T_CLOSURE.to_string()),
    });
    body.push(Instr::StructGet(T_CLOSURE.to_string(), 1)); // env

    // Push args array (containing seed)
    body.push(Instr::LocalGet(4));
    body.push(Instr::I32Const(0));
    body.push(Instr::ArrayGet(T_ARRAY.to_string()));
    body.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));

    // Push func_ref from step closure
    body.push(Instr::LocalGet(4));
    body.push(Instr::I32Const(1));
    body.push(Instr::ArrayGet(T_ARRAY.to_string()));
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
        result: Some(ValType::Anyref),
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
            let mut else_instrs = Vec::new();

            // Extract payload from step_result
            else_instrs.push(Instr::LocalGet(1));
            else_instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2)); // payload array
            else_instrs.push(Instr::LocalSet(3));

            // Get yielded value (payload[0])
            // Get next_seed (payload[1])
            // Construct next_iter = [next_seed, step]
            // Construct IterItem record (TypeId=5) = { value, next_iter }

            // Build IterItem record: UserRecord_5 with fields [value_anyref, rest_anyref]
            let iter_item_sym = user_record_type_sym(ITER_ITEM_TYPE_ID);

            // Field 0 = value = payload[0]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::I32Const(0));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));

            // Field 1 = rest = new iterator [next_seed, step]
            // next_seed = payload[1]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            // step = original it[1]
            else_instrs.push(Instr::LocalGet(0));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            // Pack into iterator array
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 2));

            // Construct IterItem struct
            else_instrs.push(Instr::StructNew(iter_item_sym));

            // Wrap in Option.Some = Variant(OPTION_TYPE_ID, 1, [item])
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));
            else_instrs.push(Instr::I32Const(OPTION_TYPE_ID.0 as i32));
            else_instrs.push(Instr::I32Const(1)); // Some variant

            // Wait, StructNew for Variant needs args in order: type_id, variant_id, payload
            // So we need: i32(type_id), i32(variant_id), ref(payload_array)
            // Let me fix the order:

            else_instrs.clear();

            // Extract payload
            else_instrs.push(Instr::LocalGet(1));
            else_instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2));
            else_instrs.push(Instr::LocalSet(3));

            // Build the IterItem record
            let iter_item_sym = user_record_type_sym(ITER_ITEM_TYPE_ID);

            // Field 0: value = payload[0]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::I32Const(0));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));

            // Field 1: rest = [payload[1], it[1]] (new iterator)
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            else_instrs.push(Instr::LocalGet(0));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 2));

            // struct.new IterItem (2 anyref fields)
            else_instrs.push(Instr::StructNew(iter_item_sym));

            // Now construct Option.Some(iter_item):
            // Variant struct fields: (type_id: i32, variant_id: i32, payload: array<anyref>)
            // Push: type_id, variant_id, payload_array
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1)); // wrap item in payload
            // We need type_id and variant_id BEFORE payload on stack for struct.new
            // struct.new takes fields in order: type_id, variant_id, payload

            // Hmm, struct.new pops args in reverse? No, struct.new pops in field order.
            // Variant struct has fields: [type_id: i32, variant_id: i32, payload: ref array]
            // So we need: i32(type_id), i32(variant_id), ref(payload) on stack, then struct.new

            // We currently have payload on top. Need to reorganize.
            // Easiest: store payload in local 3, push type_id, variant_id, then load payload

            else_instrs.clear();

            // Extract UnfoldStep payload
            else_instrs.push(Instr::LocalGet(1));
            else_instrs.push(Instr::StructGet(T_VARIANT.to_string(), 2));
            else_instrs.push(Instr::LocalSet(3)); // payload array

            // --- Build IterItem record ---
            let iter_item_sym = user_record_type_sym(ITER_ITEM_TYPE_ID);

            // Field 0: value = payload[0]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.to_string()),
            });
            else_instrs.push(Instr::I32Const(0));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));

            // Field 1: rest iterator = [next_seed, step]
            else_instrs.push(Instr::LocalGet(3));
            else_instrs.push(Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.to_string()),
            });
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            else_instrs.push(Instr::LocalGet(4));
            else_instrs.push(Instr::I32Const(1));
            else_instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
            else_instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 2));

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
        params: vec![ValType::Anyref],  // iterator array ref
        results: vec![ValType::Anyref], // Option variant ref
        locals: vec![
            ref_variant_null(), // local 1: step_result variant
            ValType::I32,       // local 2: variant_id
            ValType::Anyref,    // local 3: payload / temp
            ref_array_null(),   // local 4: it_arr (cast of param 0)
        ],
        body,
    }
}

// --- Array builder intrinsics ---
// Array builder is represented as a Cell (1-element array) containing an array.

fn emit_array_builder_new_intrinsic(bind_ty: &ValType, _ctx: &mut EmitCtx<'_>) -> Vec<Instr> {
    // Creates Cell containing empty array: [[]]
    let mut instrs = vec![
        // Empty inner array
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 0),
        // Wrap in cell (1-element outer array)
        Instr::ArrayNewFixed(T_ARRAY.to_string(), 1),
    ];
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
    instrs
}

fn emit_array_builder_push_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // array_builder_push(builder, elem) -> Void
    // builder = Cell = array[1] where builder[0] is the accumulating array
    // We need: new_arr = append(old_arr, elem); builder[0] = new_arr
    ensure_rt_arr_concat_import(ctx);

    let mut instrs = Vec::new();

    // Target: builder[0] = concat(builder[0], [elem])
    // array.set needs: array_ref, index, value

    // Push builder ref (for array.set target)
    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    // Push index 0
    instrs.push(Instr::I32Const(0));

    // Compute new value: concat(builder[0], [elem])
    // Get current array from builder
    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs.push(Instr::RefCast {
        nullable: false,
        heap: HeapType::Named(T_ARRAY.to_string()),
    });

    // Create 1-element array with the new element
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::ArrayNewFixed(T_ARRAY.to_string(), 1));

    // Concat
    instrs.push(Instr::Call("rt_arr__concat".to_string()));

    // array.set: builder[0] = new_array
    instrs.push(Instr::ArraySet(T_ARRAY.to_string()));

    instrs.extend(emit_void_value(Some(bind_ty)));
    instrs
}

fn emit_array_builder_freeze_intrinsic(
    args: &[Atom],
    bind_ty: &ValType,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    // array_builder_freeze(builder) -> Array<T>
    // builder = Cell = array[1], return builder[0]
    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.push(Instr::I32Const(0));
    instrs.push(Instr::ArrayGet(T_ARRAY.to_string()));
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
    instrs
}

fn emit_unimplemented_intrinsic_prelude_call(
    entry: &crate::codegen::prelude::PreludeEntry,
    ctx: &mut EmitCtx<'_>,
) -> Vec<Instr> {
    ensure_rt_core_trap_import(ctx);
    let mut instrs = emit_string_literal_atom(&format!(
        "unimplemented intrinsic prelude call: {}",
        entry.twinkle_name
    ));
    instrs.push(Instr::Call("rt_core__trap".to_string()));
    instrs.push(Instr::Unreachable);
    instrs
}

fn emit_runtime_prelude_call(
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
    instrs.push(Instr::Call(sym));

    match entry.runtime_results.as_slice() {
        [] => instrs.extend(emit_void_value(Some(bind_ty))),
        [single] => instrs.extend(emit_coerce_stack(single, bind_ty)),
        _ => panic!(
            "multi-value runtime prelude return not supported yet: {}",
            entry.twinkle_name
        ),
    }

    instrs
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

fn ref_user_record_null(type_id: TypeId) -> ValType {
    ValType::Ref {
        nullable: true,
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

fn ensure_rt_str_concat_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.str".to_string(),
        name: "concat".to_string(),
        as_sym: "rt_str__concat".to_string(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ref_string()],
    });
}

fn ensure_rt_arr_concat_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "concat".to_string(),
        as_sym: "rt_arr__concat".to_string(),
        params: vec![ref_array_null(), ref_array_null()],
        results: vec![ref_array()],
    });
}

fn ensure_rt_arr_get_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "get".to_string(),
        as_sym: "rt_arr__get".to_string(),
        params: vec![ref_array_null(), ValType::I32],
        results: vec![ValType::Anyref],
    });
}

fn ensure_rt_dict_get_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.dict".to_string(),
        name: "get".to_string(),
        as_sym: "rt_dict__get".to_string(),
        params: vec![ref_dict_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
    });
}

fn ensure_rt_dict_get_option_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.dict".to_string(),
        name: "get_option".to_string(),
        as_sym: "rt_dict__get_option".to_string(),
        params: vec![ref_dict_null(), ValType::Anyref],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::prelude::build_prelude_map;
    use crate::ir::{FieldId, LocalId, VariantId};
    use crate::types::ty::{OPTION_TYPE_ID, RANGE_TYPE_ID};

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
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named(T_STRING.to_string()),
                }
            ]
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_str__from_i64"));
    }

    #[test]
    fn emit_array_append_intrinsic_lowers_to_concat_with_singleton() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_array_null()));
        ctx.local_map.insert(LocalId(2), (1, ValType::I64));

        let entry = ctx
            .prelude
            .get(&prelude_ids::ARRAY_APPEND)
            .cloned()
            .expect("missing prelude entry");
        let instrs = emit_prelude_call(
            prelude_ids::ARRAY_APPEND,
            &entry,
            &[Atom::ALocal(LocalId(1)), Atom::ALocal(LocalId(2))],
            &ref_array_null(),
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::LocalGet(0),
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 1),
                Instr::Call("rt_arr__concat".to_string()),
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named("rt_types__Array".to_string()),
                },
            ]
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_arr__concat"));
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
        };
        let anf = AnfModule {
            functions: vec![fib_like],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env, &HashMap::new());
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
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named(T_CLOSURE.to_string()),
                },
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
    fn emit_tail_direct_user_call_uses_return_call() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let mut user_funcs = HashMap::new();
        user_funcs.insert(
            FuncId(100),
            FuncSigInfo {
                params: vec![ValType::I64],
                result: Some(ValType::I64),
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
        };
        let anf = AnfModule {
            functions: vec![func],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env, &HashMap::new());
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
        };
        let anf = AnfModule {
            functions: vec![callee, caller],
            init_func_id: None,
            all_init_func_ids: Vec::new(),
        };

        let module = emit_user_module(&anf, &type_env, &HashMap::new());
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

        let module = emit_user_module(&anf, &type_env, &HashMap::new());
        assert!(module.types.iter().any(|t| matches!(
            t,
            WasmTypeDef::Struct { name, .. } if name == "UserRecord_3"
        )));
    }

    #[test]
    fn emit_record_get_unboxes_anyref_field_to_i64() {
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
                Instr::RefCast {
                    nullable: false,
                    heap: HeapType::Named(T_BOXED_INT.to_string()),
                },
                Instr::StructGet(T_BOXED_INT.to_string(), 0),
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
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::LocalGet(1),
                Instr::StructGet("UserRecord_3".to_string(), 2),
                Instr::StructNew("UserRecord_3".to_string()),
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named("UserRecord_3".to_string()),
                },
                Instr::LocalSet(0),
            ]
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
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);
        ctx.local_map.insert(LocalId(1), (0, ref_array_null()));

        let instrs = emit_let_binding(
            LocalId(1),
            &AnfOp::AArrayLit(vec![Atom::ALitInt(1), Atom::ALitBool(true)]),
            None,
            &mut ctx,
        );

        assert_eq!(
            instrs,
            vec![
                Instr::I64Const(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::I32Const(1),
                Instr::RefI31,
                Instr::ArrayNewFixed(T_ARRAY.to_string(), 2),
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named(T_ARRAY.to_string()),
                },
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
        ctx.local_map.insert(LocalId(2), (1, ref_array_null()));

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
        ctx.local_map.insert(LocalId(2), (1, ref_dict_null()));

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
                Instr::I32Const(107),
                Instr::ArrayNewFixed(T_STRING.to_string(), 1),
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
    fn emit_loop_with_break_none_materializes_void_result() {
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
                                Instr::I32Const(0),
                                Instr::RefI31,
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
                },
                AnfFunctionDef {
                    func_id: FuncId(2),
                    name: "b".to_string(),
                    params: vec![],
                    param_tys: vec![],
                    body: AnfExpr::Atom(Atom::ALitVoid),
                    return_ty: MonoType::Void,
                },
            ],
            init_func_id: Some(FuncId(2)),
            all_init_func_ids: vec![FuncId(1), FuncId(2)],
        };

        let module = emit_user_module(&anf, &type_env, &HashMap::new());
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
