use anyhow::{Context, Result};

use crate::ir::anf::AnfModule;
use crate::ir::anf::verify::verify_module_or_panic;
use crate::ir::core::CoreModule;

#[derive(Debug, Clone)]
pub struct BackendAnfPipeline {
    pub core_module: CoreModule,
    pub anf_module: AnfModule,
}

#[derive(Debug, Clone)]
pub struct BackendOptPipeline {
    pub core_module: CoreModule,
    pub anf_module: AnfModule,
    pub optimized_anf_module: AnfModule,
}

/// Compile through the canonical backend boundary:
/// parse -> resolve -> typecheck -> lower (Core IR) -> monomorphize -> lower (ANF).
pub fn compile_backend_anf(file_path: &str) -> Result<BackendAnfPipeline> {
    let (core_module, _registry) = crate::module::compile_entry(file_path)
        .with_context(|| format!("compile failed for '{file_path}'"))?;
    let core_module = crate::ir::monomorphize(core_module);
    let anf_module = crate::ir::lower_anf::lower_module(&core_module);
    verify_module_or_panic(&anf_module, "post-lowering");

    Ok(BackendAnfPipeline {
        core_module,
        anf_module,
    })
}

/// Compile through the canonical backend optimization boundary:
/// parse -> resolve -> typecheck -> lower (Core IR) -> monomorphize -> lower (ANF) -> optimize.
pub fn compile_backend_opt(file_path: &str) -> Result<BackendOptPipeline> {
    let pipeline = compile_backend_anf(file_path)?;
    let optimized_anf_module = crate::opt::optimize_module(pipeline.anf_module.clone());
    verify_module_or_panic(&optimized_anf_module, "post-optimization");

    Ok(BackendOptPipeline {
        core_module: pipeline.core_module,
        anf_module: pipeline.anf_module,
        optimized_anf_module,
    })
}
