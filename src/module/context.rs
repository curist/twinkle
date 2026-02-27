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
    /// FuncId of the __init__ function (top-level statements), if any
    pub init_func_id: Option<FuncId>,
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
        func_table.insert("range_from".to_string(), prelude::RANGE_FROM);
        func_table.insert("range".to_string(), prelude::RANGE);
        func_table.insert("range_step".to_string(), prelude::RANGE_STEP);
        func_table.insert("Cell.new".to_string(), prelude::CELL_NEW);
        func_table.insert("Cell.get".to_string(), prelude::CELL_GET);
        func_table.insert("Cell.set".to_string(), prelude::CELL_SET);
        func_table.insert("Cell.update".to_string(), prelude::CELL_UPDATE);
        func_table.insert("Dict.new".to_string(), prelude::DICT_NEW);
        func_table.insert("Iterator.next".to_string(),   prelude::ITERATOR_NEXT);
        func_table.insert("Iterator.unfold".to_string(), prelude::ITERATOR_UNFOLD);
        func_table.insert("Array.len".to_string(),        prelude::ARRAY_LEN);
        func_table.insert("Array.append".to_string(),     prelude::ARRAY_APPEND);
        func_table.insert("Array.concat".to_string(),     prelude::ARRAY_CONCAT);
        func_table.insert("Array.slice".to_string(),      prelude::ARRAY_SLICE);
        func_table.insert("String.len".to_string(),       prelude::STRING_LEN);
        func_table.insert("String.concat".to_string(),    prelude::STRING_CONCAT);
        func_table.insert("String.substring".to_string(), prelude::STRING_SUBSTR);
        func_table.insert("String.to_string".to_string(), prelude::STRING_TO_STRING);
        func_table.insert("Dict.len".to_string(),         prelude::DICT_LEN);
        func_table.insert("Dict.has".to_string(),         prelude::DICT_HAS);
        func_table.insert("Dict.keys".to_string(),        prelude::DICT_KEYS);
        func_table.insert("Dict.remove".to_string(),      prelude::DICT_REMOVE);

        // Built-in module aliases: handled as module-qualified calls rather than
        // method calls on values.
        let mut module_aliases = HashSet::new();
        module_aliases.insert("Cell".to_string());
        module_aliases.insert("Dict".to_string());
        module_aliases.insert("Iterator".to_string());
        module_aliases.insert("Array".to_string());
        module_aliases.insert("String".to_string());

        Self {
            type_env: TypeEnv::new(),
            value_env: ValueEnv::new(),
            func_table,
            module_registry: HashMap::new(),
            module_aliases,
            module_cache: HashMap::new(),
            all_functions: Vec::new(),
            next_func_id: prelude::USER_FUNC_START,
            init_func_id: None,
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
                type_params: sig.type_params.clone(),
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
