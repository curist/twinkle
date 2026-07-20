/// Opt-in analysis: count COW operations in the boot compiler before/after optimization.
/// Run with: cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture
use twinkle::ir::anf::{AnfExpr, AnfModule, AnfOp, Atom};
use twinkle::ir::core::FuncId;

// COW operation FuncIds
const VECTOR_APPEND: FuncId = FuncId(11);
const VECTOR_SET_UNSAFE: FuncId = FuncId(12);
const DICT_SET: FuncId = FuncId(13);
const DICT_REMOVE: FuncId = FuncId(29);
const VECTOR_SET: FuncId = FuncId(39);

// In-place / builder FuncIds
const VECTOR_SET_IN_PLACE: FuncId = FuncId(1013);
const VECTOR_BUILDER_NEW: FuncId = FuncId(33);
const VECTOR_BUILDER_FROM: FuncId = FuncId(1014);
const VECTOR_BUILDER_PUSH: FuncId = FuncId(34);
const VECTOR_BUILDER_FREEZE: FuncId = FuncId(35);
const VECTOR_BUILDER_EXTEND: FuncId = FuncId(1100);
const DICT_SET_IN_PLACE: FuncId = FuncId(1015);
const DICT_REMOVE_IN_PLACE: FuncId = FuncId(1016);

// Record update
const VECTOR_CONCAT: FuncId = FuncId(25);

fn count_calls_to(module: &AnfModule, func_id: FuncId) -> usize {
    module
        .functions
        .iter()
        .map(|f| expr_count(&f.body, func_id))
        .sum()
}

fn expr_count(expr: &AnfExpr, func_id: FuncId) -> usize {
    match expr {
        AnfExpr::Let { op, body, .. } => op_count(op, func_id) + expr_count(body, func_id),
        _ => 0,
    }
}

fn op_count(op: &AnfOp, func_id: FuncId) -> usize {
    match op {
        AnfOp::ACall { callee, .. } => usize::from(*callee == Atom::AGlobalFunc(func_id)),
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_count(then_branch, func_id) + expr_count(else_branch, func_id),
        AnfOp::AMatch { arms, .. } => arms.iter().map(|a| expr_count(&a.body, func_id)).sum(),
        AnfOp::ALoop { body } => expr_count(body, func_id),
        _ => 0,
    }
}

/// Count record updates (ARecordUpdate) in the module, split by in-place vs COW.
fn count_record_updates(module: &AnfModule) -> (usize, usize) {
    module
        .functions
        .iter()
        .map(|f| expr_count_record_updates(&f.body))
        .fold((0, 0), |(a1, b1), (a2, b2)| (a1 + a2, b1 + b2))
}

fn expr_count_record_updates(expr: &AnfExpr) -> (usize, usize) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            let (a1, b1) = op_count_record_updates(op);
            let (a2, b2) = expr_count_record_updates(body);
            (a1 + a2, b1 + b2)
        }
        _ => (0, 0),
    }
}

fn op_count_record_updates(op: &AnfOp) -> (usize, usize) {
    match op {
        AnfOp::ARecordUpdate {
            can_reuse_in_place, ..
        } => {
            if *can_reuse_in_place {
                (1, 0) // in-place
            } else {
                (0, 1) // COW copy
            }
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            let (a1, b1) = expr_count_record_updates(then_branch);
            let (a2, b2) = expr_count_record_updates(else_branch);
            (a1 + a2, b1 + b2)
        }
        AnfOp::AMatch { arms, .. } => arms
            .iter()
            .map(|a| expr_count_record_updates(&a.body))
            .fold((0, 0), |(a1, b1), (a2, b2)| (a1 + a2, b1 + b2)),
        AnfOp::ALoop { body } => expr_count_record_updates(body),
        _ => (0, 0),
    }
}

/// Per-function breakdown for COW-heavy functions.
fn per_function_cow_counts(module: &AnfModule) -> Vec<(String, Vec<(&'static str, usize)>)> {
    let ops: &[(&str, FuncId)] = &[
        ("VECTOR_APPEND", VECTOR_APPEND),
        ("VECTOR_SET_UNSAFE", VECTOR_SET_UNSAFE),
        ("VECTOR_SET", VECTOR_SET),
        ("VECTOR_CONCAT", VECTOR_CONCAT),
        ("DICT_SET", DICT_SET),
        ("DICT_REMOVE", DICT_REMOVE),
        ("VECTOR_SET_IN_PLACE", VECTOR_SET_IN_PLACE),
        ("DICT_SET_IN_PLACE", DICT_SET_IN_PLACE),
        ("DICT_REMOVE_IN_PLACE", DICT_REMOVE_IN_PLACE),
        ("BUILDER_NEW", VECTOR_BUILDER_NEW),
        ("BUILDER_FROM", VECTOR_BUILDER_FROM),
        ("BUILDER_PUSH", VECTOR_BUILDER_PUSH),
        ("BUILDER_FREEZE", VECTOR_BUILDER_FREEZE),
        ("BUILDER_EXTEND", VECTOR_BUILDER_EXTEND),
    ];

    let mut results = Vec::new();
    for f in &module.functions {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        let mut total = 0;
        for &(name, id) in ops {
            let c = expr_count(&f.body, id);
            if c > 0 {
                counts.push((name, c));
                total += c;
            }
        }
        // Also count record updates
        let (in_place, cow) = expr_count_record_updates(&f.body);
        if in_place > 0 {
            counts.push(("REC_UPDATE_IN_PLACE", in_place));
        }
        if cow > 0 {
            counts.push(("REC_UPDATE_COW", cow));
            total += cow;
        }
        if total > 0 {
            results.push((f.name.clone(), counts));
        }
    }
    results.sort_by(|a, b| {
        let a_cow: usize =
            a.1.iter()
                .filter(|(n, _)| !n.contains("IN_PLACE") && !n.contains("BUILDER"))
                .map(|(_, c)| c)
                .sum();
        let b_cow: usize =
            b.1.iter()
                .filter(|(n, _)| !n.contains("IN_PLACE") && !n.contains("BUILDER"))
                .map(|(_, c)| c)
                .sum();
        b_cow.cmp(&a_cow)
    });
    results
}

#[test]
#[ignore = "slow opt-in analysis over boot/main.tw"]
fn analyze_checker_cow() {
    // Census over the self-hosted compiler itself. (boot/tests/main.tw, the
    // test suite, uses features the stage0 frontend can't parse; boot/main.tw is
    // the bootstrap target stage0 compiles, and is the realistic workload.)
    let path = "boot/main.tw";

    eprintln!("Compiling (pre-opt)...");
    let pre_opt = twinkle::backend_pipeline::compile_backend_anf(path)
        .expect("compile_backend_anf failed")
        .anf_module;

    eprintln!("Compiling (post-opt)...");
    let post_opt = twinkle::backend_pipeline::compile_backend_opt(path)
        .expect("compile_backend_opt failed")
        .optimized_anf_module;

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== COW Operation Analysis: boot/main.tw (self-hosted compiler) ===");
    eprintln!("{}\n", "=".repeat(70));

    let cow_ops: &[(&str, FuncId)] = &[
        ("VECTOR_APPEND", VECTOR_APPEND),
        ("VECTOR_SET_UNSAFE", VECTOR_SET_UNSAFE),
        ("VECTOR_SET", VECTOR_SET),
        ("VECTOR_CONCAT", VECTOR_CONCAT),
        ("DICT_SET", DICT_SET),
        ("DICT_REMOVE", DICT_REMOVE),
    ];
    let opt_ops: &[(&str, FuncId)] = &[
        ("VECTOR_SET_IN_PLACE", VECTOR_SET_IN_PLACE),
        ("DICT_SET_IN_PLACE", DICT_SET_IN_PLACE),
        ("DICT_REMOVE_IN_PLACE", DICT_REMOVE_IN_PLACE),
        ("BUILDER_NEW", VECTOR_BUILDER_NEW),
        ("BUILDER_FROM", VECTOR_BUILDER_FROM),
        ("BUILDER_PUSH", VECTOR_BUILDER_PUSH),
        ("BUILDER_FREEZE", VECTOR_BUILDER_FREEZE),
        ("BUILDER_EXTEND", VECTOR_BUILDER_EXTEND),
    ];

    eprintln!("--- PRE-OPTIMIZATION ---");
    for &(name, id) in cow_ops {
        let c = count_calls_to(&pre_opt, id);
        if c > 0 {
            eprintln!("  {name:30} {c:5}");
        }
    }
    let (rec_ip, rec_cow) = count_record_updates(&pre_opt);
    eprintln!(
        "  {:30} {:5}",
        "REC_UPDATE (all COW pre-opt)",
        rec_ip + rec_cow
    );

    eprintln!("\n--- POST-OPTIMIZATION ---");
    eprintln!("  COW (remaining):");
    let mut total_cow_remaining = 0;
    for &(name, id) in cow_ops {
        let c = count_calls_to(&post_opt, id);
        if c > 0 {
            eprintln!("    {name:28} {c:5}");
            total_cow_remaining += c;
        }
    }
    let (rec_ip, rec_cow) = count_record_updates(&post_opt);
    if rec_cow > 0 {
        eprintln!("    {:28} {:5}", "REC_UPDATE_COW", rec_cow);
        total_cow_remaining += rec_cow;
    }

    eprintln!("  Optimized (in-place/builder):");
    for &(name, id) in opt_ops {
        let c = count_calls_to(&post_opt, id);
        if c > 0 {
            eprintln!("    {name:28} {c:5}");
        }
    }
    if rec_ip > 0 {
        eprintln!("    {:28} {:5}", "REC_UPDATE_IN_PLACE", rec_ip);
    }

    eprintln!("\n  TOTAL COW remaining: {total_cow_remaining}");

    // Coarse guard on stage0's uniqueness optimizer over boot/main.tw.
    //
    // This total is NON-DETERMINISTIC: stage0's optimizer makes a run-to-run
    // varying number of in-place conversions (observed 1962..1970 on 2026-07-12),
    // so the count jitters by ~10. The ceiling therefore carries headroom above the
    // observed max and can only catch coarse regressions — a real regression
    // smaller than the jitter is invisible here. Read the per-op breakdown below to
    // interpret movement: an optimizer regression collapses the *_IN_PLACE/BUILDER
    // counts, whereas boot source growth raises COW and in-place roughly together.
    //
    // The total also grows with boot source size (absolute, not a ratio), so
    // re-baseline COW_CEILING (upward) for legitimate boot growth and lower it when
    // an optimizer change genuinely reduces the total — record the new number in
    // the commit/PR either way.
    //
    // Re-baselined 2026-07-12: 1696 (2026-06-03) -> 2000. The rise over 1696 was
    // boot source growth (in-place/builder counts healthy, not an optimizer
    // regression); the headroom over the ~1970 max absorbs the run-to-run jitter.
    //
    // Re-baselined 2026-07-19: 2000 -> 2200. This #[ignore] guard is not run by the
    // normal suites, so it drifted unnoticed: main itself measured 2014 (already over
    // 2000) and the uniqueness-rewrite branch measured ~2113. The rise is legitimate
    // boot source growth (the Phase 6 sound-uniqueness compiler stack) — comparing
    // main->branch, the in-place/builder counts grew MORE than COW remaining
    // (+257 in-place/builder vs +99 COW), the healthy-growth signature, not an
    // optimizer collapse. Headroom over ~2113 absorbs jitter; owned-move work in
    // progress should push this down, not up.
    const COW_CEILING: usize = 2200;
    assert!(
        total_cow_remaining <= COW_CEILING,
        "COW remaining {total_cow_remaining} exceeded ceiling {COW_CEILING}: check the per-op breakdown — collapsed IN_PLACE/BUILDER counts mean an optimizer regression; otherwise re-baseline for boot source growth"
    );

    // Per-function breakdown (top 30 COW-heavy functions, post-opt)
    eprintln!("\n--- PER-FUNCTION BREAKDOWN (post-opt, top 30 COW-heaviest) ---");
    let per_func = per_function_cow_counts(&post_opt);
    for (i, (name, counts)) in per_func.iter().take(30).enumerate() {
        let cow_count: usize = counts
            .iter()
            .filter(|(n, _)| !n.contains("IN_PLACE") && !n.contains("BUILDER"))
            .map(|(_, c)| c)
            .sum();
        let detail: Vec<String> = counts.iter().map(|(n, c)| format!("{n}={c}")).collect();
        eprintln!(
            "  {:3}. {:50} cow={:3}  [{}]",
            i + 1,
            name,
            cow_count,
            detail.join(", ")
        );
    }

    eprintln!("\nTotal functions: {}", post_opt.functions.len());
}
