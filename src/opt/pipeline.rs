use std::collections::HashSet;

use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom};
use crate::ir::core::{CorePattern, LocalId};
use crate::opt::defer_elim::eliminate_defers;
use crate::opt::liveness::annotate_in_place;
use crate::opt::passes::{
    branch_simplify, constant_fold, copy_propagate_with_pinned, dead_let_elim,
};
use crate::opt::uniqueness::uniqueness_rewrite;
use crate::opt::use_count::{collect_assigned_locals, count_uses};

const MAX_ROUNDS: usize = 10;

/// Run all peephole optimization passes to a fixed point on a single function,
/// then annotate record updates with in-place reuse eligibility.
pub fn optimize_func(mut func: AnfFunctionDef, pinned: &HashSet<LocalId>) -> AnfFunctionDef {
    for _ in 0..MAX_ROUNDS {
        let uses = count_uses(&func.body);
        let mut assigned = collect_assigned_locals(&func.body);
        assigned.extend(pinned.iter().copied());
        let mut changed = false;

        let (body, c) = dead_let_elim(func.body, &uses, &assigned);
        func.body = body;
        changed |= c;

        let (body, c) = copy_propagate_with_pinned(func.body, pinned);
        func.body = body;
        changed |= c;

        let (body, c) = constant_fold(func.body);
        func.body = body;
        changed |= c;

        let (body, c) = branch_simplify(func.body);
        func.body = body;
        changed |= c;

        if !changed {
            break;
        }
    }

    annotate_in_place(&mut func);
    uniqueness_rewrite(&mut func);
    // Eliminate all ADefer nodes — must run after peephole passes since it
    // restructures terminal nodes (Return/Break/Continue/Atom) irreversibly.
    func = eliminate_defers(func);
    func
}

/// Optimize every function in an ANF module.
pub fn optimize_module(module: AnfModule) -> AnfModule {
    let module_globals = collect_module_globals(&module);
    let functions = module
        .functions
        .into_iter()
        .map(|func| {
            if func.name == "__init__" {
                optimize_func(func, &module_globals)
            } else {
                optimize_func(func, &HashSet::new())
            }
        })
        .collect();
    AnfModule {
        functions,
        ..module
    }
}

fn collect_module_globals(module: &AnfModule) -> HashSet<LocalId> {
    let init_funcs = module
        .functions
        .iter()
        .filter(|f| f.name == "__init__")
        .map(|f| f.func_id)
        .collect::<HashSet<_>>();

    let mut referenced_outside_init = HashSet::new();
    for func in &module.functions {
        let mut declared = func.params.iter().copied().collect::<HashSet<_>>();
        let mut free = HashSet::new();
        collect_free_locals_expr(&func.body, &mut declared, &mut free);
        referenced_outside_init.extend(free);
    }

    let mut bound_in_init = HashSet::new();
    for func in &module.functions {
        if init_funcs.contains(&func.func_id) {
            collect_bound_locals_expr(&func.body, &mut bound_in_init);
        }
    }

    referenced_outside_init
        .into_iter()
        .filter(|id| bound_in_init.contains(id))
        .collect()
}

fn collect_bound_locals_expr(expr: &AnfExpr, out: &mut HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            out.insert(*local);
            collect_bound_locals_op(op, out);
            collect_bound_locals_expr(body, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_bound_locals_op(op: &AnfOp, out: &mut HashSet<LocalId>) {
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

fn collect_free_locals_expr(
    expr: &AnfExpr,
    declared: &mut HashSet<LocalId>,
    free: &mut HashSet<LocalId>,
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
    declared: &mut HashSet<LocalId>,
    free: &mut HashSet<LocalId>,
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
            for AnfMatchArm { pattern, body } in arms {
                let mut arm_declared = declared.clone();
                collect_pattern_bindings(pattern, &mut arm_declared);
                collect_free_locals_expr(body, &mut arm_declared, free);
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

fn collect_pattern_bindings(pattern: &CorePattern, declared: &mut HashSet<LocalId>) {
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

fn collect_free_locals_atom(atom: &Atom, declared: &HashSet<LocalId>, free: &mut HashSet<LocalId>) {
    if let Atom::ALocal(local_id) = atom {
        if !declared.contains(local_id) {
            free.insert(*local_id);
        }
    }
}
