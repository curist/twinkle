use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use twinkle::cli::build::build_wat;
use twinkle::cli::run_wasm::{build_engine, execute_module};
use twinkle::interp::Interpreter;
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

const REGRESSION_CASES: [RegressionCase; 12] = [
    RegressionCase {
        name: "smoke print",
        input: RegressionInput::Inline("fn main() Void { println(\"hi\") }\nmain()\n"),
    },
    RegressionCase {
        name: "option boundary fixture",
        input: RegressionInput::Fixture("tests/run/option_boundary_call.tw"),
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
    RegressionCase {
        name: "nested field update fixture",
        input: RegressionInput::Fixture("tests/run/nested_field_update.tw"),
    },
    RegressionCase {
        name: "string get fixture",
        input: RegressionInput::Fixture("tests/run/string_get.tw"),
    },
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn helper_path() -> PathBuf {
    project_root().join("boot/tests/helpers/emit_boot_wat.tw")
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

fn boot_wat(path: &Path) -> String {
    let helper = helper_path();
    let helper_text = helper
        .to_str()
        .expect("boot helper path should be valid UTF-8");
    let (core_module, _registry) = compile_entry(helper_text)
        .unwrap_or_else(|e| panic!("failed to compile boot helper {}: {e}", helper.display()));

    let argv = vec![
        helper_text.to_string(),
        path.to_str()
            .expect("fixture path should be valid UTF-8")
            .to_string(),
    ];
    let mut interp = Interpreter::new_with_argv(core_module, Vec::<u8>::new(), argv);
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

fn validated_module(engine: &wasmtime::Engine, label: &str, wat: &str) -> Module {
    let wasm = wat::parse_str(wat).unwrap_or_else(|e| panic!("{label} WAT parse failed: {e}"));
    Module::new(engine, &wasm).unwrap_or_else(|e| panic!("{label} validation failed: {e}"))
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

        for case in REGRESSION_CASES {
            let (path, is_temp) = case_path(case);
            let stage0 = stage0_wat(&path);
            let boot = boot_wat(&path);

            let _stage0_module = validated_module(
                &engine,
                &format!("stage0 {} ({})", path.display(), case.name),
                &stage0,
            );
            let _boot_module = validated_module(
                &engine,
                &format!("boot {} ({})", path.display(), case.name),
                &boot,
            );

            if is_temp {
                fs::remove_file(&path).ok();
            }
        }
    });
}

#[test]
#[test]
fn debug_dump_return_if_wat() {
    run_with_large_stack(|| {
        let source = "fn choose(b: Bool) String {\n  if b { return \"yes\" }\n  \"no\"\n}\nprintln(choose(true))\nprintln(choose(false))\n";
        let path = temp_case_path(
            RegressionCase {
                name: "debug_return_if",
                input: RegressionInput::Inline(source),
            },
            source,
        );
        let s0 = stage0_wat(&path);
        let b0 = boot_wat(&path);
        eprintln!("=== STAGE0 WAT ===\n{s0}");
        eprintln!("=== BOOT WAT ===\n{b0}");
        fs::remove_file(&path).ok();
    });
}

#[test]
fn boot_codegen_matches_stage0_runtime_behavior_for_m11_regression_matrix() {
    run_with_large_stack(|| {
        let engine = build_engine().expect("build Wasmtime engine");

        for case in REGRESSION_CASES {
            let (path, is_temp) = case_path(case);
            let stage0_module = validated_module(
                &engine,
                &format!("stage0 {} ({})", path.display(), case.name),
                &stage0_wat(&path),
            );
            let boot_module = validated_module(
                &engine,
                &format!("boot {} ({})", path.display(), case.name),
                &boot_wat(&path),
            );

            let stage0_out = execute_module(&engine, &stage0_module).unwrap_or_else(|e| {
                panic!(
                    "stage0 execution failed for {} ({}): {e}",
                    path.display(),
                    case.name
                )
            });
            let boot_out = execute_module(&engine, &boot_module).unwrap_or_else(|e| {
                panic!(
                    "boot execution failed for {} ({}): {e}",
                    path.display(),
                    case.name
                )
            });

            assert_eq!(
                boot_out,
                stage0_out,
                "boot/runtime mismatch for {} ({})",
                path.display(),
                case.name
            );

            if is_temp {
                fs::remove_file(&path).ok();
            }
        }
    });
}
