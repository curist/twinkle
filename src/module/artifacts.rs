use std::collections::HashMap;
use std::path::PathBuf;

use crate::ir::core::{ExternImport, FuncId, FunctionDef};
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::type_map::TypeMap;

/// Output of `Resolver::resolve`
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
}

/// Output of `TypeChecker::check_module`
#[derive(Debug, Clone)]
pub struct TypedModule {
    pub type_map: TypeMap,
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalFuncRef {
    pub module_path: PathBuf,
    pub local_func_id: FuncId,
}

/// Output of `Lowerer::lower_module_funcs`
#[derive(Debug, Clone)]
pub struct LoweredModule {
    pub module_path: PathBuf,
    pub dependencies: Vec<PathBuf>,
    pub functions: Vec<FunctionDef>,
    pub init_func_id: Option<FuncId>,
    pub external_func_refs: HashMap<FuncId, ExternalFuncRef>,
    pub extern_imports: HashMap<FuncId, ExternImport>,
    pub next_func_id_after: u32,
    pub next_global_local_id_after: u32,
}
