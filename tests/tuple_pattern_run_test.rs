//! End-to-end compile-and-run tests for stage0 tuple case-arm patterns
//! (Task 6: lower + codegen parity with boot).
//!
//! These tests actually compile a Twinkle program to Wasm via the stage0
//! pipeline and *execute* it, rather than just inspecting WAT text — the two
//! bugs analogous to boot's Task 3 (a slot-type-inference fallback to generic
//! anyref, and a record-vs-sum dispatch guard), plus the Byte tuple-field
//! representation bug, only surface at real Wasm runtime, not in static
//! WAT-shape assertions.
//!
//! Execution goes through the project's own JS<->Wasm-GC runtime
//! (`tools/js_runtime/run_wasm_file.mjs`, invoked with `deno`), the same
//! `runWasmBytesAsync` path the boot CLI and boot test suite use. It provides
//! the full host-import surface a stage0-emitted module links
//! (`twinkle_runtime` console I/O + `f64_to_string`, and the `Math.*`
//! intrinsics pulled in by the auto-imported `@std.math` prelude), so no
//! stub modules are needed. `deno` is already a test dependency (see the boot
//! suite); no external Wasm engine is required.
//!
//! Each test program computes a value via a tuple pattern match and calls
//! `error("mismatch")` if it disagrees with the expected constant. A clean
//! exit (0) means the computed value matched; a Wasm trap (nonzero exit)
//! means it didn't — this lets the test assert on the actual *runtime
//! value*, not just that the program compiled.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_path(prefix: &str, ext: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "twinkle-tuple-pattern-{prefix}-{}-{stamp}-{seq}.{ext}",
        std::process::id()
    ))
}

/// Compile `src` through the stage0 pipeline to a binary Wasm module written
/// at `wasm_path`. Panics (with the compile error) if compilation fails —
/// callers that want to assert compile *failure* (the RED state) should call
/// `twinkle::cli::build::build_wat` directly and expect an `Err`.
fn compile_to_wasm(src: &str, wasm_path: &std::path::Path) {
    let tw_path = unique_path("src", "tw");
    fs::write(&tw_path, src).expect("failed to write source fixture");
    let result = twinkle::cli::build::build_file(
        tw_path.to_str().unwrap(),
        Some(wasm_path.to_str().unwrap()),
        false,
    );
    let _ = fs::remove_file(&tw_path);
    result.unwrap_or_else(|e| panic!("compile failed:\n{e}\n\nsource:\n{src}"));
}

/// Path to the Deno wasm runner shipped with the JS runtime.
fn wasm_runner_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/js_runtime/run_wasm_file.mjs")
}

/// Compile `src` and run it through the project's own JS<->Wasm-GC runtime
/// (`run_wasm_file.mjs` under `deno`), which invokes the module's
/// `__twinkle_start` export. Returns the process exit code: 0 means the
/// module ran to completion without trapping; a Wasm trap (e.g. our own
/// `error("mismatch")` calls) exits nonzero.
fn compile_and_run(src: &str) -> std::process::ExitStatus {
    let wasm_path = unique_path("mod", "wasm");
    compile_to_wasm(src, &wasm_path);

    let output = Command::new("deno")
        .args(["run", "--allow-read", "--allow-env", "--allow-write"])
        .arg(wasm_runner_script())
        .arg(&wasm_path)
        .output()
        .expect("failed to spawn `deno` — it must be on PATH to run this test");

    let _ = fs::remove_file(&wasm_path);

    if !output.status.success() {
        eprintln!(
            "deno runner stderr for source:\n{src}\n---\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output.status
}

/// Assert `src` runs to completion without trapping. Test programs compute
/// a value via a tuple pattern match and call `error("mismatch")` on the
/// non-matching branch, so a trap here means the computed value diverged
/// from what the test expected.
fn assert_program_matches_expected(src: &str) {
    let status = compile_and_run(src);
    assert!(
        status.success(),
        "program trapped (expected computed value did not match) for source:\n{src}"
    );
}

#[test]
fn tuple_pattern_flat_irrefutable_bind() {
    // (a, b) => a * 10 + b — every element position is a plain identifier
    // (irrefutable), so this is a single unconditional binding, no
    // tag/eq checks at all. Expected: 3*10+4 = 34.
    assert_program_matches_expected(
        r#"
fn f() Int {
  case (3, 4) {
    (a, b) => a * 10 + b,
  }
}
r := f()
if r != 34 {
  error("mismatch")
}
"#,
    );
}

#[test]
fn tuple_pattern_nested_variant_holding_tuple() {
    // .Some((key, val)) — a tuple pattern nested inside a sum pattern. The
    // scrutinee is a function *parameter* (not a module-level literal or
    // global) so the test exercises the pattern match itself, not any
    // unrelated literal-typing or global-erasure gap.
    assert_program_matches_expected(
        r#"
fn f(opt: (Int, Int)?) Int {
  case opt {
    .Some((key, val)) => key + val,
    .None => -1,
  }
}
r := f(.Some((1, 2)))
if r != 3 {
  error("mismatch")
}
"#,
    );
}

#[test]
fn tuple_pattern_refutable_fallthrough() {
    // (0, y) / (x, _) — a literal in the first position makes the first arm
    // refutable; a non-matching first element falls through to the second
    // arm's wildcard-tail.
    assert_program_matches_expected(
        r#"
fn f(t: (Int, Int)) Int {
  case t {
    (0, y) => y,
    (x, _) => x,
  }
}
r := f((0, 7))
if r != 7 {
  error("mismatch")
}
"#,
    );
    assert_program_matches_expected(
        r#"
fn f(t: (Int, Int)) Int {
  case t {
    (0, y) => y,
    (x, _) => x,
  }
}
r := f((5, 7))
if r != 5 {
  error("mismatch")
}
"#,
    );
}

#[test]
fn tuple_pattern_nested_sub_tuple() {
    // ((a, b), c) — a tuple pattern nested inside another tuple pattern.
    // Built via annotated bindings (not a bare nested tuple literal
    // scrutinee) so this exercises the nested *pattern*, independent of any
    // literal-synthesis behavior for nested tuple expressions.
    assert_program_matches_expected(
        r#"
fn g(pair: ((Int, Int), Int)) Int {
  case pair {
    ((a, b), c) => a + b + c,
  }
}
inner: (Int, Int) = (1, 2)
outer: ((Int, Int), Int) = (inner, 3)
r := g(outer)
if r != 6 {
  error("mismatch")
}
"#,
    );
}

#[test]
fn tuple_pattern_byte_field_rebound_rematch() {
    // A Byte-typed tuple field, bound then rebound and re-matched against a
    // Byte literal pattern. Guards a stage0 typed-representation precision bug
    // where the rebound Byte field's mono lost precision, so codegen emitted a
    // cast/coercion that did not match the stored representation and trapped at
    // runtime. Expected: the re-match hits the `65 =>` arm, yielding 100.
    assert_program_matches_expected(
        r#"
fn f(t: (Byte, Int)) Int {
  case t {
    (b, n) => {
      b2 := b
      case b2 {
        65 => 100,
        _ => n,
      }
    }
  }
}
first: Byte = 65
t: (Byte, Int) = (first, 7)
r := f(t)
if r != 100 {
  error("mismatch")
}
"#,
    );
}

/// `(a, b) := expr` — tuple-pattern let binding. Task 8 fixes the stage0
/// parser so this routes to `parse_let_stmt` instead of ICEing on `:=`
/// (previously mis-parsed as an infix operator). The checker + lowering for
/// tuple-pattern lets are Task 9/10, so this is `#[ignore]`d until then —
/// un-ignore in Task 11. Parse-level coverage for this task lives in
/// `src/syntax/parser.rs`'s test module (`tuple_let_binding_parses_as_tuple_pattern`).
#[test]
#[ignore = "enabled in Task 10 (stage0 lowering)"]
fn tuple_let_binding_runs() {
    assert_program_matches_expected(
        r#"
fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }
(q, r) := dm(17, 5)
if q * 100 + r != 302 {
  error("mismatch")
}
"#,
    );
}

/// Sanity check on the harness itself: a program whose `if` branch condition
/// is deliberately wrong must actually trap (nonzero exit), proving the
/// convention distinguishes "computed value matched" from "computed value
/// didn't match" rather than trivially always succeeding.
#[test]
fn harness_sanity_mismatch_traps() {
    let status = compile_and_run(
        r#"
fn f() Int {
  case (3, 4) {
    (a, b) => a * 10 + b,
  }
}
r := f()
if r != 999 {
  error("mismatch")
}
"#,
    );
    assert!(
        !status.success(),
        "expected a deliberately-wrong expected value to trap, but the program exited cleanly"
    );
}
