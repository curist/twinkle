/// Opt-in analyses for COW/mutable-lowering progress.
/// Run with: cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture
use std::collections::BTreeMap;
use std::process::Command;

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

/// Per-function breakdown for COW-heavy functions in the Rust stage0 ANF optimizer.
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

    // This stage0 number is diagnostic only. It is intentionally not guarded by
    // a ceiling: the count is an absolute total over the growing boot compiler,
    // stage0's old ANF rewrite pass has run-to-run jitter, and boot mutable
    // codegen rewiring is measured by the separate mutable-decision audit below.

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

const STALE_TWK_MESSAGE: &str = "missing `mutable decisions` section; target/twk is stale or was built before the mutable codegen audit. Rebuild with `make bundle-cli` or `make quick-bundle-cli`.";

#[derive(Debug, Default, PartialEq, Eq)]
struct MutableFamilySummary {
    sites: usize,
    selected: usize,
    fallback: usize,
    emits_mutable: usize,
    emits_persistent: usize,
    reasons: BTreeMap<String, usize>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MutableFunctionSummary {
    sites: usize,
    selected: usize,
    fallback: usize,
    emits_mutable: usize,
    emits_persistent: usize,
    reasons: BTreeMap<String, usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct MutableAuditRow {
    func: String,
    local: String,
    family: String,
    emit: String,
    state: String,
    reason: String,
    proof: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MutableCodegenAudit {
    total_sites: usize,
    selected: usize,
    policy_disabled: usize,
    stale_or_ignored: usize,
    absent_fallback: usize,
    emits_mutable: usize,
    emits_persistent: usize,
    fallback_by_reason: BTreeMap<String, usize>,
    by_family: BTreeMap<String, MutableFamilySummary>,
    by_function: BTreeMap<String, MutableFunctionSummary>,
    fallback_rows: Vec<MutableAuditRow>,
}

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}

fn parse_mutable_codegen_audit(output: &str) -> Result<MutableCodegenAudit, String> {
    let mut in_mutable_decisions = false;
    let mut saw_header = false;
    let mut counts = MutableCodegenAudit::default();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "mutable decisions" {
            in_mutable_decisions = true;
            continue;
        }
        if !in_mutable_decisions || trimmed.is_empty() {
            continue;
        }

        let cols: Vec<&str> = line.split('\t').collect();
        if !saw_header {
            let expected = [
                "func",
                "local",
                "family",
                "persistent",
                "mutable",
                "emit",
                "state",
                "reason",
                "proof",
            ];
            if cols != expected {
                return Err(format!(
                    "unexpected mutable decisions header: {line:?}; target/twk may be stale or the audit format changed"
                ));
            }
            saw_header = true;
            continue;
        }

        if cols.len() != 9 {
            return Err(format!("malformed mutable decisions row: {line:?}"));
        }

        let func = cols[0];
        let local = cols[1];
        let family = cols[2];
        let persistent = cols[3];
        let mutable = cols[4];
        let emit = cols[5];
        let state = cols[6];
        let reason = cols[7];
        let proof = cols[8];

        counts.total_sites += 1;
        let family_summary = counts.by_family.entry(family.to_string()).or_default();
        family_summary.sites += 1;
        let function_summary = counts.by_function.entry(func.to_string()).or_default();
        function_summary.sites += 1;

        match state {
            "selected" => {
                counts.selected += 1;
                family_summary.selected += 1;
                function_summary.selected += 1;
            }
            "policy_disabled" => counts.policy_disabled += 1,
            "stale_or_ignored" => counts.stale_or_ignored += 1,
            "absent_fallback" => counts.absent_fallback += 1,
            other => {
                return Err(format!(
                    "unknown mutable audit state {other:?} in row: {line:?}"
                ));
            }
        }

        if emit == mutable {
            counts.emits_mutable += 1;
            family_summary.emits_mutable += 1;
            function_summary.emits_mutable += 1;
        } else if emit == persistent {
            counts.emits_persistent += 1;
            family_summary.emits_persistent += 1;
            function_summary.emits_persistent += 1;
        } else {
            return Err(format!(
                "mutable audit row emits neither persistent nor mutable target: {line:?}"
            ));
        }

        if state != "selected" {
            family_summary.fallback += 1;
            function_summary.fallback += 1;
            increment(&mut family_summary.reasons, reason);
            increment(&mut function_summary.reasons, reason);
            increment(&mut counts.fallback_by_reason, reason);
            counts.fallback_rows.push(MutableAuditRow {
                func: func.to_string(),
                local: local.to_string(),
                family: family.to_string(),
                emit: emit.to_string(),
                state: state.to_string(),
                reason: reason.to_string(),
                proof: proof.to_string(),
            });
        }
    }

    if !in_mutable_decisions {
        return Err(STALE_TWK_MESSAGE.to_string());
    }
    if !saw_header {
        return Err("missing mutable decisions table header".to_string());
    }

    Ok(counts)
}

fn run_twk_ir_census_sites(path: &str) -> Result<String, String> {
    let output = Command::new("target/twk")
        .args(["ir", "--census", "--sites", path])
        .output()
        .map_err(|e| format!("failed to run target/twk: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!(
            "target/twk ir --census --sites {path} failed with status {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status
        ));
    }

    Ok(stdout)
}

fn mutable_codegen_fixture_path(name: &str) -> String {
    format!("boot/tests/fixtures/sound_uniqueness/{name}.tw")
}

fn mutable_codegen_fast_analysis_paths() -> Vec<String> {
    vec![
        mutable_codegen_fixture_path("phase8a_vector_set_fresh"),
        mutable_codegen_fixture_path("phase8a_vector_set_alias"),
        mutable_codegen_fixture_path("phase8a_vector_set_loop"),
    ]
}

fn render_mutable_codegen_analysis(path: &str, counts: &MutableCodegenAudit) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n=== Boot Mutable Codegen Rewiring Audit: {path} ===\n"
    ));
    out.push_str(&format!(
        "  audited sites:          {}\n",
        counts.total_sites
    ));
    out.push_str(&format!("  selected mutable emit:  {}\n", counts.selected));
    out.push_str(&format!(
        "  emitted mutable target: {}\n",
        counts.emits_mutable
    ));
    out.push_str(&format!(
        "  emitted persistent:     {}\n",
        counts.emits_persistent
    ));
    out.push_str(&format!(
        "  fallback states: policy_disabled={} stale_or_ignored={} absent_fallback={}\n",
        counts.policy_disabled, counts.stale_or_ignored, counts.absent_fallback
    ));

    out.push_str("\n--- Families ---\n");
    if counts.by_family.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let mut families: Vec<_> = counts.by_family.iter().collect();
        families.sort_by(|a, b| b.1.sites.cmp(&a.1.sites).then_with(|| a.0.cmp(b.0)));
        for (family, summary) in families {
            out.push_str(&format!(
                "  {family:24} sites={} selected={} fallback={} mutable_emit={} persistent_emit={}\n",
                summary.sites,
                summary.selected,
                summary.fallback,
                summary.emits_mutable,
                summary.emits_persistent
            ));
        }
    }

    out.push_str("\n--- Fallback reasons ---\n");
    if counts.fallback_by_reason.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let mut reasons: Vec<_> = counts.fallback_by_reason.iter().collect();
        reasons.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (reason, count) in reasons {
            out.push_str(&format!("  {reason:28} {count}\n"));
        }
    }

    out.push_str("\n--- Functions with persistent mutable-candidate fallback ---\n");
    let mut functions: Vec<_> = counts
        .by_function
        .iter()
        .filter(|(_, summary)| summary.fallback > 0)
        .collect();
    functions.sort_by(|a, b| {
        b.1.fallback
            .cmp(&a.1.fallback)
            .then_with(|| b.1.sites.cmp(&a.1.sites))
            .then_with(|| a.0.cmp(b.0))
    });
    if functions.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for (func, summary) in functions.iter().take(30) {
            let reason_detail = summary
                .reasons
                .iter()
                .map(|(reason, count)| format!("{reason}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "  {func:50} selected={} fallback={} sites={} [{}]\n",
                summary.selected, summary.fallback, summary.sites, reason_detail
            ));
        }
    }

    out.push_str("\n--- Example fallback sites ---\n");
    if counts.fallback_rows.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for row in counts.fallback_rows.iter().take(40) {
            out.push_str(&format!(
                "  {} {} family={} state={} reason={} emit={} proof={}\n",
                row.func, row.local, row.family, row.state, row.reason, row.emit, row.proof
            ));
        }
    }

    out
}

fn analyze_mutable_codegen_path(path: &str) {
    let output = match run_twk_ir_census_sites(path) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("\n=== Boot Mutable Codegen Rewiring Audit: {path} ===");
            eprintln!("  unavailable: {err}");
            return;
        }
    };
    let counts = match parse_mutable_codegen_audit(&output) {
        Ok(counts) => counts,
        Err(err) if err == STALE_TWK_MESSAGE => {
            eprintln!("\n=== Boot Mutable Codegen Rewiring Audit: {path} ===");
            eprintln!("  unavailable: {err}");
            return;
        }
        Err(err) => panic!("{err}"),
    };

    eprint!("{}", render_mutable_codegen_analysis(path, &counts));
}

#[test]
#[ignore = "opt-in mutable-codegen audit over small rewiring fixtures; requires fresh target/twk for audit section"]
fn analyze_boot_mutable_codegen_rewiring_effectiveness() {
    for path in mutable_codegen_fast_analysis_paths() {
        analyze_mutable_codegen_path(&path);
    }
}

#[test]
#[ignore = "heavy opt-in mutable-codegen audit over boot/main.tw; set TWINKLE_COW_ANALYZE_BOOT_MAIN=1"]
fn analyze_boot_main_mutable_codegen_rewiring_effectiveness() {
    if std::env::var("TWINKLE_COW_ANALYZE_BOOT_MAIN").as_deref() != Ok("1") {
        eprintln!(
            "\n=== Boot Mutable Codegen Rewiring Audit: boot/main.tw ===\n  skipped: set TWINKLE_COW_ANALYZE_BOOT_MAIN=1 to run the whole-boot audit"
        );
        return;
    }

    analyze_mutable_codegen_path("boot/main.tw");
}

#[test]
fn mutable_codegen_default_analysis_paths_are_fast_fixtures() {
    let paths = mutable_codegen_fast_analysis_paths();

    assert_eq!(paths.len(), 3);
    assert!(paths.iter().all(|p| p.contains("phase8a_vector_set_")));
    assert!(paths.iter().all(|p| !p.contains("boot/main.tw")));
}

#[test]
fn parse_mutable_codegen_audit_counts_all_families() {
    let output = "family\tcandidates\tin_place\n\
vector_set\t4\t0\n\
mutable decisions\n\
func\tlocal\tfamily\tpersistent\tmutable\temit\tstate\treason\tproof\n\
fresh\tL3\tvector_set\tvector$set_unsafe\tvector$set_in_place\tvector$set_in_place\tselected\tMutableSelected\tphase8a:fresh:L3\n\
alias\tL4\tvector_set\tvector$set_unsafe\tvector$set_in_place\tvector$set_unsafe\tabsent_fallback\tAbsent\t-\n\
stale\tL5\tvector_set\tvector$set_unsafe\tvector$set_in_place\tvector$set_unsafe\tstale_or_ignored\tSourceLocalMismatch\tphase8a:stale:L5\n\
dict\tL6\tdict_set\tDict.set\tdict$set_in_place\tDict.set\tpolicy_disabled\tPolicyDisabled\tphase8a:dict:L6\n";

    let counts = parse_mutable_codegen_audit(output).expect("parse audit counts");

    assert_eq!(counts.total_sites, 4);
    assert_eq!(counts.selected, 1);
    assert_eq!(counts.policy_disabled, 1);
    assert_eq!(counts.stale_or_ignored, 1);
    assert_eq!(counts.absent_fallback, 1);
    assert_eq!(counts.emits_mutable, 1);
    assert_eq!(counts.emits_persistent, 3);
    assert_eq!(
        counts
            .by_family
            .get("vector_set")
            .expect("vector_set family")
            .sites,
        3
    );
    assert_eq!(
        counts
            .by_family
            .get("dict_set")
            .expect("dict_set family")
            .sites,
        1
    );
    assert_eq!(counts.fallback_by_reason.get("Absent"), Some(&1));
    assert_eq!(
        counts.fallback_by_reason.get("SourceLocalMismatch"),
        Some(&1)
    );
    assert_eq!(counts.fallback_by_reason.get("PolicyDisabled"), Some(&1));

    let fresh = counts.by_function.get("fresh").expect("fresh summary");
    assert_eq!(fresh.selected, 1);
    assert_eq!(fresh.fallback, 0);

    let alias = counts.by_function.get("alias").expect("alias summary");
    assert_eq!(alias.selected, 0);
    assert_eq!(alias.fallback, 1);
    assert_eq!(alias.reasons.get("Absent"), Some(&1));

    let stale = counts.by_function.get("stale").expect("stale summary");
    assert_eq!(stale.selected, 0);
    assert_eq!(stale.fallback, 1);
    assert_eq!(stale.reasons.get("SourceLocalMismatch"), Some(&1));

    let dict = counts.by_function.get("dict").expect("dict summary");
    assert_eq!(dict.selected, 0);
    assert_eq!(dict.fallback, 1);
    assert_eq!(dict.reasons.get("PolicyDisabled"), Some(&1));
}
