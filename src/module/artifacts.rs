use crate::ir::core::{FuncId, FunctionDef};
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::type_map::TypeMap;

/// Output of `Resolver::resolve`
pub struct ResolvedModule {
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
}

/// Output of `TypeChecker::check_module`
pub struct TypedModule {
    pub type_map: TypeMap,
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
}

/// Output of `Lowerer::lower_module_funcs`
pub struct LoweredModule {
    pub functions: Vec<FunctionDef>,
    pub init_func_id: Option<FuncId>,
    pub next_func_id_after: u32,
    pub next_global_local_id_after: u32,
}
