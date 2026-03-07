use std::path::Path;

use anyhow::Result;

/// Lower a Twinkle source file to ANF IR and print it.
///
/// The backend-oriented pipeline: parse → resolve → typecheck → lower (Core IR) →
/// monomorphize → lower_anf (ANF IR).
pub fn cmd_lower_anf(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();

    match crate::backend_pipeline::compile_backend_anf(&path_str) {
        Ok(pipeline) => {
            print!("{}", pipeline.anf_module);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            anyhow::bail!("lower-anf failed");
        }
    }
}
