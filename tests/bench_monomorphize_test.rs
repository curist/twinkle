//! Benchmark: compare WAT output before and after monomorphization.
//!
//! Run with `cargo test --test bench_monomorphize_test -- --nocapture` to see
//! the full report.
//!
//! This test asserts measurable improvements (fewer `anyref` locals in user
//! functions, specialized function names visible in the WAT) so it doubles as
//! a correctness gate.

use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::codegen::emit::emit_user_module;
use twinkle::ir::lower_anf::lower_module;
use twinkle::ir::monomorphize::monomorphize;
use twinkle::opt::optimize_module;
use twinkle::runtime;
use twinkle::wasm::{emit::emit_wat, linker::link};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// Build WAT **without** monomorphization (pre-9.5 behaviour).
fn build_wat_no_mono(file_path: &str) -> String {
    let (core_module, _) =
        twinkle::module::compile_entry(file_path).expect("compile failed");
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    emit_wat(&linked)
}

/// Build WAT **with** monomorphization (post-9.5 behaviour).
fn build_wat_with_mono(file_path: &str) -> String {
    let (core_module, _) =
        twinkle::module::compile_entry(file_path).expect("compile failed");
    let core_module = monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    emit_wat(&linked)
}

/// Count `anyref` occurrences in user-function bodies (excludes type/import
/// sections and runtime functions).
fn count_anyref_in_user_funcs(wat: &str) -> usize {
    let mut in_user = false;
    let mut depth: i32 = 0;
    let mut count = 0;
    for line in wat.lines() {
        let trimmed = line.trim();
        if trimmed.contains("$user__func_") && trimmed.starts_with("(func") {
            in_user = true;
            depth = 0;
        }
        if in_user {
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            in_user = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if in_user && trimmed.contains("anyref") {
                count += 1;
            }
        }
    }
    count
}

/// Count user-function definitions in the WAT.
fn count_user_funcs(wat: &str) -> usize {
    wat.lines()
        .filter(|l| l.trim().starts_with("(func $user__func_"))
        .count()
}


struct Report {
    label: &'static str,
    funcs_before: usize,
    funcs_after: usize,
    anyref_before: usize,
    anyref_after: usize,
    bytes_before: usize,
    bytes_after: usize,
}

impl Report {
    fn print(&self) {
        println!("\n=== Monomorphization benchmark: {} ===", self.label);
        println!(
            "  User functions : {:>4}  →  {:>4}  (Δ {:+})",
            self.funcs_before,
            self.funcs_after,
            self.funcs_after as i64 - self.funcs_before as i64
        );
        println!(
            "  anyref in funcs: {:>4}  →  {:>4}  (Δ {:+})",
            self.anyref_before,
            self.anyref_after,
            self.anyref_after as i64 - self.anyref_before as i64
        );
        println!(
            "  WAT bytes      : {:>6}  →  {:>6}  (Δ {:+})",
            self.bytes_before,
            self.bytes_after,
            self.bytes_after as i64 - self.bytes_before as i64
        );
    }
}

fn benchmark(label: &'static str, file: &str) -> Report {
    let path = fixture(file);
    let wat_before = build_wat_no_mono(&path);
    let wat_after = build_wat_with_mono(&path);
    Report {
        label,
        funcs_before: count_user_funcs(&wat_before),
        funcs_after: count_user_funcs(&wat_after),
        anyref_before: count_anyref_in_user_funcs(&wat_before),
        anyref_after: count_anyref_in_user_funcs(&wat_after),
        bytes_before: wat_before.len(),
        bytes_after: wat_after.len(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn mono_generic_user_funcs_reduces_anyref() {
    let r = benchmark("generic_user_funcs.tw", "generic_user_funcs.tw");
    r.print();

    // After monomorphization: fewer anyref in user functions (id, apply, first
    // are now specialized with concrete Wasm types).
    assert!(
        r.anyref_after < r.anyref_before,
        "expected fewer anyref after monomorphization: {} → {}",
        r.anyref_before,
        r.anyref_after,
    );

    // More user functions after mono: one specialization per (function, type-arg-tuple).
    // id is called with Int, String, Bool → 3 specializations.
    // apply is called once (fn(Int)Int, Int) → 1 specialization.
    // first is called once (String, Bool) → 1 specialization.
    assert!(
        r.funcs_after > r.funcs_before,
        "expected more functions after specialization: {} → {}",
        r.funcs_before,
        r.funcs_after,
    );
}

#[test]
fn mono_iterator_fixture_no_regressions() {
    let r = benchmark("iterator.tw", "iterator.tw");
    r.print();
    // Iterator fixture uses generics heavily (unfold, collect); monomorphization
    // should not increase anyref usage.
    assert!(
        r.anyref_after <= r.anyref_before,
        "monomorphization should not increase anyref: {} → {}",
        r.anyref_before,
        r.anyref_after,
    );
}

#[test]
fn mono_non_generic_fixture_is_identity() {
    // hello.tw has no user-defined generic functions; the module should come
    // out identical in structure (same function count).
    let r = benchmark("hello.tw", "hello.tw");
    r.print();
    assert_eq!(
        r.funcs_before, r.funcs_after,
        "non-generic program should have same function count after monomorphization"
    );
}
