use std::path::{Path, PathBuf};

use twinkle::cli::build::build_wat;
use twinkle::cli::run_wasm::{build_engine, execute_module};
use twinkle::interp::Interpreter;
use twinkle::module::compile_entry;
use wasmtime::Module;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn helper_path() -> PathBuf {
    project_root().join("boot/tests/helpers/emit_boot_wat.tw")
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

#[test]
fn boot_codegen_regresses_vector_method_cases() {
    let engine = build_engine().expect("build Wasmtime engine");

    for fixture in ["empty_vector.tw", "loops.tw", "method_chaining.tw"] {
        let path = project_root().join("tests/run").join(fixture);

        let s0_wat = stage0_wat(&path);
        let b_wat = boot_wat(&path);

        let s0_wasm =
            wat::parse_str(&s0_wat).unwrap_or_else(|e| panic!("stage0 WAT parse failed: {e}"));
        let b_wasm =
            wat::parse_str(&b_wat).unwrap_or_else(|e| panic!("boot WAT parse failed: {e}"));

        let s0_mod = Module::new(&engine, &s0_wasm)
            .unwrap_or_else(|e| panic!("stage0 validate failed for {}: {e}", path.display()));
        let b_mod = Module::new(&engine, &b_wasm)
            .unwrap_or_else(|e| panic!("boot validate failed for {}: {e}", path.display()));

        let s0_out = execute_module(&engine, &s0_mod)
            .unwrap_or_else(|e| panic!("stage0 exec failed for {}: {e}", path.display()));
        let b_out = execute_module(&engine, &b_mod)
            .unwrap_or_else(|e| panic!("boot exec failed for {}: {e}", path.display()));

        assert_eq!(b_out, s0_out, "output mismatch for {}", path.display());
    }
}
