use std::path::Path;

use anyhow::Result;

/// Lower a Twinkle source file to ANF IR and print it.
///
/// The pipeline: parse → resolve → typecheck → lower (Core IR) → lower_anf (ANF IR).
/// This follows the same pipeline as `twk lower` but adds the ANF lowering pass.
pub fn cmd_lower_anf(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();

    match crate::module::compile_entry(&path_str) {
        Ok((core_module, _registry)) => {
            let anf_module = crate::ir::lower_anf::lower_module(&core_module);
            print!("{}", anf_module);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            anyhow::bail!("lower-anf failed");
        }
    }
}
