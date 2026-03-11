use std::collections::HashSet;

use crate::ir::anf::analysis::{collect_bound_locals, collect_free_locals};
use crate::ir::anf::{AnfFunctionDef, AnfModule};
use crate::ir::core::LocalId;
use crate::opt::defer_elim::eliminate_defers;
use crate::opt::passes::{
    branch_simplify, constant_fold, copy_propagate_with_pinned, dead_let_elim,
};
use crate::opt::uniqueness::uniqueness_rewrite;
use crate::opt::use_count::{collect_assigned_locals, count_uses};

const MAX_ROUNDS: usize = 10;

/// Run all peephole optimization passes to a fixed point on a single function,
/// then run uniqueness-based rewrites/annotations.
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
