use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::syntax::ast::{Item, SourceFile};

use super::ModuleSourceAdapter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PlannedDependencyKind {
    Import,
    Prelude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedDependency {
    pub canonical_path: PathBuf,
    pub alias: String,
    pub kind: PlannedDependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleDependencyPlan {
    pub dependencies: Vec<PlannedDependency>,
    pub canonical_paths: Vec<PathBuf>,
    pub is_internal: bool,
}

pub(super) fn plan_module_dependencies<A: ModuleSourceAdapter>(
    file_path: &Path,
    canonical: &Path,
    ast: &SourceFile,
    adapter: &A,
) -> Result<ModuleDependencyPlan> {
    let mut dependencies = Vec::new();
    let mut canonical_paths = Vec::new();

    for item in &ast.items {
        if let Item::Import(import) = item {
            let dep_path = adapter.resolve_import_path(file_path, import);
            let dep_canonical = adapter.canonicalize(&dep_path);
            if !adapter.exists(&dep_canonical) {
                return Err(anyhow!(
                    "Cannot resolve module '{}': expected file '{}'",
                    if import.is_stdlib {
                        format!("@{}", import.module_path.join("."))
                    } else {
                        import.module_path.join(".")
                    },
                    dep_path.display()
                ));
            }

            canonical_paths.push(dep_canonical.clone());
            dependencies.push(PlannedDependency {
                canonical_path: dep_canonical,
                alias: import.module_name().to_string(),
                kind: PlannedDependencyKind::Import,
            });
        }
    }

    let stdlib_root_canonical = adapter.canonicalize(&adapter.stdlib_root());
    let prelude_root_canonical = adapter.canonicalize(&adapter.prelude_root());
    let is_internal = canonical.starts_with(&stdlib_root_canonical)
        || canonical.starts_with(&prelude_root_canonical);

    if !is_internal {
        for prelude_path in &adapter.list_prelude_modules() {
            let prelude_canonical = adapter.canonicalize(prelude_path);
            if canonical_paths.contains(&prelude_canonical) {
                continue;
            }
            if !adapter.exists(&prelude_canonical) {
                continue;
            }

            canonical_paths.push(prelude_canonical.clone());
            dependencies.push(PlannedDependency {
                canonical_path: prelude_canonical,
                alias: format!(
                    "__prelude_{}",
                    prelude_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                ),
                kind: PlannedDependencyKind::Prelude,
            });
        }
    }

    Ok(ModuleDependencyPlan {
        dependencies,
        canonical_paths,
        is_internal,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::query::api::parse_source_module;
    use crate::syntax::ast::ImportDecl;

    use super::*;
    use crate::module::{resolve_module_path, resolve_stdlib_module_path_from_root};

    struct TestAdapter {
        project_root: PathBuf,
        stdlib_root: PathBuf,
        prelude_root: PathBuf,
        existing: HashSet<PathBuf>,
        prelude_modules: Vec<PathBuf>,
    }

    impl TestAdapter {
        fn new(project_root: PathBuf, stdlib_root: PathBuf, prelude_root: PathBuf) -> Self {
            Self {
                project_root,
                stdlib_root,
                prelude_root,
                existing: HashSet::new(),
                prelude_modules: Vec::new(),
            }
        }

        fn with_existing(mut self, path: PathBuf) -> Self {
            self.existing.insert(path);
            self
        }

        fn with_prelude_modules(mut self, modules: Vec<PathBuf>) -> Self {
            self.prelude_modules = modules;
            self
        }

        fn resolve_non_std_import(&self, import: &ImportDecl) -> PathBuf {
            resolve_module_path(&self.project_root, &import.module_path)
        }

        fn resolve_std_import(&self, import: &ImportDecl) -> PathBuf {
            resolve_stdlib_module_path_from_root(&self.stdlib_root, &import.module_path)
        }
    }

    impl ModuleSourceAdapter for TestAdapter {
        fn canonicalize(&self, path: &Path) -> PathBuf {
            path.to_path_buf()
        }

        fn read_source(&self, _path: &Path) -> Result<String> {
            panic!("read_source should not be called in planner tests")
        }

        fn exists(&self, path: &Path) -> bool {
            self.existing.contains(path)
        }

        fn resolve_import_path(&self, _importing_file: &Path, import: &ImportDecl) -> PathBuf {
            if import.is_stdlib {
                self.resolve_std_import(import)
            } else {
                self.resolve_non_std_import(import)
            }
        }

        fn list_prelude_modules(&self) -> Vec<PathBuf> {
            self.prelude_modules.clone()
        }

        fn stdlib_root(&self) -> PathBuf {
            self.stdlib_root.clone()
        }

        fn prelude_root(&self) -> PathBuf {
            self.prelude_root.clone()
        }
    }

    #[test]
    fn planner_orders_source_imports_then_prelude_modules() {
        let project_root = PathBuf::from("/virtual/planner_order");
        let stdlib_root = project_root.join("stdlib");
        let prelude_root = project_root.join("prelude");
        let main = project_root.join("main.tw");
        let dep_b = project_root.join("dep_b.tw");
        let dep_a = project_root.join("dep_a.tw");
        let prelude_alpha = prelude_root.join("alpha.tw");
        let prelude_beta = prelude_root.join("beta.tw");

        let adapter = TestAdapter::new(project_root.clone(), stdlib_root, prelude_root)
            .with_existing(dep_a.clone())
            .with_existing(dep_b.clone())
            .with_existing(prelude_alpha.clone())
            .with_existing(prelude_beta.clone())
            .with_prelude_modules(vec![prelude_alpha.clone(), prelude_beta.clone()]);

        let source = r#"
use dep_b
use dep_a

println("ok")
"#;
        let parsed = parse_source_module(source, &main).expect("parse should succeed");

        let plan = plan_module_dependencies(&main, &main, &parsed.ast, &adapter)
            .expect("planning should succeed");

        assert!(!plan.is_internal);
        assert_eq!(
            plan.canonical_paths,
            vec![
                dep_b.clone(),
                dep_a.clone(),
                prelude_alpha.clone(),
                prelude_beta
            ]
        );
        assert_eq!(
            plan.dependencies,
            vec![
                PlannedDependency {
                    canonical_path: dep_b,
                    alias: "dep_b".to_string(),
                    kind: PlannedDependencyKind::Import,
                },
                PlannedDependency {
                    canonical_path: dep_a,
                    alias: "dep_a".to_string(),
                    kind: PlannedDependencyKind::Import,
                },
                PlannedDependency {
                    canonical_path: prelude_alpha,
                    alias: "__prelude_alpha".to_string(),
                    kind: PlannedDependencyKind::Prelude,
                },
                PlannedDependency {
                    canonical_path: PathBuf::from("/virtual/planner_order/prelude/beta.tw"),
                    alias: "__prelude_beta".to_string(),
                    kind: PlannedDependencyKind::Prelude,
                },
            ]
        );
    }

    #[test]
    fn planner_marks_stdlib_module_internal_and_skips_prelude_injection() {
        let project_root = PathBuf::from("/virtual/planner_internal");
        let stdlib_root = project_root.join("stdlib");
        let prelude_root = project_root.join("prelude");
        let stdlib_main = stdlib_root.join("main.tw");
        let dep = stdlib_root.join("dep.tw");

        let adapter = TestAdapter::new(project_root, stdlib_root, prelude_root.clone())
            .with_existing(dep.clone())
            .with_existing(prelude_root.join("alpha.tw"))
            .with_prelude_modules(vec![prelude_root.join("alpha.tw")]);

        let source = r#"
use @std.dep

println("ok")
"#;
        let parsed = parse_source_module(source, &stdlib_main).expect("parse should succeed");

        let plan = plan_module_dependencies(&stdlib_main, &stdlib_main, &parsed.ast, &adapter)
            .expect("planning should succeed");

        assert!(plan.is_internal);
        assert_eq!(
            plan.dependencies,
            vec![PlannedDependency {
                canonical_path: dep.clone(),
                alias: "dep".to_string(),
                kind: PlannedDependencyKind::Import,
            }]
        );
        assert_eq!(plan.canonical_paths, vec![dep]);
    }

    #[test]
    fn planner_dedupes_explicit_prelude_import_from_auto_prelude() {
        let project_root = PathBuf::from("/virtual/planner_dedupe");
        let stdlib_root = project_root.join("stdlib");
        let prelude_root = project_root.join("prelude");
        let main = project_root.join("main.tw");
        let prelude_alpha = prelude_root.join("alpha.tw");
        let prelude_beta = prelude_root.join("beta.tw");

        let adapter = TestAdapter::new(project_root, stdlib_root, prelude_root)
            .with_existing(prelude_alpha.clone())
            .with_existing(prelude_beta.clone())
            .with_prelude_modules(vec![prelude_alpha.clone(), prelude_beta.clone()]);

        let source = r#"
use prelude.alpha

println("ok")
"#;
        let parsed = parse_source_module(source, &main).expect("parse should succeed");

        let plan = plan_module_dependencies(&main, &main, &parsed.ast, &adapter)
            .expect("planning should succeed");

        let alpha_count = plan
            .canonical_paths
            .iter()
            .filter(|path| **path == prelude_alpha)
            .count();
        assert_eq!(alpha_count, 1);

        assert_eq!(
            plan.dependencies,
            vec![
                PlannedDependency {
                    canonical_path: prelude_alpha,
                    alias: "alpha".to_string(),
                    kind: PlannedDependencyKind::Import,
                },
                PlannedDependency {
                    canonical_path: prelude_beta,
                    alias: "__prelude_beta".to_string(),
                    kind: PlannedDependencyKind::Prelude,
                },
            ]
        );
    }
}
