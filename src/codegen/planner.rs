//! Module-level emit plan builder.
//!
//! Separates the "what to emit" planning from the "how to emit" instruction
//! generation.  `build_module_emit_plan` collects all pre-emission data that
//! `emit_user_module` currently computes inline, then `ModuleEmitPlan::emit_wat`
//! feeds that plan to the emitter.

use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};

use crate::codegen::ctx::{FuncSigInfo, IteratorStateInfo};
use crate::codegen::emit;
use crate::ir::anf::AnfModule;
use crate::ir::{FuncId, LocalId};
use crate::types::env::TypeEnv;
use crate::types::ty::MonoType;
/// Pre-computed planning data for module emission.
///
/// Contains everything the emitter needs to produce a `ModuleIR` without
/// performing any analysis itself.
#[derive(Debug, Clone)]
pub struct ModuleEmitPlan {
    /// Concrete closure signatures (func_id → (param_types, return_type)).
    pub concrete_func_sigs: HashMap<FuncId, (Vec<MonoType>, MonoType)>,
    /// Closure capture layouts (func_id → ordered captured locals).
    pub closure_capture_layouts: HashMap<FuncId, Vec<LocalId>>,
    /// Function ABI signatures for all user functions.
    pub user_sigs: HashMap<FuncId, FuncSigInfo>,
    /// Module-level global local IDs (init-bound, referenced outside init).
    pub module_global_ids: Vec<LocalId>,
    /// Mapping from global local ID to its Wasm symbol name.
    pub module_global_map: HashMap<LocalId, String>,
    /// Per-function capture monotype maps (func_id → local_id → MonoType).
    pub capture_mono_by_func: HashMap<FuncId, HashMap<LocalId, MonoType>>,
    /// Iterator state info for user functions returning concrete iterators.
    pub user_func_iterator_states: HashMap<FuncId, IteratorStateInfo>,
    /// Typed Cell payloads to register (sym → elem MonoType).
    pub typed_cell_payloads: BTreeMap<String, MonoType>,
}

impl ModuleEmitPlan {
    /// Emit WAT from this plan.  Equivalent to `emit_user_module` but using
    /// pre-computed plan data instead of inline analysis.
    pub fn emit_wat(&self, anf: &AnfModule, type_env: &TypeEnv) -> String {
        let exported_names = HashSet::new();
        let module_ir =
            emit::emit_named_module_from_plan(self, anf, type_env, "user", &exported_names);
        let mut modules = crate::runtime::all_modules();
        modules.extend(
            crate::compiler_lib::all_modules()
                .expect("compiler-owned library modules should build"),
        );
        modules.push(module_ir);
        let linked = crate::wasm::linker::link(modules, None).expect("link should succeed");
        crate::wasm::emit::emit_wat(&linked)
    }
}

/// Build a `ModuleEmitPlan` by running all analysis/collection passes.
pub fn build_module_emit_plan(anf: &AnfModule, type_env: &TypeEnv) -> ModuleEmitPlan {
    emit::build_module_emit_plan_impl(anf, type_env)
}
