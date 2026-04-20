use std::collections::{HashMap, HashSet};

use crate::ir::anf::analysis::{collect_bound_locals, collect_free_locals};
use crate::ir::anf::{AnfFunctionDef, AnfModule};
use crate::ir::core::{FuncId, LocalId};
use crate::opt::defer_elim::eliminate_defers;
use crate::opt::passes::{
    branch_simplify, constant_fold, copy_propagate_with_pinned, dead_let_elim,
};
use crate::opt::uniqueness::{
    TinyWrapperSummary, collect_tiny_wrapper_summaries, uniqueness_rewrite,
};
use crate::opt::use_count::{collect_assigned_locals, count_uses};

#[cfg(debug_assertions)]
use crate::ir::anf::verify::verify_function_after_pass;

const MAX_ROUNDS: usize = 10;

fn run_peephole_passes(mut func: AnfFunctionDef, pinned: &HashSet<LocalId>) -> AnfFunctionDef {
    for _ in 0..MAX_ROUNDS {
        let uses = count_uses(&func.body);
        let mut assigned = collect_assigned_locals(&func.body);
        assigned.extend(pinned.iter().copied());
        let mut changed = false;

        let (body, c) = dead_let_elim(func.body, &uses, &assigned);
        func.body = body;
        changed |= c;
        #[cfg(debug_assertions)]
        verify_function_after_pass(&func, "dead_let_elim");

        let (body, c) = copy_propagate_with_pinned(func.body, pinned);
        func.body = body;
        changed |= c;
        #[cfg(debug_assertions)]
        verify_function_after_pass(&func, "copy_propagate");

        let (body, c) = constant_fold(func.body);
        func.body = body;
        changed |= c;
        #[cfg(debug_assertions)]
        verify_function_after_pass(&func, "constant_fold");

        let (body, c) = branch_simplify(func.body);
        func.body = body;
        changed |= c;
        #[cfg(debug_assertions)]
        verify_function_after_pass(&func, "branch_simplify");

        if !changed {
            break;
        }
    }

    func
}

/// Run all peephole optimization passes to a fixed point on a single function,
/// then run uniqueness-based rewrites/annotations.
pub fn optimize_func(
    func: AnfFunctionDef,
    pinned: &HashSet<LocalId>,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> AnfFunctionDef {
    // eliminate_defers must run before peephole passes: branch_simplify would
    // otherwise hoist ADefer nodes out of constant-condition branches into the
    // enclosing function scope, breaking block-scoped defer semantics.
    let mut func = eliminate_defers(func);
    #[cfg(debug_assertions)]
    verify_function_after_pass(&func, "eliminate_defers");
    func = run_peephole_passes(func, pinned);
    uniqueness_rewrite(&mut func, wrappers);
    func
}

/// Optimize every function in an ANF module.
pub fn optimize_module(module: AnfModule) -> AnfModule {
    let module_globals = collect_module_globals(&module);

    // eliminate_defers first: branch_simplify (in peephole) would otherwise
    // hoist ADefer nodes out of constant-condition branches, breaking block-scoped
    // defer semantics.
    let defer_eliminated = module
        .functions
        .into_iter()
        .map(|func| {
            let func = eliminate_defers(func);
            #[cfg(debug_assertions)]
            verify_function_after_pass(&func, "eliminate_defers");
            func
        })
        .collect::<Vec<_>>();

    let peepholed = defer_eliminated
        .into_iter()
        .map(|func| {
            if func.name == "__init__" {
                run_peephole_passes(func, &module_globals)
            } else {
                run_peephole_passes(func, &HashSet::new())
            }
        })
        .collect::<Vec<_>>();

    let summaries = collect_tiny_wrapper_summaries(&AnfModule {
        functions: peepholed.clone(),
        init_func_id: module.init_func_id,
        all_init_func_ids: module.all_init_func_ids.clone(),
    });

    let functions = peepholed
        .into_iter()
        .map(|mut func| {
            let pinned = if func.name == "__init__" {
                &module_globals
            } else {
                &HashSet::new()
            };
            uniqueness_rewrite(&mut func, &summaries);
            #[cfg(debug_assertions)]
            verify_function_after_pass(&func, "uniqueness_rewrite");
            let _ = pinned;
            func
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
        let declared = func.params.iter().copied().collect::<HashSet<_>>();
        let free = collect_free_locals(&func.body, declared);
        referenced_outside_init.extend(free);
    }

    let mut bound_in_init = HashSet::new();
    for func in &module.functions {
        if init_funcs.contains(&func.func_id) {
            bound_in_init.extend(collect_bound_locals(&func.body));
        }
    }

    referenced_outside_init
        .into_iter()
        .filter(|id| bound_in_init.contains(id))
        .collect()
}
