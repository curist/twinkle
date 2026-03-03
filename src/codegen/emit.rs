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
    FieldDef as WasmFieldDef, FuncDef, HeapType, ImportDef, Instr, ModuleIR,
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
    let user_sigs = build_user_sig_map(anf, type_env);
    let mut ctx = EmitCtx::new(type_env, &prelude, &user_sigs);
    let mut module = ModuleIR::new("user");
    module.types.extend(emit_user_record_type_defs(type_env));

    for func in &anf.functions {
        module.funcs.push(emit_func_stub(func, &mut ctx));
    }
    for func in &anf.functions {
        let capture_locals = infer_capture_locals(func);
        module.funcs.push(emit_user_closure_trampoline(
            func,
            capture_locals.len(),
            &ctx,
        ));
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

fn build_user_sig_map(anf: &AnfModule, type_env: &TypeEnv) -> HashMap<FuncId, FuncSigInfo> {
    anf.functions
        .iter()
        .map(|func| {
            let capture_locals = infer_capture_locals(func);
            let mut params = func
                .param_tys
                .iter()
                .map(|ty| mono_to_valtype(ty, type_env))
                .collect::<Vec<_>>();
            params.extend(vec![ValType::Anyref; capture_locals.len()]);
            let result = match &func.return_ty {
                MonoType::Void => None,
                other => Some(mono_to_valtype(other, type_env)),
            };
            (func.func_id, FuncSigInfo { params, result })
        })
        .collect()
}

fn emit_func_stub(func: &AnfFunctionDef, ctx: &mut EmitCtx<'_>) -> FuncDef {
    let capture_locals = infer_capture_locals(func);
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

fn infer_capture_locals(func: &AnfFunctionDef) -> Vec<crate::ir::LocalId> {
    let mut declared: HashSet<crate::ir::LocalId> = func.params.iter().copied().collect();
    let mut free: HashSet<crate::ir::LocalId> = HashSet::new();
    collect_free_locals_expr(&func.body, &mut declared, &mut free);
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
        MonoType::Void => Vec::new(),
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
            let mut instrs = emit_atom(value, Some(&bind_ty), ctx);
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AAssign {
            local: target,
            value,
        } => {
            let (target_idx, target_ty) = ctx
                .local(*target)
                .cloned()
                .unwrap_or_else(|| panic!("missing assignment target mapping for L{}", target.0));
            let mut instrs = emit_atom(value, Some(&target_ty), ctx);
            instrs.push(Instr::LocalSet(target_idx));
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
            instrs.push(Instr::If {
                result: Some(bind_ty.clone()),
                then_body,
                else_body,
            });
            instrs.push(Instr::LocalSet(bind_idx));
            instrs
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let mut instrs = emit_match_op(scrutinee, arms, &bind_ty, fn_return_ty, ctx);
            instrs.push(Instr::LocalSet(bind_idx));
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
    let else_body = emit_match_arm_chain(scrutinee_anyref, &arms[1..], bind_ty, fn_return_ty, ctx);
    instrs.push(Instr::If {
        result: Some(bind_ty.clone()),
        then_body,
        else_body,
    });
    instrs
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
            let mut checks = Vec::new();

            let mut type_check = value_anyref_instrs.to_vec();
            type_check.extend(emit_unbox_on_stack(&ref_variant_null()));
            type_check.push(Instr::StructGet(T_VARIANT.to_string(), 0));
            type_check.push(Instr::I32Const(type_id.0 as i32));
            type_check.push(Instr::I32Eq);
            checks.push(type_check);

            let mut variant_check = value_anyref_instrs.to_vec();
            variant_check.extend(emit_unbox_on_stack(&ref_variant_null()));
            variant_check.push(Instr::StructGet(T_VARIANT.to_string(), 1));
            variant_check.push(Instr::I32Const(variant.0 as i32));
            variant_check.push(Instr::I32Eq);
            checks.push(variant_check);

            for (idx, field_pat) in fields.iter().enumerate() {
                if pattern_is_trivially_true(field_pat) {
                    continue;
                }
                let field_anyref = emit_variant_field_anyref(value_anyref_instrs, idx as i32);
                checks.push(emit_pattern_condition(field_pat, &field_anyref, ctx));
            }

            combine_i32_ands(checks)
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
        body: vec![Instr::Loop {
            label: cont_label,
            result: None,
            body: loop_body,
        }],
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
    let (idx, local_ty) = ctx
        .local(local_id)
        .cloned()
        .unwrap_or_else(|| panic!("missing local mapping for L{}", local_id.0));

    match expected_ty {
        None => vec![Instr::LocalGet(idx)],
        Some(expected) if expected == &local_ty => vec![Instr::LocalGet(idx)],
        Some(expected) => emit_coerce_local(idx, &local_ty, expected),
    }
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
        }
        crate::ir::anf::IndexKind::Dict => {
            ensure_rt_dict_get_import(ctx);
            instrs.extend(emit_atom(base, Some(&ref_dict_null()), ctx));
            instrs.extend(emit_atom(index, Some(&ValType::Anyref), ctx));
            instrs.push(Instr::Call("rt_dict__get".to_string()));
        }
    }
    instrs.extend(emit_coerce_stack(&ValType::Anyref, bind_ty));
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
    // Sort free_vars by LocalId to match `infer_capture_locals` ordering.
    // The trampoline reads env slots in sorted order, so the env array must
    // be built in the same order.
    let mut sorted_vars = free_vars.to_vec();
    sorted_vars.sort_by_key(|id| id.0);
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

    ensure_rt_arr_len_import(ctx);
    ensure_rt_arr_set_import(ctx);

    let mut instrs = emit_atom(&args[0], Some(&ref_array_null()), ctx);
    instrs.extend(emit_atom(&args[0], Some(&ref_array_null()), ctx));
    instrs.push(Instr::Call("rt_arr__len".to_string()));
    instrs.extend(emit_atom(&args[1], Some(&ValType::Anyref), ctx));
    instrs.push(Instr::Call("rt_arr__set".to_string()));
    instrs.extend(emit_coerce_stack(&ref_array(), bind_ty));
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

fn ensure_rt_arr_len_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "len".to_string(),
        as_sym: "rt_arr__len".to_string(),
        params: vec![ref_array_null()],
        results: vec![ValType::I32],
    });
}

fn ensure_rt_arr_set_import(ctx: &mut EmitCtx<'_>) {
    ctx.add_import(ImportDef {
        module: "rt.arr".to_string(),
        name: "set".to_string(),
        as_sym: "rt_arr__set".to_string(),
        params: vec![ref_array_null(), ValType::I32, ValType::Anyref],
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
    fn emit_array_append_intrinsic_lowers_to_len_plus_set() {
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
                Instr::LocalGet(0),
                Instr::Call("rt_arr__len".to_string()),
                Instr::LocalGet(1),
                Instr::StructNew(T_BOXED_INT.to_string()),
                Instr::Call("rt_arr__set".to_string()),
                Instr::RefCast {
                    nullable: true,
                    heap: HeapType::Named("rt_types__Array".to_string()),
                },
            ]
        );

        let imports = ctx.imports();
        assert!(imports.iter().any(|i| i.as_sym == "rt_arr__len"));
        assert!(imports.iter().any(|i| i.as_sym == "rt_arr__set"));
    }

    #[test]
    fn emit_unimplemented_intrinsic_uses_runtime_trap_not_compiler_panic() {
        let type_env = TypeEnv::new();
        let prelude = build_prelude_map();
        let user_funcs = HashMap::new();
        let mut ctx = EmitCtx::new(&type_env, &prelude, &user_funcs);

        let entry = ctx
            .prelude
            .get(&prelude_ids::RANGE)
            .cloned()
            .expect("missing prelude entry");
        let instrs = emit_prelude_call(
            prelude_ids::RANGE,
            &entry,
            &[Atom::ALitInt(5)],
            &ValType::Anyref,
            &mut ctx,
        );

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
        let func = AnfFunctionDef {
            func_id: FuncId(2),
            name: "capturing".to_string(),
            params: vec![LocalId(1)],
            param_tys: vec![MonoType::Int],
            body: AnfExpr::Atom(Atom::ALocal(LocalId(42))),
            return_ty: MonoType::Int,
        };
        let anf = AnfModule {
            functions: vec![func],
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
                Instr::Call("rt_dict__get".to_string()),
                Instr::LocalSet(0),
            ]
        );
        assert!(ctx.imports().iter().any(|i| i.as_sym == "rt_dict__get"));
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
                    body: vec![Instr::Loop {
                        label: "cont_0".to_string(),
                        result: None,
                        body: vec![
                            Instr::I64Const(5),
                            Instr::Br("break_0".to_string()),
                            Instr::Br("cont_0".to_string()),
                        ],
                    }],
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
                    body: vec![Instr::Loop {
                        label: "cont_0".to_string(),
                        result: None,
                        body: vec![
                            Instr::I32Const(0),
                            Instr::RefI31,
                            Instr::Br("break_0".to_string()),
                            Instr::Br("cont_0".to_string()),
                        ],
                    }],
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
        assert_eq!(instrs.iter().rev().nth(1), Some(&Instr::Unreachable));
        assert!(ctx.imports().iter().any(|i| i.as_sym == "rt_core__trap"));
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
