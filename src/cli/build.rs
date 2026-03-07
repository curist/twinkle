use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};

use crate::codegen::emit::emit_user_module_typed;
use crate::ir::lower_anf::lower_module;
use crate::opt::optimize_module;
use crate::runtime;
use crate::wasm::emit::emit_wat;
use crate::wasm::linker::{LinkError, link};

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
    let (core_module, _registry) = crate::module::compile_entry(file_path)
        .with_context(|| format!("compile failed for '{}'", file_path))?;
    let core_module = crate::ir::monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);

    let func_table = HashMap::new();
    let user_module = emit_user_module_typed(&optimized, &core_module.type_env, &func_table);
    let mut modules = runtime::all_modules();
    modules.push(user_module);

    let linked = link(modules, None).map_err(format_link_errors)?;
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
                wat.contains("(start $__linked_init)"),
                "missing linked start for {}",
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
