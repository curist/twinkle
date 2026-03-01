use crate::ir::anf::{AnfFunctionDef, AnfModule};
use crate::opt::liveness::annotate_in_place;
use crate::opt::passes::{branch_simplify, constant_fold, copy_propagate, dead_let_elim};
use crate::opt::use_count::count_uses;

const MAX_ROUNDS: usize = 10;

/// Run all peephole optimization passes to a fixed point on a single function,
/// then annotate record updates with in-place reuse eligibility.
pub fn optimize_func(mut func: AnfFunctionDef) -> AnfFunctionDef {
    for _ in 0..MAX_ROUNDS {
        let uses = count_uses(&func.body);
        let mut changed = false;

        let (body, c) = dead_let_elim(func.body, &uses);
        func.body = body;
        changed |= c;

        let (body, c) = copy_propagate(func.body);
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
    func
}

/// Optimize every function in an ANF module.
pub fn optimize_module(module: AnfModule) -> AnfModule {
    let functions = module.functions.into_iter().map(optimize_func).collect();
    AnfModule { functions, ..module }
}
