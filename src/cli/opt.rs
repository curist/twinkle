use std::path::Path;

use anyhow::Result;

/// Optimise a Twinkle source file and print the resulting ANF IR.
///
/// Pipeline: parse → resolve → typecheck → lower (Core IR) → monomorphize →
/// lower_anf → optimize_module.
/// With `--show-original`, also prints the post-monomorphization ANF before the optimized form.
pub fn cmd_opt(path: &Path, show_original: bool) -> Result<()> {
    let path_str = path.to_string_lossy();

    match crate::backend_pipeline::compile_backend_opt(&path_str) {
        Ok(pipeline) => {
            let anf_module = pipeline.anf_module;
            if show_original {
                println!(
                    "// ── Original ANF ─────────────────────────────────────────────────────────────"
                );
                print!("{}", anf_module);
                println!(
                    "// ── Optimized ANF ────────────────────────────────────────────────────────────"
                );
            }
            print!("{}", pipeline.optimized_anf_module);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            anyhow::bail!("opt failed");
        }
    }
}
