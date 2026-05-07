use std::collections::{HashMap, HashSet};

use crate::ir::core::{FuncId, LocalId};
use crate::types::env::{TypeEnvBindingSnapshot, ValueEnvBindingSnapshot};

use crate::syntax::ast::ImportItem;

use super::{CompileState, ExternalFuncRef, ModuleExports};

#[derive(Clone)]
pub(super) struct CompileEnvSnapshot {
    pub type_env: TypeEnvBindingSnapshot,
    pub value_env: ValueEnvBindingSnapshot,
    pub func_table: HashMap<String, FuncId>,
    pub module_aliases: HashSet<String>,
    pub qualified_value_globals: HashMap<String, LocalId>,
    pub qualified_func_targets: HashMap<String, ExternalFuncRef>,
    pub module_registry: HashMap<String, ModuleExports>,
}

pub(super) enum DependencyProjection<'a> {
    Import {
        alias: &'a str,
        items: Option<&'a [ImportItem]>,
    },
    Prelude,
}

pub(super) fn snapshot_compile_env(state: &CompileState) -> CompileEnvSnapshot {
    CompileEnvSnapshot {
        type_env: state.type_env.snapshot_bindings(),
        value_env: state.value_env.snapshot_bindings(),
        func_table: state.func_table.clone(),
        module_aliases: state.module_aliases.clone(),
        qualified_value_globals: state.qualified_value_globals.clone(),
        qualified_func_targets: state.qualified_func_targets.clone(),
        module_registry: state.module_registry.clone(),
    }
}

pub(super) fn restore_compile_env(state: &mut CompileState, snapshot: CompileEnvSnapshot) {
    state.type_env.restore_bindings(snapshot.type_env);
    state.value_env.restore_bindings(snapshot.value_env);
    state.func_table = snapshot.func_table;
    state.module_aliases = snapshot.module_aliases;
    state.qualified_value_globals = snapshot.qualified_value_globals;
    state.qualified_func_targets = snapshot.qualified_func_targets;
    state.module_registry = snapshot.module_registry;
}

pub(super) fn project_dependency_exports(
    state: &mut CompileState,
    projection: DependencyProjection<'_>,
    exports: &ModuleExports,
) -> anyhow::Result<()> {
    match projection {
        DependencyProjection::Import { alias, items } => {
            if let Some(items) = items {
                // Destructured import: only bring the listed names into scope,
                // NOT the parent module name.
                if let Err(missing) = state.register_import_items(alias, exports, items) {
                    let details: Vec<String> = missing
                        .iter()
                        .map(|(name, kind)| format!("{} `{}`", kind, name))
                        .collect();
                    return Err(anyhow::anyhow!(
                        "Module '{}' does not export: {}",
                        alias,
                        details.join(", ")
                    ));
                }
            } else {
                // Plain import: bring the module name into scope.
                state.register_module_exports(alias, exports);
            }
        }
        DependencyProjection::Prelude => state.register_prelude_exports(exports),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::types::ty::{FunctionSignature, MonoType};

    use super::*;

    fn sample_exports_with_publics() -> ModuleExports {
        let mut exports = ModuleExports::empty();
        exports.canonical_path = PathBuf::from("/virtual/env_integration/dep.tw");
        exports.public_functions.insert(
            "double".to_string(),
            FunctionSignature {
                name: "double".to_string(),
                type_params: vec![],
                type_param_bounds: HashMap::new(),
                param_names: vec!["x".to_string()],
                params: vec![MonoType::Int],
                ret: Some(MonoType::Int),
                doc: None,
                extern_module: None,
            },
        );
        exports
            .public_func_ids
            .insert("double".to_string(), FuncId(10_001));
        exports
            .public_values
            .insert("answer".to_string(), (MonoType::Int, LocalId(7)));
        exports
    }

    #[test]
    fn snapshot_restore_reverts_env_projection_mutations() {
        let mut state = CompileState::initial();
        let snapshot = snapshot_compile_env(&state);

        let exports = sample_exports_with_publics();
        project_dependency_exports(
            &mut state,
            DependencyProjection::Import {
                alias: "dep",
                items: None,
            },
            &exports,
        )
        .unwrap();

        assert!(state.module_aliases.contains("dep"));
        assert!(state.module_registry.contains_key("dep"));
        assert!(state.value_env.lookup("dep.answer").is_some());
        assert!(state.func_table.contains_key("dep.double"));

        restore_compile_env(&mut state, snapshot);

        assert!(!state.module_aliases.contains("dep"));
        assert!(!state.module_registry.contains_key("dep"));
        assert!(state.value_env.lookup("dep.answer").is_none());
        assert!(!state.func_table.contains_key("dep.double"));
    }

    #[test]
    fn dependency_projection_distinguishes_import_and_prelude_visibility() {
        let mut state = CompileState::initial();

        let import_exports = sample_exports_with_publics();
        project_dependency_exports(
            &mut state,
            DependencyProjection::Import {
                alias: "math",
                items: None,
            },
            &import_exports,
        )
        .unwrap();

        let mut prelude_exports = ModuleExports::empty();
        prelude_exports.canonical_path =
            PathBuf::from("/virtual/env_integration/prelude/vector.tw");
        prelude_exports.public_functions.insert(
            "map".to_string(),
            FunctionSignature {
                name: "map".to_string(),
                type_params: vec!["A".to_string(), "B".to_string()],
                type_param_bounds: HashMap::new(),
                param_names: vec!["xs".to_string(), "f".to_string()],
                params: vec![
                    MonoType::Vector(Box::new(MonoType::Int)),
                    MonoType::Function {
                        params: vec![MonoType::Int],
                        ret: Box::new(MonoType::Int),
                    },
                ],
                ret: Some(MonoType::Vector(Box::new(MonoType::Int))),
                doc: None,
                extern_module: None,
            },
        );
        prelude_exports
            .public_func_ids
            .insert("map".to_string(), FuncId(10_002));

        project_dependency_exports(&mut state, DependencyProjection::Prelude, &prelude_exports)
            .unwrap();

        assert!(state.module_aliases.contains("math"));
        assert!(state.module_registry.contains_key("math"));

        assert!(!state.module_aliases.contains("__prelude_vector"));
        assert!(!state.module_registry.contains_key("__prelude_vector"));

        assert!(state.value_env.get_function("Vector.map").is_some());
        assert!(state.func_table.contains_key("Vector.map"));
    }
}
