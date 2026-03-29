//! Dead-code elimination for linked CoreModule.
//!
//! After `link()` assembles all functions from all modules, this pass removes
//! unreachable functions and renumbers FuncIds to be compact and sequential.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::intrinsics::registry;
use crate::ir::core::{CoreExpr, CoreExprKind, CoreModule, FuncId, MatchArm};
use crate::ir::lower::prelude;

/// Remove unreachable functions from a linked CoreModule and renumber FuncIds.
///
/// Roots: all `__init__` functions in `all_init_func_ids`.
/// Reachable set is computed by BFS over GlobalFunc and MakeClosure references.
/// Unreachable functions are dropped; remaining functions get compact FuncIds.
pub fn eliminate_dead_code(module: CoreModule) -> CoreModule {
    eliminate_dead_code_with_roots(module, &[])
}

/// Remove unreachable functions from a linked `CoreModule`, preserving any
/// additional explicit roots alongside the standard `__init__` roots.
pub fn eliminate_dead_code_with_roots(
    mut module: CoreModule,
    extra_roots: &[FuncId],
) -> CoreModule {
    // 1. Build adjacency list
    let mut refs: HashMap<FuncId, HashSet<FuncId>> = HashMap::new();
    for func in &module.functions {
        let mut callees = HashSet::new();
        collect_func_refs(&func.body, &mut callees);
        refs.insert(func.func_id, callees);
    }

    // 2. BFS from roots
    let mut reachable = HashSet::new();
    let mut queue: VecDeque<FuncId> = VecDeque::new();
    for &init_id in &module.all_init_func_ids {
        if reachable.insert(init_id) {
            queue.push_back(init_id);
        }
    }
    for &root_id in extra_roots {
        if reachable.insert(root_id) {
            queue.push_back(root_id);
        }
    }
    while let Some(id) = queue.pop_front() {
        if let Some(callees) = refs.get(&id) {
            for &callee in callees {
                // Only track user functions; prelude intrinsics are always available
                if callee.0 >= prelude::USER_FUNC_START && reachable.insert(callee) {
                    queue.push_back(callee);
                }
            }
        }
    }

    // 3. Filter to reachable functions (preserve original order)
    module.functions.retain(|f| reachable.contains(&f.func_id));

    // 4. Build old→new FuncId mapping (compact sequential IDs)
    //    Sort by original FuncId to preserve the linker's ID assignment order,
    //    which may differ from the vec position order.
    //    Skip prelude FuncIds to avoid collisions with sparse intrinsic IDs (1001+).
    let prelude_ids: HashSet<u32> = registry::all_specs()
        .iter()
        .map(|spec| spec.func_id.0)
        .collect();
    let mut old_to_new: HashMap<FuncId, FuncId> = HashMap::new();
    let mut sorted_ids: Vec<FuncId> = module
        .functions
        .iter()
        .filter(|f| f.func_id.0 >= prelude::USER_FUNC_START)
        .map(|f| f.func_id)
        .collect();
    sorted_ids.sort_by_key(|id| id.0);
    let mut next_id = prelude::USER_FUNC_START;
    for old_id in sorted_ids {
        while prelude_ids.contains(&next_id) {
            next_id += 1;
        }
        old_to_new.insert(old_id, FuncId(next_id));
        next_id += 1;
    }

    // 5. Remap all FuncIds
    for func in &mut module.functions {
        if let Some(&new_id) = old_to_new.get(&func.func_id) {
            func.func_id = new_id;
        }
        remap_expr(&mut func.body, &old_to_new);
    }

    // Remap init FuncIds
    if let Some(ref mut init_id) = module.init_func_id {
        if let Some(&new_id) = old_to_new.get(init_id) {
            *init_id = new_id;
        }
    }
    module.all_init_func_ids = module
        .all_init_func_ids
        .iter()
        .filter_map(|id| {
            if reachable.contains(id) {
                Some(old_to_new.get(id).copied().unwrap_or(*id))
            } else {
                None
            }
        })
        .collect();

    module
}

/// Collect all FuncId references (GlobalFunc and MakeClosure) from an expression.
fn collect_func_refs(expr: &CoreExpr, out: &mut HashSet<FuncId>) {
    match &expr.kind {
        CoreExprKind::GlobalFunc(id) => {
            out.insert(*id);
        }
        CoreExprKind::MakeClosure { func_id, .. } => {
            out.insert(*func_id);
        }
        CoreExprKind::Let { value, body, .. } => {
            collect_func_refs(value, out);
            collect_func_refs(body, out);
        }
        CoreExprKind::Assign { value, .. } => {
            collect_func_refs(value, out);
        }
        CoreExprKind::BinOp { left, right, .. } => {
            collect_func_refs(left, out);
            collect_func_refs(right, out);
        }
        CoreExprKind::UnOp { expr, .. } => {
            collect_func_refs(expr, out);
        }
        CoreExprKind::Call { callee, args } => {
            collect_func_refs(callee, out);
            for arg in args {
                collect_func_refs(arg, out);
            }
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_func_refs(cond, out);
            collect_func_refs(then_branch, out);
            collect_func_refs(else_branch, out);
        }
        CoreExprKind::Match { scrutinee, arms } => {
            collect_func_refs(scrutinee, out);
            for MatchArm { body, .. } in arms {
                collect_func_refs(body, out);
            }
        }
        CoreExprKind::Loop { body } => {
            collect_func_refs(body, out);
        }
        CoreExprKind::Break { value } | CoreExprKind::Return { value } => {
            if let Some(v) = value {
                collect_func_refs(v, out);
            }
        }
        CoreExprKind::Record { fields, .. } => {
            for (_, v) in fields {
                collect_func_refs(v, out);
            }
        }
        CoreExprKind::RecordGet { target, .. } => {
            collect_func_refs(target, out);
        }
        CoreExprKind::Variant { args, .. } => {
            for arg in args {
                collect_func_refs(arg, out);
            }
        }
        CoreExprKind::ArrayLit { elements } => {
            for e in elements {
                collect_func_refs(e, out);
            }
        }
        CoreExprKind::Index { base, index } => {
            collect_func_refs(base, out);
            collect_func_refs(index, out);
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            collect_func_refs(base, out);
            collect_func_refs(value, out);
        }
        CoreExprKind::Defer(inner) => {
            collect_func_refs(inner, out);
        }
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => {}
    }
}

/// Remap FuncIds in an expression tree using old→new mapping.
fn remap_expr(expr: &mut CoreExpr, map: &HashMap<FuncId, FuncId>) {
    match &mut expr.kind {
        CoreExprKind::GlobalFunc(id) => {
            if let Some(&new_id) = map.get(id) {
                *id = new_id;
            }
        }
        CoreExprKind::MakeClosure { func_id, .. } => {
            if let Some(&new_id) = map.get(func_id) {
                *func_id = new_id;
            }
        }
        CoreExprKind::Let { value, body, .. } => {
            remap_expr(value, map);
            remap_expr(body, map);
        }
        CoreExprKind::Assign { value, .. } => {
            remap_expr(value, map);
        }
        CoreExprKind::BinOp { left, right, .. } => {
            remap_expr(left, map);
            remap_expr(right, map);
        }
        CoreExprKind::UnOp { expr, .. } => {
            remap_expr(expr, map);
        }
        CoreExprKind::Call { callee, args } => {
            remap_expr(callee, map);
            for arg in args {
                remap_expr(arg, map);
            }
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            remap_expr(cond, map);
            remap_expr(then_branch, map);
            remap_expr(else_branch, map);
        }
        CoreExprKind::Match { scrutinee, arms } => {
            remap_expr(scrutinee, map);
            for MatchArm { body, .. } in arms {
                remap_expr(body, map);
            }
        }
        CoreExprKind::Loop { body } => {
            remap_expr(body, map);
        }
        CoreExprKind::Break { value } | CoreExprKind::Return { value } => {
            if let Some(v) = value {
                remap_expr(v, map);
            }
        }
        CoreExprKind::Record { fields, .. } => {
            for (_, v) in fields {
                remap_expr(v, map);
            }
        }
        CoreExprKind::RecordGet { target, .. } => {
            remap_expr(target, map);
        }
        CoreExprKind::Variant { args, .. } => {
            for arg in args {
                remap_expr(arg, map);
            }
        }
        CoreExprKind::ArrayLit { elements } => {
            for e in elements {
                remap_expr(e, map);
            }
        }
        CoreExprKind::Index { base, index } => {
            remap_expr(base, map);
            remap_expr(index, map);
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            remap_expr(base, map);
            remap_expr(value, map);
        }
        CoreExprKind::Defer(inner) => {
            remap_expr(inner, map);
        }
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::FunctionDef;
    use crate::syntax::span::{FileId, Span};
    use crate::types::env::TypeEnv;
    use crate::types::ty::MonoType;

    fn dummy_span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    fn lit_void() -> CoreExpr {
        CoreExpr {
            kind: CoreExprKind::LitVoid,
            ty: MonoType::Void,
            span: dummy_span(),
        }
    }

    fn global_func_expr(id: u32) -> CoreExpr {
        CoreExpr {
            kind: CoreExprKind::GlobalFunc(FuncId(id)),
            ty: MonoType::Void,
            span: dummy_span(),
        }
    }

    fn call_expr(callee: CoreExpr, args: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            ty: MonoType::Void,
            span: dummy_span(),
        }
    }

    fn make_func(id: u32, name: &str, body: CoreExpr) -> FunctionDef {
        FunctionDef {
            func_id: FuncId(id),
            name: name.to_string(),
            params: vec![],
            param_tys: vec![],
            body,
            return_ty: MonoType::Void,
        }
    }

    fn make_module(
        functions: Vec<FunctionDef>,
        init_func_id: Option<FuncId>,
        all_init_func_ids: Vec<FuncId>,
    ) -> CoreModule {
        CoreModule {
            functions,
            type_env: TypeEnv::new(),
            init_func_id,
            all_init_func_ids,
        }
    }

    #[test]
    fn test_unreachable_function_is_removed() {
        // __init__ (41) calls foo (42), but bar (43) is unreachable
        let init = make_func(41, "__init__", call_expr(global_func_expr(42), vec![]));
        let foo = make_func(42, "foo", lit_void());
        let bar = make_func(43, "bar", lit_void()); // unreachable

        let module = make_module(vec![init, foo, bar], Some(FuncId(41)), vec![FuncId(41)]);

        let result = eliminate_dead_code(module);

        let names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["__init__", "foo"]);
        assert!(!names.contains(&"bar"));
    }

    #[test]
    fn test_funcids_are_renumbered_compactly() {
        // __init__ (41) calls baz (43), skipping 42; after DCE, baz should be 42
        let init = make_func(41, "__init__", call_expr(global_func_expr(43), vec![]));
        let unused = make_func(42, "unused", lit_void());
        let baz = make_func(43, "baz", lit_void());

        let module = make_module(vec![init, unused, baz], Some(FuncId(41)), vec![FuncId(41)]);

        let result = eliminate_dead_code(module);

        let ids: Vec<u32> = result.functions.iter().map(|f| f.func_id.0).collect();
        assert_eq!(ids, vec![41, 42]); // compact: 41, 42

        // The call in __init__ should now point to 42 (was 43)
        if let CoreExprKind::Call { callee, .. } = &result.functions[0].body.kind {
            if let CoreExprKind::GlobalFunc(id) = &callee.kind {
                assert_eq!(id.0, 42);
            } else {
                panic!("expected GlobalFunc callee");
            }
        } else {
            panic!("expected Call body");
        }
    }

    #[test]
    fn test_closure_reference_keeps_function_alive() {
        // __init__ (41) creates a closure over func 42
        let init = make_func(
            41,
            "__init__",
            CoreExpr {
                kind: CoreExprKind::MakeClosure {
                    func_id: FuncId(42),
                    free_vars: vec![],
                },
                ty: MonoType::Void,
                span: dummy_span(),
            },
        );
        let closure_fn = make_func(42, "closure_fn", lit_void());
        let dead = make_func(43, "dead", lit_void());

        let module = make_module(
            vec![init, closure_fn, dead],
            Some(FuncId(41)),
            vec![FuncId(41)],
        );

        let result = eliminate_dead_code(module);
        let names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["__init__", "closure_fn"]);
    }

    #[test]
    fn test_transitive_reachability() {
        // __init__ → a → b → c; d is unreachable
        let init = make_func(41, "__init__", call_expr(global_func_expr(42), vec![]));
        let a = make_func(42, "a", call_expr(global_func_expr(43), vec![]));
        let b = make_func(43, "b", call_expr(global_func_expr(44), vec![]));
        let c = make_func(44, "c", lit_void());
        let d = make_func(45, "d", lit_void()); // unreachable

        let module = make_module(vec![init, a, b, c, d], Some(FuncId(41)), vec![FuncId(41)]);

        let result = eliminate_dead_code(module);
        let names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["__init__", "a", "b", "c"]);
    }

    #[test]
    fn test_init_func_id_is_remapped() {
        let init = make_func(41, "__init__", lit_void());
        let module = make_module(vec![init], Some(FuncId(41)), vec![FuncId(41)]);

        let result = eliminate_dead_code(module);
        assert_eq!(result.init_func_id, Some(FuncId(41)));
        assert_eq!(result.all_init_func_ids, vec![FuncId(41)]);
    }

    #[test]
    fn test_prelude_refs_not_tracked_as_user_functions() {
        // __init__ calls a prelude function (id=2, println) — should not affect reachability
        let init = make_func(41, "__init__", call_expr(global_func_expr(2), vec![]));
        let dead = make_func(42, "dead", lit_void());

        let module = make_module(vec![init, dead], Some(FuncId(41)), vec![FuncId(41)]);

        let result = eliminate_dead_code(module);
        let names: Vec<&str> = result.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["__init__"]);
    }
}
