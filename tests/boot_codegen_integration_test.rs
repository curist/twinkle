use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use twinkle::cli::build::build_wat;
use twinkle::cli::run_wasm::{build_engine, execute_module};
use twinkle::interp::Interpreter;
use twinkle::ir::core::CoreModule;
use twinkle::module::compile_entry;
use wasmtime::Module;

#[derive(Clone, Copy)]
enum RegressionInput {
    Inline(&'static str),
    Fixture(&'static str),
}

#[derive(Clone, Copy)]
struct RegressionCase {
    name: &'static str,
    input: RegressionInput,
}

const REGRESSION_CASES: &[RegressionCase] = &[
    // --- original inline cases ---
    RegressionCase {
        name: "smoke print",
        input: RegressionInput::Inline("fn main() Void { println(\"hi\") }\nmain()\n"),
    },
    RegressionCase {
        name: "direct return",
        input: RegressionInput::Inline(
            "fn choose() String { return \"yes\" }\nprintln(choose())\n",
        ),
    },
    RegressionCase {
        name: "string return boundary",
        input: RegressionInput::Inline("fn greet() String { \"hi\" }\nprintln(greet())\n"),
    },
    RegressionCase {
        name: "record field access",
        input: RegressionInput::Inline(
            "type Pair = .{ x: Int, y: Int }\nfn main() Void { p := Pair.{ x: 41, y: 1 }\nprintln(\"${p.x}\")\nprintln(\"${p.y}\") }\nmain()\n",
        ),
    },
    RegressionCase {
        name: "return inside if",
        input: RegressionInput::Inline(
            "fn choose(b: Bool) String {\n  if b { return \"yes\" }\n  \"no\"\n}\nprintln(choose(true))\nprintln(choose(false))\n",
        ),
    },
    RegressionCase {
        name: "return inside for",
        input: RegressionInput::Inline(
            "fn first_gt(xs: Vector<Int>, limit: Int) String {\n  for x in xs {\n    if x > limit { return \"found\" }\n  }\n  \"none\"\n}\nprintln(first_gt([1, 3, 7], 6))\nprintln(first_gt([1, 3], 6))\n",
        ),
    },
    RegressionCase {
        name: "qualified variant constructor",
        input: RegressionInput::Inline(
            "type Inner = { Hit, Miss }\ntype Outer = { Wrap(Inner), Empty }\nfn describe(o: Outer) String {\n  case o {\n    .Wrap(inner) => case inner {\n      .Hit => \"hit\",\n      .Miss => \"miss\",\n    },\n    .Empty => \"empty\",\n  }\n}\nprintln(describe(Outer.Wrap(Inner.Hit)))\n",
        ),
    },
    RegressionCase {
        name: "user sum zero-arg variant",
        input: RegressionInput::Inline(
            "type Color = { Red, Blue, Green }\nc := Color.Red\ncase c {\n  .Red => println(\"red\"),\n  .Blue => println(\"blue\"),\n  .Green => println(\"green\"),\n}\n",
        ),
    },
    RegressionCase {
        name: "option local construction and match",
        input: RegressionInput::Inline(
            "o: Option<Int> = .Some(42)\ncase o {\n  .Some(n) => println(\"got ${n}\"),\n  .None => println(\"none\"),\n}\n",
        ),
    },
    // --- fixture cases ---
    RegressionCase {
        name: "arithmetic",
        input: RegressionInput::Fixture("tests/run/arithmetic.tw"),
    },
    RegressionCase {
        name: "bitwise_ops",
        input: RegressionInput::Fixture("tests/run/bitwise_ops.tw"),
    },
    RegressionCase {
        name: "byte_arithmetic_promotion",
        input: RegressionInput::Fixture("tests/run/byte_arithmetic_promotion.tw"),
    },
    RegressionCase {
        name: "byte_int_comparison",
        input: RegressionInput::Fixture("tests/run/byte_int_comparison.tw"),
    },
    RegressionCase {
        name: "capability_records",
        input: RegressionInput::Fixture("tests/run/capability_records.tw"),
    },
    RegressionCase {
        name: "collect_while",
        input: RegressionInput::Fixture("tests/run/collect_while.tw"),
    },
    RegressionCase {
        name: "control_flow",
        input: RegressionInput::Fixture("tests/run/control_flow.tw"),
    },
    RegressionCase {
        name: "defer_basic",
        input: RegressionInput::Fixture("tests/run/defer_basic.tw"),
    },
    RegressionCase {
        name: "defer_capture",
        input: RegressionInput::Fixture("tests/run/defer_capture.tw"),
    },
    RegressionCase {
        name: "defer_if",
        input: RegressionInput::Fixture("tests/run/defer_if.tw"),
    },
    RegressionCase {
        name: "defer_return",
        input: RegressionInput::Fixture("tests/run/defer_return.tw"),
    },
    RegressionCase {
        name: "empty_vector",
        input: RegressionInput::Fixture("tests/run/empty_vector.tw"),
    },
    RegressionCase {
        name: "fib_perf",
        input: RegressionInput::Fixture("tests/run/fib_perf.tw"),
    },
    RegressionCase {
        name: "for_break",
        input: RegressionInput::Fixture("tests/run/for_break.tw"),
    },
    RegressionCase {
        name: "generic_types",
        input: RegressionInput::Fixture("tests/run/generic_types.tw"),
    },
    RegressionCase {
        name: "generic_user_funcs",
        input: RegressionInput::Fixture("tests/run/generic_user_funcs.tw"),
    },
    RegressionCase {
        name: "hello",
        input: RegressionInput::Fixture("tests/run/hello.tw"),
    },
    RegressionCase {
        name: "large_index_narrowing",
        input: RegressionInput::Fixture("tests/run/large_index_narrowing.tw"),
    },
    RegressionCase {
        name: "loops",
        input: RegressionInput::Fixture("tests/run/loops.tw"),
    },
    RegressionCase {
        name: "method_chaining",
        input: RegressionInput::Fixture("tests/run/method_chaining.tw"),
    },
    RegressionCase {
        name: "mutual_recursion",
        input: RegressionInput::Fixture("tests/run/mutual_recursion.tw"),
    },
    RegressionCase {
        name: "nested_field_update",
        input: RegressionInput::Fixture("tests/run/nested_field_update.tw"),
    },
    RegressionCase {
        name: "option_boundary_call",
        input: RegressionInput::Fixture("tests/run/option_boundary_call.tw"),
    },
    RegressionCase {
        name: "option_local_match",
        input: RegressionInput::Fixture("tests/run/option_local_match.tw"),
    },
    RegressionCase {
        name: "record_field_punning",
        input: RegressionInput::Fixture("tests/run/record_field_punning.tw"),
    },
    RegressionCase {
        name: "records",
        input: RegressionInput::Fixture("tests/run/records.tw"),
    },
    RegressionCase {
        name: "recursive_types",
        input: RegressionInput::Fixture("tests/run/recursive_types.tw"),
    },
    RegressionCase {
        name: "stderr_prelude",
        input: RegressionInput::Fixture("tests/run/stderr_prelude.tw"),
    },
    RegressionCase {
        name: "string_byte_semantics",
        input: RegressionInput::Fixture("tests/run/string_byte_semantics.tw"),
    },
    RegressionCase {
        name: "string_get",
        input: RegressionInput::Fixture("tests/run/string_get.tw"),
    },
    RegressionCase {
        name: "string_iteration_index",
        input: RegressionInput::Fixture("tests/run/string_iteration_index.tw"),
    },
    RegressionCase {
        name: "string_large_index_semantics",
        input: RegressionInput::Fixture("tests/run/string_large_index_semantics.tw"),
    },
    RegressionCase {
        name: "string_methods",
        input: RegressionInput::Fixture("tests/run/string_methods.tw"),
    },
    RegressionCase {
        name: "string_slice",
        input: RegressionInput::Fixture("tests/run/string_slice.tw"),
    },
    RegressionCase {
        name: "strings_escape",
        input: RegressionInput::Fixture("tests/run/strings_escape.tw"),
    },
    RegressionCase {
        name: "variant_collision",
        input: RegressionInput::Fixture("tests/run/variant_collision.tw"),
    },
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn helper_path() -> PathBuf {
    project_root().join("boot/tests/helpers/emit_boot_wat.tw")
}

fn compile_boot_helper() -> (String, CoreModule) {
    let helper = helper_path();
    let helper_text = helper
        .to_str()
        .expect("boot helper path should be valid UTF-8")
        .to_string();
    let (core_module, _registry) = compile_entry(&helper_text)
        .unwrap_or_else(|e| panic!("failed to compile boot helper {}: {e}", helper.display()));
    (helper_text, core_module)
}

fn temp_case_path(case: RegressionCase, source: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let slug = case.name.replace(' ', "_");
    let path = std::env::temp_dir().join(format!("twinkle_boot_codegen_{slug}_{stamp}.tw"));
    fs::write(&path, source).unwrap_or_else(|e| {
        panic!("failed to write temp source for {}: {e}", case.name);
    });
    path
}

fn case_path(case: RegressionCase) -> (PathBuf, bool) {
    match case.input {
        RegressionInput::Inline(source) => (temp_case_path(case, source), true),
        RegressionInput::Fixture(path) => (project_root().join(path), false),
    }
}

fn stage0_wat(path: &Path) -> String {
    build_wat(path.to_str().expect("fixture path should be valid UTF-8"))
        .unwrap_or_else(|e| panic!("stage0 build_wat failed for {}: {e}", path.display()))
}

fn boot_wat(path: &Path, helper_text: &str, core_module: &CoreModule) -> String {
    let argv = vec![
        helper_text.to_string(),
        path.to_str()
            .expect("fixture path should be valid UTF-8")
            .to_string(),
    ];
    let mut interp = Interpreter::new_with_argv(core_module.clone(), Vec::<u8>::new(), argv);
    interp
        .run()
        .unwrap_or_else(|e| panic!("boot helper failed for {}: {e}", path.display()));

    let stderr = String::from_utf8_lossy(interp.error_output()).to_string();
    assert!(
        stderr.is_empty(),
        "boot helper wrote unexpected stderr for {}:\n{}",
        path.display(),
        stderr
    );

    String::from_utf8(interp.into_output()).expect("boot helper output should be valid UTF-8")
}

fn run_with_large_stack(f: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .name("boot-codegen-integration".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .expect("join test thread");
}

#[test]
fn boot_codegen_emits_valid_wat_for_m11_regression_matrix() {
    run_with_large_stack(|| {
        let engine = build_engine().expect("build Wasmtime engine");
        let (helper_text, helper_module) = compile_boot_helper();
        let mut failures: Vec<String> = Vec::new();

        for &case in REGRESSION_CASES {
            let (path, is_temp) = case_path(case);
            let stage0 = stage0_wat(&path);
            let boot = boot_wat(&path, &helper_text, &helper_module);

            if let Err(e) = (|| -> Result<(), String> {
                let wasm = wat::parse_str(&stage0).map_err(|e| format!("stage0 WAT parse: {e}"))?;
                Module::new(&engine, &wasm).map_err(|e| format!("stage0 validate: {e}"))?;
                let wasm = wat::parse_str(&boot).map_err(|e| format!("boot WAT parse: {e}"))?;
                Module::new(&engine, &wasm).map_err(|e| format!("boot validate: {e}"))?;
                Ok(())
            })() {
                failures.push(format!("{}: {e}", case.name));
            }

            if is_temp {
                fs::remove_file(&path).ok();
            }
        }

        if !failures.is_empty() {
            panic!(
                "{}/{} cases failed WAT validation:\n  {}",
                failures.len(),
                REGRESSION_CASES.len(),
                failures.join("\n  ")
            );
        }
    });
}

#[test]
fn boot_codegen_return_inside_if_produces_valid_wat() {
    run_with_large_stack(|| {
        let engine = build_engine().expect("build Wasmtime engine");
        let (helper_text, helper_module) = compile_boot_helper();
        let source = "fn choose(b: Bool) String {\n  if b { return \"yes\" }\n  \"no\"\n}\nprintln(choose(true))\nprintln(choose(false))\n";
        let path = temp_case_path(
            RegressionCase {
                name: "return_inside_if",
                input: RegressionInput::Inline(source),
            },
            source,
        );
        let boot = boot_wat(&path, &helper_text, &helper_module);
        let wasm = wat::parse_str(&boot).expect("boot WAT should parse");
        Module::new(&engine, &wasm).expect("boot WAT should validate");
        fs::remove_file(&path).ok();
    });
}

#[test]
fn boot_codegen_matches_stage0_runtime_behavior_for_m11_regression_matrix() {
    run_with_large_stack(|| {
        let engine = build_engine().expect("build Wasmtime engine");
        let (helper_text, helper_module) = compile_boot_helper();
        let mut failures: Vec<String> = Vec::new();

        for &case in REGRESSION_CASES {
            let (path, is_temp) = case_path(case);

            if let Err(e) = (|| -> Result<(), String> {
                let s0_wasm = wat::parse_str(&stage0_wat(&path))
                    .map_err(|e| format!("stage0 WAT parse: {e}"))?;
                let s0_mod =
                    Module::new(&engine, &s0_wasm).map_err(|e| format!("stage0 validate: {e}"))?;
                let b_wasm = wat::parse_str(&boot_wat(&path, &helper_text, &helper_module))
                    .map_err(|e| format!("boot WAT parse: {e}"))?;
                let b_mod =
                    Module::new(&engine, &b_wasm).map_err(|e| format!("boot validate: {e}"))?;

                let s0_out =
                    execute_module(&engine, &s0_mod).map_err(|e| format!("stage0 exec: {e}"))?;
                let b_out =
                    execute_module(&engine, &b_mod).map_err(|e| format!("boot exec: {e}"))?;

                if b_out != s0_out {
                    return Err(format!(
                        "output mismatch:\n  stage0: {s0_out:?}\n  boot:   {b_out:?}"
                    ));
                }
                Ok(())
            })() {
                failures.push(format!("{}: {e}", case.name));
            }

            if is_temp {
                fs::remove_file(&path).ok();
            }
        }

        if !failures.is_empty() {
            panic!(
                "{}/{} cases failed behavioral equivalence:\n  {}",
                failures.len(),
                REGRESSION_CASES.len(),
                failures.join("\n  ")
            );
        }
    });
}
