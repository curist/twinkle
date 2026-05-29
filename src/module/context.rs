use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::intrinsics::registry;
use crate::ir::core::{FuncId, LocalId};
use crate::ir::lower::prelude;
use crate::syntax::ast::ImportItem;
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
    /// Persistent method func targets — NOT snapshot/restored.
    /// Accumulates ExternalFuncRef entries for inherent methods so they
    /// remain available across module boundaries (transitive method calls).
    pub method_func_targets: HashMap<String, ExternalFuncRef>,
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
    /// Whether prelude method signatures have been registered ahead of compiling
    /// prelude bodies. This lets prelude modules call methods from later
    /// alphabetically compiled prelude modules.
    pub prelude_method_signatures_registered: bool,
}

pub fn default_func_table() -> HashMap<String, FuncId> {
    let mut func_table: HashMap<String, FuncId> = HashMap::new();
    registry::populate_func_table(&mut func_table, true);
    debug_assert!(
        !func_table
            .values()
            .any(|id| prelude::is_retired_prelude_id(*id)),
        "default func_table must not contain retired prelude IDs"
    );

    func_table
}

pub fn default_module_aliases() -> HashSet<String> {
    // Built-in module aliases: handled as module-qualified calls rather than
    // method calls on values.
    registry::builtin_module_aliases()
        .iter()
        .map(|alias| (*alias).to_string())
        .collect()
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
            method_func_targets: HashMap::new(),
            module_registry: HashMap::new(),
            next_global_local_id: 0,
            lowered_modules: Vec::new(),
            entry_module_path: None,
            module_hashes: HashMap::new(),
            prelude_method_signatures_registered: false,
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
            // Extern functions are registered under their extern namespace
            // (e.g., "console.log"), not the import alias.
            let qualified_name = if sig.extern_module.is_some() {
                // Already extern-qualified (e.g., "console.log") — use as-is
                func_name.clone()
            } else {
                format!("{}.{}", alias, func_name)
            };

            if let Some(ref ext_mod) = sig.extern_module {
                self.module_aliases.insert(ext_mod.clone());
                self.value_env.add_extern_namespace(ext_mod.clone());
            }

            let qualified_sig = FunctionSignature {
                name: qualified_name.clone(),
                type_params: sig.type_params.clone(),
                type_param_bounds: sig.type_param_bounds.clone(),
                param_names: sig.param_names.clone(),
                params: sig.params.clone(),
                ret: sig.ret.clone(),
                doc: sig.doc.clone(),
                extern_module: sig.extern_module.clone(),
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

            // Register inherent methods: only for types defined in this module.
            // Extension methods on foreign or builtin types are not allowed
            // (builtin methods are registered separately via prelude plumbing).
            // The function itself is still callable via its qualified name
            // (registered above); this only controls dot-method availability.
            if let Some(receiver_ty) = sig.params.first()
                && let Some(receiver_type_id) = method_receiver_type_id(receiver_ty)
            {
                let is_own_type = exports
                    .public_types
                    .values()
                    .any(|&tid| tid == receiver_type_id);
                if is_own_type {
                    self.type_env.add_method(
                        receiver_type_id,
                        func_name.clone(),
                        qualified_name.clone(),
                        Some(sig.clone()),
                    );
                    // Persist method func target (not snapshot/restored)
                    if let Some(&func_id) = exports.public_func_ids.get(func_name) {
                        self.method_func_targets.insert(
                            qualified_name.clone(),
                            ExternalFuncRef {
                                module_path: exports.canonical_path.clone(),
                                local_func_id: func_id,
                            },
                        );
                    }

                    // Builtin receiver methods can also be called via canonical
                    // module aliases, e.g. `Vector.map(xs, f)`.
                    if let Some(alias_name) = builtin_method_alias(receiver_type_id) {
                        let builtin_name = format!("{}.{}", alias_name, func_name);
                        let builtin_sig = FunctionSignature {
                            name: builtin_name.clone(),
                            type_params: sig.type_params.clone(),
                            type_param_bounds: sig.type_param_bounds.clone(),
                            param_names: sig.param_names.clone(),
                            params: sig.params.clone(),
                            ret: sig.ret.clone(),
                            doc: sig.doc.clone(),
                            extern_module: sig.extern_module.clone(),
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

    /// Register unqualified bindings for destructuring import items.
    ///
    /// For each `ImportItem`, binds the selected name (or alias) directly
    /// into the value/type namespace so it can be used without qualification.
    /// Returns an error listing any names not found in the module's exports.
    pub fn register_import_items(
        &mut self,
        alias: &str,
        exports: &ModuleExports,
        items: &[ImportItem],
    ) -> Result<(), Vec<(String, &'static str)>> {
        let mut missing = Vec::new();

        for item in items {
            match item {
                ImportItem::Value {
                    name,
                    alias: item_alias,
                    ..
                } => {
                    let found_func = exports.public_functions.contains_key(name);
                    let found_value = exports.public_values.contains_key(name);

                    if !found_func && !found_value {
                        missing.push((name.clone(), "value"));
                        continue;
                    }

                    let unqualified = item_alias.as_deref().unwrap_or(name);

                    // Bind function signature under unqualified name
                    if let Some(sig) = exports.public_functions.get(name) {
                        let unq_sig = FunctionSignature {
                            name: unqualified.to_string(),
                            type_params: sig.type_params.clone(),
                            type_param_bounds: sig.type_param_bounds.clone(),
                            param_names: sig.param_names.clone(),
                            params: sig.params.clone(),
                            ret: sig.ret.clone(),
                            doc: sig.doc.clone(),
                            extern_module: sig.extern_module.clone(),
                        };
                        self.value_env.add_function(unq_sig);

                        if let Some(&func_id) = exports.public_func_ids.get(name) {
                            self.func_table.insert(unqualified.to_string(), func_id);
                            self.qualified_func_targets.insert(
                                unqualified.to_string(),
                                ExternalFuncRef {
                                    module_path: exports.canonical_path.clone(),
                                    local_func_id: func_id,
                                },
                            );
                        }
                    }

                    // Bind value global under unqualified name
                    if let Some((val_ty, local_id)) = exports.public_values.get(name) {
                        self.value_env
                            .add_value(unqualified.to_string(), val_ty.clone());
                        self.qualified_value_globals
                            .insert(unqualified.to_string(), *local_id);
                    }
                }
                ImportItem::Type {
                    name,
                    alias: item_alias,
                    ..
                } => {
                    if !exports.public_types.contains_key(name) {
                        missing.push((name.clone(), "type"));
                        continue;
                    }

                    let unqualified = item_alias.as_deref().unwrap_or(name);
                    let &type_id = exports.public_types.get(name).unwrap();
                    self.type_env
                        .register_type_alias(unqualified.to_string(), type_id);

                    // Register inherent methods for the imported type.
                    // The signature is stored directly in TypeEnv so method
                    // resolution bypasses ValueEnv entirely (Option B).
                    for (func_name, sig) in &exports.public_functions {
                        if let Some(receiver_ty) = sig.params.first()
                            && let Some(receiver_type_id) = method_receiver_type_id(receiver_ty)
                            && receiver_type_id == type_id
                        {
                            // Use the module's qualified name for
                            // FuncId resolution in the lowerer.
                            let qualified_name = format!("{}.{}", alias, func_name);

                            // Register FuncId mapping so the lowerer
                            // can resolve the qualified name.
                            if let Some(&func_id) = exports.public_func_ids.get(func_name) {
                                self.func_table.insert(qualified_name.clone(), func_id);
                                let ext_ref = ExternalFuncRef {
                                    module_path: exports.canonical_path.clone(),
                                    local_func_id: func_id,
                                };
                                self.qualified_func_targets
                                    .insert(qualified_name.clone(), ext_ref.clone());
                                // Persist for transitive method resolution
                                self.method_func_targets
                                    .insert(qualified_name.clone(), ext_ref);
                            }

                            self.type_env.add_method(
                                type_id,
                                func_name.clone(),
                                qualified_name,
                                Some(sig.clone()),
                            );
                        }
                    }
                }
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
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
                type_param_bounds: sig.type_param_bounds.clone(),
                param_names: sig.param_names.clone(),
                params: sig.params.clone(),
                ret: sig.ret.clone(),
                doc: sig.doc.clone(),
                extern_module: sig.extern_module.clone(),
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

            self.type_env.add_method(
                receiver_type_id,
                func_name.clone(),
                builtin_name,
                Some(sig.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_func_table_excludes_retired_prelude_policy_entries() {
        let func_table = default_func_table();

        for retired in prelude::RETIRED_PRELUDE_IDS {
            assert!(
                !func_table
                    .values()
                    .any(|func_id| *func_id == retired.func_id),
                "retired prelude FuncId({}) leaked into default table",
                retired.func_id.0
            );
            assert!(
                !func_table.contains_key(retired.former_twinkle_name),
                "retired prelude name '{}' leaked into default table",
                retired.former_twinkle_name
            );
            if let Some(replacement) = retired.replacement {
                assert!(
                    func_table.values().any(|func_id| *func_id == replacement),
                    "replacement FuncId({}) missing from default table",
                    replacement.0
                );
            }
        }
    }
}
