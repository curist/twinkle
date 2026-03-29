use std::collections::HashSet;

use anyhow::{Context, Result};

use crate::codegen::emit::emit_named_module;
use crate::ir::anf::verify::verify_module_or_panic;
use crate::wasm::ir::ModuleIR;

pub const BOOTLIB_VECTOR_I64_NAMESPACE: &str = "bootlib.vector_i64";
const BOOTLIB_VECTOR_I64_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/boot/lib/vector_i64.tw");

pub fn all_modules() -> Result<Vec<ModuleIR>> {
    Ok(vec![compile_vector_i64_module()?])
}

pub fn compile_vector_i64_module() -> Result<ModuleIR> {
    let (core_module, exports, _registry) =
        crate::module::compile_entry_library(BOOTLIB_VECTOR_I64_PATH).with_context(|| {
            format!(
                "compile failed for compiler-owned library '{}'",
                BOOTLIB_VECTOR_I64_PATH
            )
        })?;
    let core_module = crate::ir::monomorphize(core_module);
    let anf_module = crate::ir::lower_anf::lower_module(&core_module);
    verify_module_or_panic(&anf_module, "post-lowering compiler library");
    let optimized_anf_module = crate::opt::optimize_module(anf_module);
    verify_module_or_panic(&optimized_anf_module, "post-optimization compiler library");
    let exported_names = exports
        .public_functions
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    Ok(emit_named_module(
        &optimized_anf_module,
        &core_module.type_env,
        BOOTLIB_VECTOR_I64_NAMESPACE,
        &exported_names,
    ))
}
