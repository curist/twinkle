use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ir::core::{FuncId, LocalId};
use crate::ir::lower::prelude;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{
    FunctionSignature, MonoType, TypeId, builtin_method_alias, method_receiver_type_id,
};

use super::artifacts::{ExternalFuncRef, LoweredModule};

/// Exported symbols from a compiled module
#[derive(Debug, Clone)]
pub struct ModuleExports {
    pub canonical_path: PathBuf,
    /// exported type name → global TypeId
    pub public_types: HashMap<String, TypeId>,
    /// exported function name → signature
    pub public_functions: HashMap<String, FunctionSignature>,
    /// exported function name → module-local FuncId
    pub public_func_ids: HashMap<String, FuncId>,
    /// exported value name → (type, globally-unique LocalId in the module's __init__)
    pub public_values: HashMap<String, (MonoType, LocalId)>,
}

impl ModuleExports {
    pub fn empty() -> Self {
        Self {
            canonical_path: PathBuf::new(),
            public_types: HashMap::new(),
            public_functions: HashMap::new(),
            public_func_ids: HashMap::new(),
            public_values: HashMap::new(),
        }
    }
}

/// Module-loader infrastructure: deduplication cache only.
/// Passed through the recursive compile_module calls but never carries
/// accumulated compilation state.
pub struct CompilationContext {
    /// Deduplication cache: canonical path → exports (prevents re-compiling same file)
    pub module_cache: HashMap<PathBuf, ModuleExports>,
}

impl CompilationContext {
    pub fn new() -> Self {
        Self {
            module_cache: HashMap::new(),
        }
    }
}

impl Default for CompilationContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Accumulated compilation state across all modules compiled in one `twk` invocation.
pub struct CompileState {
    // Environments (grow as modules are compiled)
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
    /// Qualified "module.fn" and plain "fn" → FuncId
    pub func_table: HashMap<String, FuncId>,
    /// Set of module alias names (for the lowerer and type checker)
    pub module_aliases: HashSet<String>,
    /// "alias.name" → globally-unique LocalId, for cross-module value references
    pub qualified_value_globals: HashMap<String, LocalId>,
    /// "alias.fn" → target module path + module-local FuncId for linker remap.
    pub qualified_func_targets: HashMap<String, ExternalFuncRef>,
    /// alias → exports for each compiled import
    pub module_registry: HashMap<String, ModuleExports>,

    // Allocation counters
    pub next_global_local_id: u32,

    // Accumulated lowered modules (for link step)
    pub lowered_modules: Vec<LoweredModule>,
    /// Canonical path of the top-level entry module being compiled.
    pub entry_module_path: Option<PathBuf>,
    /// Content/dependency hash per module for query-stage cache keys.
    pub module_hashes: HashMap<PathBuf, u64>,
}

pub fn default_func_table() -> HashMap<String, FuncId> {
    let mut func_table: HashMap<String, FuncId> = HashMap::new();

    // Register prelude functions in the table (same as Lowerer::new does)
    func_table.insert("print".to_string(), prelude::PRINT);
    func_table.insert("println".to_string(), prelude::PRINTLN);
    func_table.insert("error".to_string(), prelude::ERROR);
    func_table.insert("eprint".to_string(), prelude::EPRINT);
    func_table.insert("eprintln".to_string(), prelude::EPRINTLN);
    func_table.insert("string_len".to_string(), prelude::STRING_LEN);
    func_table.insert("string_concat".to_string(), prelude::STRING_CONCAT);
    func_table.insert("vector_len".to_string(), prelude::VECTOR_LEN);
    func_table.insert("vector_push".to_string(), prelude::VECTOR_PUSH);
    func_table.insert("vector_set_unsafe".to_string(), prelude::VECTOR_SET_UNSAFE);
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
    func_table.insert("Iterator.next".to_string(), prelude::ITERATOR_NEXT);
    func_table.insert("Iterator.unfold".to_string(), prelude::ITERATOR_UNFOLD);
    func_table.insert("Vector.len".to_string(), prelude::VECTOR_LEN);
    func_table.insert("Vector.concat".to_string(), prelude::VECTOR_CONCAT);
    func_table.insert("Vector.slice".to_string(), prelude::VECTOR_SLICE);
    func_table.insert("Vector.make".to_string(), prelude::VECTOR_MAKE);
    func_table.insert("String.len".to_string(), prelude::STRING_LEN);
    func_table.insert("String.concat".to_string(), prelude::STRING_CONCAT);
    func_table.insert("String.substring".to_string(), prelude::STRING_SUBSTR);
    func_table.insert("String.get".to_string(), prelude::STRING_GET);
    func_table.insert("Int.to_string".to_string(), prelude::INT_TO_STRING);
    func_table.insert("Float.to_string".to_string(), prelude::FLOAT_TO_STRING);
    func_table.insert("Bool.to_string".to_string(), prelude::BOOL_TO_STRING);
    func_table.insert("String.to_string".to_string(), prelude::STRING_TO_STRING);
    func_table.insert("Dict.len".to_string(), prelude::DICT_LEN);
    func_table.insert("Dict.has".to_string(), prelude::DICT_HAS);
    func_table.insert("Dict.keys".to_string(), prelude::DICT_KEYS);
    func_table.insert("Dict.remove".to_string(), prelude::DICT_REMOVE);
    func_table.insert("String.char_code_at".to_string(), prelude::CHAR_CODE_AT);
    func_table.insert("String.from_char_code".to_string(), prelude::FROM_CHAR_CODE);
    func_table.insert("Int.from_string".to_string(), prelude::INT_FROM_STRING);
    func_table.insert("Float.from_string".to_string(), prelude::FLOAT_FROM_STRING);
    func_table.insert("__host_read_file".to_string(), prelude::HOST_READ_FILE);
    func_table.insert("__host_write_file".to_string(), prelude::HOST_WRITE_FILE);
    func_table.insert("__host_write_bytes".to_string(), prelude::HOST_WRITE_BYTES);
    func_table.insert("__host_mkdirp".to_string(), prelude::HOST_MKDIRP);
    func_table.insert("__host_list_dir".to_string(), prelude::HOST_LIST_DIR);
    func_table.insert("__host_exists".to_string(), prelude::HOST_EXISTS);
    func_table.insert("__host_args".to_string(), prelude::HOST_ARGS);
    func_table.insert("__host_env".to_string(), prelude::HOST_ENV);
    func_table.insert("__host_cwd".to_string(), prelude::HOST_CWD);
    func_table.insert("__host_exit".to_string(), prelude::HOST_EXIT);

    func_table
}

pub fn default_module_aliases() -> HashSet<String> {
    // Built-in module aliases: handled as module-qualified calls rather than
    // method calls on values.
    let mut module_aliases = HashSet::new();
    module_aliases.insert("Cell".to_string());
    module_aliases.insert("Dict".to_string());
    module_aliases.insert("Iterator".to_string());
    module_aliases.insert("Vector".to_string());
    module_aliases.insert("String".to_string());
    module_aliases.insert("Int".to_string());
    module_aliases.insert("Float".to_string());
    module_aliases.insert("Bool".to_string());
    module_aliases
}

impl CompileState {
    pub fn initial() -> Self {
        Self {
            type_env: TypeEnv::new(),
            value_env: ValueEnv::new(),
            func_table: default_func_table(),
            module_aliases: default_module_aliases(),
            qualified_value_globals: HashMap::new(),
            qualified_func_targets: HashMap::new(),
            module_registry: HashMap::new(),
            next_global_local_id: 0,
            lowered_modules: Vec::new(),
            entry_module_path: None,
            module_hashes: HashMap::new(),
        }
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
                self.qualified_func_targets.insert(
                    qualified_name.clone(),
                    ExternalFuncRef {
                        module_path: exports.canonical_path.clone(),
                        local_func_id: func_id,
                    },
                );
            }

            // Register inherent methods: functions whose first parameter is a
            // receiver type (named or builtin synthetic receiver) become methods.
            if let Some(receiver_ty) = sig.params.first() {
                if let Some(receiver_type_id) = method_receiver_type_id(receiver_ty) {
                    self.type_env.add_method(
                        receiver_type_id,
                        func_name.clone(),
                        qualified_name.clone(),
                    );

                    // Builtin receiver methods can also be called via canonical
                    // module aliases, e.g. `Vector.map(xs, f)`.
                    if let Some(alias_name) = builtin_method_alias(receiver_type_id) {
                        let builtin_name = format!("{}.{}", alias_name, func_name);
                        let builtin_sig = FunctionSignature {
                            name: builtin_name.clone(),
                            type_params: sig.type_params.clone(),
                            params: sig.params.clone(),
                            ret: sig.ret.clone(),
                        };
                        self.value_env.add_function(builtin_sig);
                        if let Some(&func_id) = exports.public_func_ids.get(func_name) {
                            self.func_table.insert(builtin_name.clone(), func_id);
                            self.qualified_func_targets.insert(
                                builtin_name,
                                ExternalFuncRef {
                                    module_path: exports.canonical_path.clone(),
                                    local_func_id: func_id,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Register qualified pub value names and their global LocalIds
        for (val_name, (val_ty, local_id)) in &exports.public_values {
            let qualified = format!("{}.{}", alias, val_name);
            self.value_env.add_value(qualified.clone(), val_ty.clone());
            self.qualified_value_globals.insert(qualified, *local_id);
        }

        self.module_registry
            .insert(alias.to_string(), exports.clone());
    }

    /// Register prelude exports without exposing internal prelude aliases.
    ///
    /// Prelude modules provide:
    /// - inherent methods for dot syntax (`xs.map(...)`)
    /// - canonical builtin-qualified calls (`Vector.map(...)`)
    ///
    /// They intentionally do not add user-visible module aliases like
    /// `__prelude_vector`.
    pub fn register_prelude_exports(&mut self, exports: &ModuleExports) {
        for (func_name, sig) in &exports.public_functions {
            let Some(receiver_ty) = sig.params.first() else {
                continue;
            };
            let Some(receiver_type_id) = method_receiver_type_id(receiver_ty) else {
                continue;
            };
            let Some(alias_name) = builtin_method_alias(receiver_type_id) else {
                continue;
            };

            let builtin_name = format!("{}.{}", alias_name, func_name);
            let builtin_sig = FunctionSignature {
                name: builtin_name.clone(),
                type_params: sig.type_params.clone(),
                params: sig.params.clone(),
                ret: sig.ret.clone(),
            };
            self.value_env.add_function(builtin_sig);

            if let Some(&func_id) = exports.public_func_ids.get(func_name) {
                self.func_table.insert(builtin_name.clone(), func_id);
                self.qualified_func_targets.insert(
                    builtin_name.clone(),
                    ExternalFuncRef {
                        module_path: exports.canonical_path.clone(),
                        local_func_id: func_id,
                    },
                );
            }

            self.type_env
                .add_method(receiver_type_id, func_name.clone(), builtin_name);
        }
    }
}
