use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};

use crate::codegen::emit::emit_user_module;
use crate::runtime;
use crate::wasm::emit::emit_wat;
use crate::wasm::linker::{LinkError, link_with_extern_modules};

pub fn build_file(file_path: &str, output: Option<&str>, emit_wat: bool) -> Result<()> {
    let wat = build_wat(file_path)?;
    let plan = resolve_output_plan(file_path, output, emit_wat)?;

    if let Some(out_path) = &plan.wat_out {
        fs::write(out_path, &wat)
            .with_context(|| format!("failed to write WAT output '{}'", out_path.display()))?;
    }

    if let Some(out_path) = &plan.wasm_out {
        assemble_wat_to_wasm(&wat, out_path)?;
    }

    println!("Building: {}", file_path);
    if let Some(out_path) = &plan.wasm_out {
        println!("Wasm output: {}", out_path.display());
    }
    if let Some(out_path) = &plan.wat_out {
        println!("WAT output: {}", out_path.display());
    }

    Ok(())
}

pub fn build_wat(file_path: &str) -> Result<String> {
    let pipeline = crate::backend_pipeline::compile_backend_opt(file_path)?;
    build_wat_from_core_module(pipeline.core_module)
}

/// Build WAT from an already-compiled CoreModule (useful for source-map compilation tests).
pub fn build_wat_from_core_module(core_module: crate::ir::core::CoreModule) -> Result<String> {
    let core_module = crate::ir::monomorphize(core_module);
    let anf_module = crate::ir::lower_anf::lower_module(&core_module);
    crate::ir::anf::verify::verify_module_or_panic(&anf_module, "post-lowering");
    let optimized_anf_module = crate::opt::optimize_module(anf_module);
    crate::ir::anf::verify::verify_module_or_panic(&optimized_anf_module, "post-optimization");

    let user_module = emit_user_module(&optimized_anf_module, &core_module.type_env);
    let mut modules = runtime::all_modules();
    modules.push(user_module);

    let extern_modules: std::collections::HashSet<String> = optimized_anf_module
        .extern_imports
        .values()
        .map(|ext| ext.wasm_module.clone())
        .collect();
    let linked =
        link_with_extern_modules(modules, None, &extern_modules).map_err(format_link_errors)?;
    Ok(emit_wat(&linked))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildOutputPlan {
    wasm_out: Option<PathBuf>,
    wat_out: Option<PathBuf>,
}

fn resolve_output_plan(
    file_path: &str,
    output: Option<&str>,
    emit_wat: bool,
) -> Result<BuildOutputPlan> {
    let out = match output {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(file_path).with_extension("wasm"),
    };
    match out.extension().and_then(|ext| ext.to_str()) {
        Some("wat") => Ok(BuildOutputPlan {
            wasm_out: None,
            wat_out: Some(out),
        }),
        Some("wasm") | None => Ok(BuildOutputPlan {
            wat_out: emit_wat.then(|| out.with_extension("wat")),
            wasm_out: Some(out),
        }),
        Some(ext) => bail!(
            "unsupported output extension '.{}' (use .wasm or .wat)",
            ext
        ),
    }
}

fn format_link_errors(errors: Vec<LinkError>) -> anyhow::Error {
    let msgs = errors
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    anyhow!("link errors:\n{msgs}")
}

fn assemble_wat_to_wasm(wat: &str, wasm_out: &Path) -> Result<()> {
    let wasm_bytes =
        wat::parse_str(wat).context("failed to assemble WAT to Wasm bytes using wat crate")?;
    fs::write(wasm_out, wasm_bytes)
        .with_context(|| format!("failed to write Wasm output '{}'", wasm_out.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_wat_for_smoke_fixtures_contains_module_and_start() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for fixture in [
            "hello.tw",
            "arithmetic.tw",
            "records.tw",
            "closures.tw",
            "for_break.tw",
            "capability_records.tw",
        ] {
            let path = root.join("tests/run").join(fixture);
            let wat = build_wat(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("build_wat failed for {}: {e}", path.display()));
            assert!(wat.contains("(module"), "missing module for {}", fixture);
            assert!(
                wat.contains("(export \"__twinkle_start\""),
                "missing linked start export for {}",
                fixture
            );
            assert!(
                wat.contains("$user__func_"),
                "missing user function for {}",
                fixture
            );
        }
    }

    #[test]
    fn resolve_output_defaults_to_wasm() {
        let out =
            resolve_output_plan("tests/run/hello.tw", None, false).expect("path resolution failed");
        assert_eq!(
            out,
            BuildOutputPlan {
                wasm_out: Some(PathBuf::from("tests/run/hello.wasm")),
                wat_out: None,
            }
        );
    }

    #[test]
    fn resolve_output_accepts_wat_and_wasm() {
        assert_eq!(
            resolve_output_plan("tests/run/hello.tw", Some("out.wat"), false).expect("wat output"),
            BuildOutputPlan {
                wasm_out: None,
                wat_out: Some(PathBuf::from("out.wat")),
            }
        );
        assert_eq!(
            resolve_output_plan("tests/run/hello.tw", Some("out.wasm"), false)
                .expect("wasm output"),
            BuildOutputPlan {
                wasm_out: Some(PathBuf::from("out.wasm")),
                wat_out: None,
            }
        );
    }

    #[test]
    fn resolve_output_rejects_unsupported_extension() {
        let err = resolve_output_plan("tests/run/hello.tw", Some("out.txt"), false).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported output extension '.txt'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_output_emits_wat_sidecar_for_wasm_targets() {
        assert_eq!(
            resolve_output_plan("tests/run/hello.tw", None, true).expect("default output"),
            BuildOutputPlan {
                wasm_out: Some(PathBuf::from("tests/run/hello.wasm")),
                wat_out: Some(PathBuf::from("tests/run/hello.wat")),
            }
        );
        assert_eq!(
            resolve_output_plan("tests/run/hello.tw", Some("custom.wasm"), true)
                .expect("custom wasm output"),
            BuildOutputPlan {
                wasm_out: Some(PathBuf::from("custom.wasm")),
                wat_out: Some(PathBuf::from("custom.wat")),
            }
        );
    }

    #[test]
    fn build_wat_extern_fn_emits_imports() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/run/extern_fn.tw");
        let wat = build_wat(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("build_wat failed for extern_fn.tw: {e}"));

        // Each extern declaration should produce a WASM import
        assert!(
            wat.contains(r#"(import "console" "log""#),
            "missing console.log import in WAT:\n{wat}"
        );
        assert!(
            wat.contains(r#"(import "math" "add""#),
            "missing math.add import in WAT:\n{wat}"
        );
        // Explicit env module import
        assert!(
            wat.contains(r#"(import "env" "env_helper""#),
            "missing env.env_helper import in WAT:\n{wat}"
        );
    }

    #[test]
    fn build_wat_extern_fn_emits_unused_imports() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/run/extern_fn_unused.tw");
        let wat = build_wat(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("build_wat failed for extern_fn_unused.tw: {e}"));

        assert!(
            wat.contains(r#"(import "unused" "ping""#),
            "missing unused.ping import in WAT:\n{wat}"
        );
    }

    #[test]
    fn build_wat_extern_types_validate_and_cross_anyref_boundaries() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/run/extern_types.tw");
        let wat = build_wat(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("build_wat failed for extern_types.tw: {e}"));

        assert!(
            wat.contains(r#"(import "dom" "get_element""#),
            "missing dom.get_element import in WAT:\n{wat}"
        );
        assert!(
            wat.contains(r#"(import "dom" "append_child""#),
            "missing dom.append_child import in WAT:\n{wat}"
        );
        assert!(
            wat.contains("(ref extern)"),
            "extern type should lower to non-null ref extern:\n{wat}"
        );
        assert!(
            wat.contains("any.convert_extern"),
            "storing extern refs in Vector should convert externref to anyref:\n{wat}"
        );
        assert!(
            wat.contains("extern.convert_any"),
            "loading extern refs from Vector should convert anyref to externref:\n{wat}"
        );

        wat::parse_str(&wat).expect("extern type WAT should assemble");
    }

    #[test]
    fn build_wat_extern_types_nullable_option_externref() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/run/extern_types_nullable.tw");
        let wat = build_wat(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("build_wat failed for extern_types_nullable.tw: {e}"));

        // Import returning Element? should use nullable externref at the boundary
        assert!(
            wat.contains(r#"(import "dom" "query_selector""#),
            "missing dom.query_selector import in WAT:\n{wat}"
        );
        assert!(
            wat.contains("(result (ref null extern))"),
            "extern fn returning Element? should have nullable externref result:\n{wat}"
        );

        // Bridge function wraps nullable externref into Variant
        assert!(
            wat.contains("any.convert_extern"),
            "bridge should convert externref to anyref for Variant payload:\n{wat}"
        );

        wat::parse_str(&wat).expect("nullable extern type WAT should assemble");
    }

    #[test]
    fn assemble_wat_to_wasm_writes_binary_module() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let out = std::env::temp_dir().join(format!(
            "twinkle-build-test-{}-{stamp}.wasm",
            std::process::id()
        ));
        assemble_wat_to_wasm("(module)", &out).expect("assemble should succeed");
        let bytes = fs::read(&out).expect("wasm output should exist");
        let _ = fs::remove_file(&out);
        assert!(bytes.starts_with(b"\0asm"), "missing wasm magic header");
    }
}
