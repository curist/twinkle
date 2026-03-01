use std::path::Path;

use anyhow::Result;

/// Optimise a Twinkle source file and print the resulting ANF IR.
///
/// Pipeline: parse → resolve → typecheck → lower (Core IR) → lower_anf → optimize_module.
/// With `--show-original`, also prints the unoptimized ANF before the optimized form.
pub fn cmd_opt(path: &Path, show_original: bool) -> Result<()> {
    let path_str = path.to_string_lossy();

    match crate::module::compile_entry(&path_str) {
        Ok((core_module, _registry)) => {
            let anf_module = crate::ir::lower_anf::lower_module(&core_module);
            if show_original {
                println!("// ── Original ANF ─────────────────────────────────────────────────────────────");
                print!("{}", anf_module);
                println!("// ── Optimized ANF ────────────────────────────────────────────────────────────");
            }
            let optimized = crate::opt::optimize_module(anf_module);
            print!("{}", optimized);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            anyhow::bail!("opt failed");
        }
    }
}
