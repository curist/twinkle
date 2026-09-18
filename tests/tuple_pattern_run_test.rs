//! End-to-end compile-and-run tests for stage0 tuple case-arm patterns
//! (Task 6: lower + codegen parity with boot).
//!
//! These tests actually compile a Twinkle program to Wasm via the stage0
//! pipeline and *execute* it under `wasmtime`, rather than just inspecting
//! WAT text — the two bugs analogous to boot's Task 3 (a slot-type-inference
//! fallback to generic anyref, and a record-vs-sum dispatch guard) only
//! surface at real Wasm runtime, not in static WAT-shape assertions.
//!
//! Stage0's CLI has no `run` subcommand and the Rust crate has no embedded
//! Wasm engine, so this harness shells out to the `wasmtime` CLI (must be on
//! `PATH`). A stage0-emitted module always links two host-import surfaces
//! regardless of what the source program uses:
//!   - `twinkle_runtime`: print/println/error/eprint/eprintln + f64_to_string
//!     (unconditionally imported by the always-linked `rt.core`/`rt.str`
//!     runtime modules)
//!   - `Math`: the full JS `Math.*` intrinsic surface (unconditionally
//!     imported by the auto-imported `@std.math` prelude)
//! Both are satisfied here with no-op stub modules (`--preload`) — the test
//! programs never call print or Float.to_string, so the stub bodies are
//! never actually exercised; they only need to type-check the import edge.
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

/// Stub for the always-linked `twinkle_runtime` host import surface.
/// `$rt_types__String` is `(array (mut i8))` in every stage0-emitted module
/// (see `src/runtime/types.rs`); reproduced here structurally so wasmtime's
/// cross-module GC type equivalence check accepts the import/export pairing.
/// All bodies are no-ops/dummies — the test programs never call print or
/// Float.to_string, so these are never actually invoked.
const STUB_TWINKLE_RUNTIME_WAT: &str = r#"(module
  (type $rt_types__String (array (mut i8)))
  (func $noop_str (param (ref null $rt_types__String)))
  (func $f64_to_string (param f64) (result (ref $rt_types__String))
    i32.const 0
    i32.const 0
    array.new $rt_types__String)
  (export "print" (func $noop_str))
  (export "println" (func $noop_str))
  (export "error" (func $noop_str))
  (export "eprint" (func $noop_str))
  (export "eprintln" (func $noop_str))
  (export "f64_to_string" (func $f64_to_string))
)"#;

/// Stub for the always-linked `Math` host import surface (`@std.math` is
/// auto-imported by the prelude regardless of use). Never actually invoked
/// by these test programs.
const STUB_MATH_WAT: &str = r#"(module
  (func $unary (param f64) (result f64) f64.const 0)
  (func $binary (param f64 f64) (result f64) f64.const 0)
  (export "acos" (func $unary))
  (export "acosh" (func $unary))
  (export "asin" (func $unary))
  (export "asinh" (func $unary))
  (export "atan" (func $unary))
  (export "atan2" (func $binary))
  (export "atanh" (func $unary))
  (export "cbrt" (func $unary))
  (export "cos" (func $unary))
  (export "cosh" (func $unary))
  (export "exp" (func $unary))
  (export "expm1" (func $unary))
  (export "fround" (func $unary))
  (export "hypot" (func $binary))
  (export "log" (func $unary))
  (export "log10" (func $unary))
  (export "log1p" (func $unary))
  (export "log2" (func $unary))
  (export "pow" (func $binary))
  (export "sign" (func $unary))
  (export "sin" (func $unary))
  (export "sinh" (func $unary))
  (export "tan" (func $unary))
  (export "tanh" (func $unary))
)"#;

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

/// Compile `src` through the stage0 pipeline. Panics (with the compile
/// error) if compilation fails — callers that want to assert compile
/// *failure* (the RED state) should call this expecting a panic, or inspect
/// `twinkle::cli::build::build_wat` directly.
fn compile_to_wat(src: &str) -> String {
    let tw_path = unique_path("src", "tw");
    fs::write(&tw_path, src).expect("failed to write source fixture");
    let result = twinkle::cli::build::build_wat(tw_path.to_str().unwrap());
    let _ = fs::remove_file(&tw_path);
    result.unwrap_or_else(|e| panic!("compile failed:\n{e}\n\nsource:\n{src}"))
}

/// Compile `src` and run it under `wasmtime`, invoking the linked-module
/// entry point (`__twinkle_start`, the exported top-level-statements
/// function — stage0 has no automatic Wasm `start` section). Returns the
/// process exit code: 0 means the module ran to completion without
/// trapping; a Wasm trap (e.g. our own `error("mismatch")` calls) exits
/// nonzero.
fn compile_and_run(src: &str) -> std::process::ExitStatus {
    let wat = compile_to_wat(src);

    let wat_path = unique_path("mod", "wat");
    fs::write(&wat_path, &wat).expect("failed to write compiled WAT");
    let runtime_stub_path = unique_path("twinkle_runtime", "wat");
    fs::write(&runtime_stub_path, STUB_TWINKLE_RUNTIME_WAT).expect("failed to write runtime stub");
    let math_stub_path = unique_path("math", "wat");
    fs::write(&math_stub_path, STUB_MATH_WAT).expect("failed to write Math stub");

    let output = Command::new("wasmtime")
        .args([
            "run",
            "-W",
            "gc=y",
            "-W",
            "function-references=y",
            "--preload",
            &format!("twinkle_runtime={}", runtime_stub_path.display()),
            "--preload",
            &format!("Math={}", math_stub_path.display()),
            "--invoke",
            "__twinkle_start",
        ])
        .arg(&wat_path)
        .output()
        .expect("failed to spawn `wasmtime` — it must be on PATH to run this test");

    let _ = fs::remove_file(&wat_path);
    let _ = fs::remove_file(&runtime_stub_path);
    let _ = fs::remove_file(&math_stub_path);

    if !output.status.success() {
        eprintln!(
            "wasmtime stderr for source:\n{src}\n---\n{}",
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
