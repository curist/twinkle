use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ir::core::{FuncId, FunctionDef};
use crate::ir::lower::prelude;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{FunctionSignature, MonoType, TypeId};

/// Exported symbols from a compiled module
#[derive(Debug, Clone)]
pub struct ModuleExports {
    /// exported type name → global TypeId
    pub public_types: HashMap<String, TypeId>,
    /// exported function name → signature
    pub public_functions: HashMap<String, FunctionSignature>,
    /// exported function name → assigned FuncId
    pub public_func_ids: HashMap<String, FuncId>,
}

impl ModuleExports {
    pub fn empty() -> Self {
        Self {
            public_types: HashMap::new(),
            public_functions: HashMap::new(),
            public_func_ids: HashMap::new(),
        }
    }
}

/// Shared compilation context accumulated across all modules compiled in one
/// `twk` invocation.  TypeIds and FuncIds are globally unique within it.
pub struct CompilationContext {
    /// Shared type environment — grows as modules are compiled
    pub type_env: TypeEnv,
    /// Shared value environment — grows as modules are compiled
    pub value_env: ValueEnv,
    /// Qualified "module.fn" and plain "fn" → FuncId
    pub func_table: HashMap<String, FuncId>,
    /// alias → exports for each compiled import
    pub module_registry: HashMap<String, ModuleExports>,
    /// Set of module alias names (for the lowerer and type checker)
    pub module_aliases: HashSet<String>,
    /// path → exports cache (for deduplication)
    pub module_cache: HashMap<PathBuf, ModuleExports>,
    /// Accumulated lowered functions from all compiled modules
    pub all_functions: Vec<FunctionDef>,
    /// Next FuncId to assign (starts after prelude)
    pub next_func_id: u32,
}

impl CompilationContext {
    pub fn new() -> Self {
        let mut func_table: HashMap<String, FuncId> = HashMap::new();

        // Register prelude functions in the table (same as Lowerer::new does)
        func_table.insert("print".to_string(), prelude::PRINT);
        func_table.insert("println".to_string(), prelude::PRINTLN);
        func_table.insert("error".to_string(), prelude::ERROR);
        func_table.insert("int_to_string".to_string(), prelude::INT_TO_STRING);
        func_table.insert("float_to_string".to_string(), prelude::FLOAT_TO_STRING);
        func_table.insert("bool_to_string".to_string(), prelude::BOOL_TO_STRING);
        func_table.insert("string_to_string".to_string(), prelude::STRING_TO_STRING);
        func_table.insert("string_len".to_string(), prelude::STRING_LEN);
        func_table.insert("string_concat".to_string(), prelude::STRING_CONCAT);
        func_table.insert("array_len".to_string(), prelude::ARRAY_LEN);
        func_table.insert("array_append".to_string(), prelude::ARRAY_APPEND);
        func_table.insert("array_set".to_string(), prelude::ARRAY_SET);
        func_table.insert("dict_set".to_string(), prelude::DICT_SET);
        func_table.insert("dict_keys".to_string(), prelude::DICT_KEYS);

        Self {
            type_env: TypeEnv::new(),
            value_env: ValueEnv::new(),
            func_table,
            module_registry: HashMap::new(),
            module_aliases: HashSet::new(),
            module_cache: HashMap::new(),
            all_functions: Vec::new(),
            next_func_id: prelude::USER_FUNC_START,
        }
    }

    /// Allocate a new globally-unique FuncId
    pub fn alloc_func_id(&mut self) -> FuncId {
        let id = FuncId(self.next_func_id);
        self.next_func_id += 1;
        id
    }

    /// Register all exports from a compiled module under its alias.
    /// Adds qualified type/function names to shared TypeEnv/ValueEnv and the
    /// method table.
    pub fn register_module_exports(&mut self, alias: &str, exports: &ModuleExports) {
        self.module_aliases.insert(alias.to_string());

        // Register qualified type names: "alias.TypeName" → TypeId
        for (type_name, &type_id) in &exports.public_types {
            let qualified = format!("{}.{}", alias, type_name);
            self.type_env.register_type_alias(qualified, type_id);
        }

        // Register qualified function signatures and FuncIds
        for (func_name, sig) in &exports.public_functions {
            let qualified_name = format!("{}.{}", alias, func_name);
            let qualified_sig = FunctionSignature {
                name: qualified_name.clone(),
                params: sig.params.clone(),
                ret: sig.ret.clone(),
            };
            self.value_env.add_function(qualified_sig);

            // Register qualified FuncId in func_table
            if let Some(&func_id) = exports.public_func_ids.get(func_name) {
                self.func_table.insert(qualified_name.clone(), func_id);
            }

            // Register inherent methods: if first param is a Named type,
            // register (type_id, method_name) → qualified_func_name
            if let Some(MonoType::Named { type_id, .. }) = sig.params.first() {
                self.type_env.add_method(*type_id, func_name.clone(), qualified_name);
            }
        }

        self.module_registry.insert(alias.to_string(), exports.clone());
    }
}

impl Default for CompilationContext {
    fn default() -> Self {
        Self::new()
    }
}
