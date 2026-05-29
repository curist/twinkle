use std::collections::HashSet;

use super::{AnfExpr, AnfOp, Atom};
use crate::ir::core::{CorePattern, LocalId};

#[derive(Debug, Clone, Copy)]
pub struct DivergenceOptions {
    pub empty_match_diverges: bool,
}

impl Default for DivergenceOptions {
    fn default() -> Self {
        Self {
            empty_match_diverges: true,
        }
    }
}

pub fn collect_pattern_bindings(pattern: &CorePattern, out: &mut HashSet<LocalId>) {
    match pattern {
        CorePattern::Var(id) => {
            out.insert(*id);
        }
        CorePattern::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(field, out);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

pub fn collect_free_locals(
    expr: &AnfExpr,
    mut declared_seed: HashSet<LocalId>,
) -> HashSet<LocalId> {
    let mut free = HashSet::new();
    collect_free_locals_expr(expr, &mut declared_seed, &mut free);
    free
}

pub fn collect_bound_locals(expr: &AnfExpr) -> HashSet<LocalId> {
    let mut out = HashSet::new();
    collect_bound_locals_expr(expr, &mut out);
    out
}

/// Collect locals introduced by `AInit` bindings.
///
/// In `__init__`, source-level module bindings are lowered as `AInit`, while
/// compiler temporaries use their producing op directly. This distinction is
/// important for whole-module analyses because ordinary `LocalId`s are scoped to
/// a function and can collide numerically across functions.
pub fn collect_init_binding_locals(expr: &AnfExpr) -> HashSet<LocalId> {
    let mut out = HashSet::new();
    collect_init_binding_locals_expr(expr, &mut out);
    out
}

pub fn collect_assigned_locals(expr: &AnfExpr) -> HashSet<LocalId> {
    let mut out = HashSet::new();
    collect_assigned_locals_expr(expr, &mut out);
    out
}

pub fn expr_always_diverges(expr: &AnfExpr) -> bool {
    expr_always_diverges_with(expr, DivergenceOptions::default())
}

pub fn expr_always_diverges_with(expr: &AnfExpr, opts: DivergenceOptions) -> bool {
    match expr {
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => true,
        AnfExpr::Atom(_) => false,
        AnfExpr::Let { op, body, .. } => {
            op_always_diverges_with(op, opts) || expr_always_diverges_with(body, opts)
        }
    }
}

pub fn op_always_diverges(op: &AnfOp) -> bool {
    op_always_diverges_with(op, DivergenceOptions::default())
}

pub fn op_always_diverges_with(op: &AnfOp, opts: DivergenceOptions) -> bool {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            expr_always_diverges_with(then_branch, opts)
                && expr_always_diverges_with(else_branch, opts)
        }
        AnfOp::AMatch { arms, .. } => {
            (opts.empty_match_diverges || !arms.is_empty())
                && arms
                    .iter()
                    .all(|arm| expr_always_diverges_with(&arm.body, opts))
        }
        _ => false,
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

fn collect_free_locals_atom(atom: &Atom, declared: &HashSet<LocalId>, free: &mut HashSet<LocalId>) {
    if let Atom::ALocal(local_id) = atom
        && !declared.contains(local_id)
    {
        free.insert(*local_id);
    }
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

fn collect_init_binding_locals_expr(expr: &AnfExpr, out: &mut HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if matches!(op.as_ref(), AnfOp::AInit { .. }) {
                out.insert(*local);
            }
            collect_init_binding_locals_op(op, out);
            collect_init_binding_locals_expr(body, out);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn collect_init_binding_locals_op(op: &AnfOp, out: &mut HashSet<LocalId>) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_init_binding_locals_expr(then_branch, out);
            collect_init_binding_locals_expr(else_branch, out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_init_binding_locals_expr(&arm.body, out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_init_binding_locals_expr(body, out);
        }
        _ => {}
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        DivergenceOptions, collect_assigned_locals, collect_bound_locals, collect_free_locals,
        collect_init_binding_locals, collect_pattern_bindings, expr_always_diverges,
        expr_always_diverges_with, op_always_diverges,
    };
    use crate::ir::anf::{AnfExpr, AnfMatchArm, AnfOp, Atom, OpKind};
    use crate::ir::core::{CorePattern, LocalId, VariantId};
    use crate::syntax::ast::BinOp;
    use crate::types::ty::{MonoType, TypeId};

    fn lid(id: u32) -> LocalId {
        LocalId(id)
    }

    #[test]
    fn collect_pattern_bindings_handles_nested_variants() {
        let pattern = CorePattern::Variant {
            type_id: TypeId(7),
            variant: VariantId(2),
            fields: vec![
                CorePattern::Var(lid(1)),
                CorePattern::Variant {
                    type_id: TypeId(8),
                    variant: VariantId(1),
                    fields: vec![CorePattern::Var(lid(3)), CorePattern::Wildcard],
                },
            ],
        };
        let mut out = HashSet::new();
        collect_pattern_bindings(&pattern, &mut out);
        assert_eq!(out, HashSet::from([lid(1), lid(3)]));
    }

    #[test]
    fn collect_free_locals_respects_let_and_match_pattern_bindings() {
        let expr = AnfExpr::Let {
            local: lid(1),
            op: Box::new(AnfOp::AMatch {
                scrutinee: Atom::ALocal(lid(0)),
                arms: vec![AnfMatchArm {
                    pattern: CorePattern::Variant {
                        type_id: TypeId(5),
                        variant: VariantId(0),
                        fields: vec![CorePattern::Var(lid(2))],
                    },
                    body: AnfExpr::Let {
                        local: lid(3),
                        op: Box::new(AnfOp::ABinOp {
                            op: BinOp::Add,
                            left: Atom::ALocal(lid(2)),
                            right: Atom::ALitInt(5),
                            operand_ty: OpKind::Int,
                        }),
                        body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(3)))),
                    },
                }],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(1)))),
        };
        let free = collect_free_locals(&expr, HashSet::from([lid(0)]));
        assert!(free.is_empty(), "unexpected free locals: {free:?}");
    }

    #[test]
    fn collect_free_locals_includes_undeclared_assign_target() {
        let expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(9),
                value: Atom::ALitInt(42),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let free = collect_free_locals(&expr, HashSet::new());
        assert_eq!(free, HashSet::from([lid(9)]));
    }

    #[test]
    fn collect_free_locals_accumulates_branch_locals() {
        let expr = AnfExpr::Let {
            local: lid(4),
            op: Box::new(AnfOp::AIf {
                cond: Atom::ALocal(lid(0)),
                then_branch: Box::new(AnfExpr::Let {
                    local: lid(5),
                    op: Box::new(AnfOp::AInit {
                        value: Atom::ALocal(lid(1)),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(5)))),
                }),
                else_branch: Box::new(AnfExpr::Let {
                    local: lid(6),
                    op: Box::new(AnfOp::AInit {
                        value: Atom::ALocal(lid(2)),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(6)))),
                }),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(4)))),
        };
        let free = collect_free_locals(&expr, HashSet::from([lid(0)]));
        assert_eq!(free, HashSet::from([lid(1), lid(2)]));
    }

    #[test]
    fn collect_free_locals_handles_defer_and_match_scrutinee() {
        let expr = AnfExpr::Let {
            local: lid(10),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Let {
                local: lid(11),
                op: Box::new(AnfOp::AMatch {
                    scrutinee: Atom::ALocal(lid(20)),
                    arms: vec![AnfMatchArm {
                        pattern: CorePattern::LitInt(1),
                        body: AnfExpr::Atom(Atom::ALitInt(0)),
                    }],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(11)))),
            }))),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(10)))),
        };
        let free = collect_free_locals(&expr, HashSet::new());
        assert_eq!(free, HashSet::from([lid(20)]));
    }

    #[test]
    fn collect_free_locals_ignores_globals_and_literals() {
        let expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ARecord {
                type_id: TypeId(1),
                fields: vec![(crate::ir::core::FieldId(0), Atom::ALitStr("x".to_string()))],
            }),
            body: Box::new(AnfExpr::Let {
                local: lid(1),
                op: Box::new(AnfOp::ABinOp {
                    op: BinOp::Eq,
                    left: Atom::ALocal(lid(0)),
                    right: Atom::ALitInt(1),
                    operand_ty: OpKind::Int,
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(1)))),
            }),
        };
        let free = collect_free_locals(&expr, HashSet::new());
        assert_eq!(free, HashSet::new());
    }

    #[test]
    fn collect_free_locals_tracks_make_closure_free_vars() {
        let expr = AnfExpr::Let {
            local: lid(8),
            op: Box::new(AnfOp::AMakeClosure {
                func_id: crate::ir::core::FuncId(99),
                free_vars: vec![lid(1), lid(2)],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(8)))),
        };
        let free = collect_free_locals(&expr, HashSet::from([lid(1)]));
        assert_eq!(free, HashSet::from([lid(2)]));
    }

    #[test]
    fn collect_free_locals_handles_return_and_break_atoms() {
        let return_expr = AnfExpr::Return(Some(Atom::ALocal(lid(7))));
        let break_expr = AnfExpr::Break(Some(Atom::ALocal(lid(8))));
        let free_return = collect_free_locals(&return_expr, HashSet::new());
        let free_break = collect_free_locals(&break_expr, HashSet::new());
        assert_eq!(free_return, HashSet::from([lid(7)]));
        assert_eq!(free_break, HashSet::from([lid(8)]));
    }

    #[test]
    fn collect_free_locals_handles_variant_and_index_operands() {
        let expr = AnfExpr::Let {
            local: lid(3),
            op: Box::new(AnfOp::AVariant {
                type_id: TypeId(2),
                variant: VariantId(0),
                args: vec![Atom::ALocal(lid(1)), Atom::ALocal(lid(2))],
            }),
            body: Box::new(AnfExpr::Let {
                local: lid(4),
                op: Box::new(AnfOp::AIndex {
                    base: Atom::ALocal(lid(3)),
                    index: Atom::ALocal(lid(9)),
                    base_ty: crate::ir::anf::IndexKind::Array,
                    result_ty: MonoType::Int,
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(4)))),
            }),
        };
        let free = collect_free_locals(&expr, HashSet::new());
        assert_eq!(free, HashSet::from([lid(1), lid(2), lid(9)]));
    }

    #[test]
    fn collect_bound_locals_collects_nested_let_bindings() {
        let expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AIf {
                cond: Atom::ALitBool(true),
                then_branch: Box::new(AnfExpr::Let {
                    local: lid(1),
                    op: Box::new(AnfOp::AInit {
                        value: Atom::ALitInt(1),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(1)))),
                }),
                else_branch: Box::new(AnfExpr::Let {
                    local: lid(2),
                    op: Box::new(AnfOp::ALoop {
                        body: Box::new(AnfExpr::Let {
                            local: lid(3),
                            op: Box::new(AnfOp::AInit {
                                value: Atom::ALitInt(2),
                            }),
                            body: Box::new(AnfExpr::Break(None)),
                        }),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(2)))),
                }),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(0)))),
        };
        let bound = collect_bound_locals(&expr);
        assert_eq!(bound, HashSet::from([lid(0), lid(1), lid(2), lid(3)]));
    }

    #[test]
    fn collect_init_binding_locals_excludes_non_init_temporaries() {
        let expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AArrayLit(vec![])),
            body: Box::new(AnfExpr::Let {
                local: lid(1),
                op: Box::new(AnfOp::AInit {
                    value: Atom::ALocal(lid(0)),
                }),
                body: Box::new(AnfExpr::Let {
                    local: lid(2),
                    op: Box::new(AnfOp::AIf {
                        cond: Atom::ALitBool(true),
                        then_branch: Box::new(AnfExpr::Let {
                            local: lid(3),
                            op: Box::new(AnfOp::AInit {
                                value: Atom::ALitInt(1),
                            }),
                            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(3)))),
                        }),
                        else_branch: Box::new(AnfExpr::Let {
                            local: lid(4),
                            op: Box::new(AnfOp::ARecord {
                                type_id: TypeId(0),
                                fields: vec![],
                            }),
                            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(4)))),
                        }),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(2)))),
                }),
            }),
        };
        assert_eq!(
            collect_bound_locals(&expr),
            HashSet::from([lid(0), lid(1), lid(2), lid(3), lid(4)])
        );
        assert_eq!(
            collect_init_binding_locals(&expr),
            HashSet::from([lid(1), lid(3)])
        );
    }

    #[test]
    fn collect_assigned_locals_tracks_nested_assign_targets() {
        let expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AIf {
                cond: Atom::ALitBool(true),
                then_branch: Box::new(AnfExpr::Let {
                    local: lid(1),
                    op: Box::new(AnfOp::AAssign {
                        local: lid(8),
                        value: Atom::ALitInt(10),
                    }),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(1)))),
                }),
                else_branch: Box::new(AnfExpr::Let {
                    local: lid(2),
                    op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Let {
                        local: lid(3),
                        op: Box::new(AnfOp::AAssign {
                            local: lid(9),
                            value: Atom::ALitInt(20),
                        }),
                        body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(3)))),
                    }))),
                    body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(2)))),
                }),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(0)))),
        };
        let assigned = collect_assigned_locals(&expr);
        assert_eq!(assigned, HashSet::from([lid(8), lid(9)]));
    }

    #[test]
    fn divergence_if_requires_both_branches_to_diverge() {
        let diverging_if = AnfOp::AIf {
            cond: Atom::ALitBool(true),
            then_branch: Box::new(AnfExpr::Return(None)),
            else_branch: Box::new(AnfExpr::Break(None)),
        };
        let non_diverging_if = AnfOp::AIf {
            cond: Atom::ALitBool(true),
            then_branch: Box::new(AnfExpr::Return(None)),
            else_branch: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        assert!(op_always_diverges(&diverging_if));
        assert!(!op_always_diverges(&non_diverging_if));
    }

    #[test]
    fn divergence_match_empty_policy_is_configurable() {
        let empty_match = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AMatch {
                scrutinee: Atom::ALitInt(1),
                arms: vec![],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        assert!(expr_always_diverges(&empty_match));
        assert!(!expr_always_diverges_with(
            &empty_match,
            DivergenceOptions {
                empty_match_diverges: false,
            }
        ));
    }

    #[test]
    fn divergence_treats_assign_and_defer_as_non_diverging_ops() {
        let assign = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(1),
                value: Atom::ALitInt(3),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(0)))),
        };
        let defer = AnfExpr::Let {
            local: lid(2),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Atom(Atom::ALitVoid)))),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(2)))),
        };
        assert!(!expr_always_diverges(&assign));
        assert!(!expr_always_diverges(&defer));
    }

    #[test]
    fn divergence_keeps_loop_ops_conservative_even_with_diverging_body() {
        let loop_expr = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ALoop {
                body: Box::new(AnfExpr::Return(None)),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        assert!(!expr_always_diverges(&loop_expr));
    }

    #[test]
    fn guardrail_no_local_analysis_reimplementations_in_consumers() {
        let files = [
            ("codegen/ctx.rs", include_str!("../../codegen/ctx.rs")),
            ("codegen/emit.rs", include_str!("../../codegen/emit.rs")),
            ("opt/pipeline.rs", include_str!("../../opt/pipeline.rs")),
            ("opt/defer_elim.rs", include_str!("../../opt/defer_elim.rs")),
            ("opt/use_count.rs", include_str!("../../opt/use_count.rs")),
        ];
        let forbidden = [
            "fn collect_free_locals_expr(",
            "fn collect_free_locals_op(",
            "fn collect_free_locals_atom(",
            "fn collect_pattern_bindings(",
            "fn collect_bound_locals_expr(",
            "fn collect_bound_locals_op(",
            "fn collect_assigned_locals_expr(",
            "fn collect_assigned_locals_op(",
            "fn expr_always_diverges(",
            "fn op_always_diverges(",
        ];
        for (path, body) in files {
            for pattern in forbidden {
                assert!(
                    !body.contains(pattern),
                    "{path} reintroduces local analysis helper `{pattern}`"
                );
            }
        }
    }
}
