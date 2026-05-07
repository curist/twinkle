pub mod artifacts;
pub mod context;
pub mod dce;
mod env_integration;
pub mod loader;
mod planner;
mod stage_runner;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::ir::CoreModule;
use crate::ir::core::{CoreExpr, CoreExprKind, FuncId, LocalId, MatchArm};
use crate::ir::lower::LowerInput;
use crate::ir::lower::prelude;
use crate::query::api::{
    QueryDiagnostic, QuerySpan, preassign_module_function_ids, resolve_stage_with_diagnostics,
    typecheck_stage_with_diagnostics_and_options,
};
use crate::query::cache::with_global_cache;
use crate::query::keys as query_keys;
use crate::syntax::ast::{ImportDecl, Item, Pattern, Stmt};
use crate::syntax::span::FileRegistry;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{FunctionSignature, TypeId, builtin_method_alias, method_receiver_type_id};

pub use artifacts::{ExternalFuncRef, LoweredModule, ResolvedModule, TypedModule};
pub use context::{CompilationContext, CompileState, ModuleExports};
use env_integration::{
    DependencyProjection, project_dependency_exports, restore_compile_env, snapshot_compile_env,
};
pub use loader::{
    find_project_root, list_prelude_modules_default, resolve_module_path,
    resolve_stdlib_module_path, resolve_stdlib_module_path_from_root,
};
use planner::{PlannedDependencyKind, plan_module_dependencies};
use stage_runner::ModuleStageRunner;

trait ModuleSourceAdapter {
    fn canonicalize(&self, path: &Path) -> PathBuf;
    fn read_source(&self, path: &Path) -> Result<String>;
    fn exists(&self, path: &Path) -> bool;
    fn resolve_import_path(&self, importing_file: &Path, import: &ImportDecl) -> PathBuf;
    /// Return prelude module paths in deterministic (sorted) order.
    fn list_prelude_modules(&self) -> Vec<PathBuf>;
    /// Return the stdlib root path (for detecting stdlib/prelude-internal modules).
    fn stdlib_root(&self) -> PathBuf;
    /// Return the prelude root path.
    fn prelude_root(&self) -> PathBuf;
}

struct FsModuleSourceAdapter;

impl ModuleSourceAdapter for FsModuleSourceAdapter {
    fn canonicalize(&self, path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn read_source(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).map_err(|e| anyhow!("Cannot read '{}': {}", path.display(), e))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn resolve_import_path(&self, importing_file: &Path, import: &ImportDecl) -> PathBuf {
        if import.is_stdlib {
            resolve_stdlib_module_path(&import.module_path)
        } else if import.is_relative {
            loader::resolve_relative_module_path(importing_file, &import.module_path)
        } else {
            let root = find_project_root(importing_file.parent().unwrap_or(Path::new(".")));
            resolve_module_path(&root, &import.module_path)
        }
    }

    fn list_prelude_modules(&self) -> Vec<PathBuf> {
        list_prelude_modules_default()
    }

    fn stdlib_root(&self) -> PathBuf {
        loader::resolve_stdlib_root_default()
    }

    fn prelude_root(&self) -> PathBuf {
        loader::resolve_prelude_root_default()
    }
}

struct SourceMapModuleAdapter {
    project_root: PathBuf,
    stdlib_root: PathBuf,
    sources: HashMap<PathBuf, String>,
}

impl SourceMapModuleAdapter {
    fn new(
        project_root: &Path,
        stdlib_root: &Path,
        sources: &HashMap<PathBuf, String>,
    ) -> SourceMapModuleAdapter {
        let project_root = normalize_path_lexical(project_root);
        let stdlib_root = normalize_path_lexical(stdlib_root);
        let sources = sources
            .iter()
            .map(|(path, source)| {
                (
                    normalize_source_map_path(path.as_path(), project_root.as_path()),
                    source.clone(),
                )
            })
            .collect();
        SourceMapModuleAdapter {
            project_root,
            stdlib_root,
            sources,
        }
    }
}

impl ModuleSourceAdapter for SourceMapModuleAdapter {
    fn canonicalize(&self, path: &Path) -> PathBuf {
        normalize_source_map_path(path, self.project_root.as_path())
    }

    fn read_source(&self, path: &Path) -> Result<String> {
        let canonical = self.canonicalize(path);
        self.sources.get(&canonical).cloned().ok_or_else(|| {
            anyhow!(
                "Cannot read '{}': source not found in module map",
                canonical.display()
            )
        })
    }

    fn exists(&self, path: &Path) -> bool {
        let canonical = self.canonicalize(path);
        self.sources.contains_key(&canonical)
    }

    fn resolve_import_path(&self, importing_file: &Path, import: &ImportDecl) -> PathBuf {
        if import.is_stdlib {
            resolve_stdlib_module_path_from_root(&self.stdlib_root, &import.module_path)
        } else if import.is_relative {
            loader::resolve_relative_module_path(importing_file, &import.module_path)
        } else {
            // Try project root first (matches twinkle.toml-based resolution)
            let from_root = resolve_module_path(&self.project_root, &import.module_path);
            if self.sources.contains_key(&from_root) {
                return from_root;
            }
            // Fall back to importing file's directory (matches FsAdapter behavior
            // when no twinkle.toml is found)
            if let Some(parent) = importing_file.parent() {
                let from_parent = resolve_module_path(parent, &import.module_path);
                if self.sources.contains_key(&from_parent) {
                    return from_parent;
                }
            }
            from_root
        }
    }

    fn list_prelude_modules(&self) -> Vec<PathBuf> {
        // List prelude modules from the source map — prelude/ is a sibling of stdlib/.
        // Match filesystem behavior: include only direct `prelude/*.tw` modules.
        let prelude_root = self.prelude_root();
        let mut paths: Vec<PathBuf> = self
            .sources
            .keys()
            .filter(|p| {
                p.parent() == Some(prelude_root.as_path())
                    && p.extension().is_some_and(|e| e == "tw")
            })
            .cloned()
            .collect();
        paths.sort();
        paths
    }

    fn stdlib_root(&self) -> PathBuf {
        self.stdlib_root.clone()
    }

    fn prelude_root(&self) -> PathBuf {
        // prelude/ is a sibling of stdlib/
        self.stdlib_root
            .parent()
            .map(|p| p.join("prelude"))
            .unwrap_or_else(|| self.stdlib_root.join("../prelude"))
    }
}

fn normalize_source_map_path(path: &Path, project_root: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_path_lexical(path)
    } else {
        normalize_path_lexical(&project_root.join(path))
    }
}

fn normalize_path_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileStage {
    Parse,
    Resolve,
    Typecheck,
    Lower,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileTraceEvent {
    pub module_path: PathBuf,
    pub stage: CompileStage,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedImport {
    pub alias: String,
    pub canonical_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AnalyzedModule {
    pub ast: crate::syntax::ast::SourceFile,
    pub file_registry: FileRegistry,
    pub typed: TypedModule,
    pub imports: Vec<AnalyzedImport>,
    pub qualified_func_targets: HashMap<String, ExternalFuncRef>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceAnalysis {
    pub entry_path: PathBuf,
    pub modules: HashMap<PathBuf, AnalyzedModule>,
    pub diagnostics: HashMap<PathBuf, Vec<QueryDiagnostic>>,
    /// File registries for modules that failed analysis (parse succeeded
    /// but resolve/typecheck failed). Used for span→position conversion
    /// when producing LSP diagnostics.
    pub file_registries: HashMap<PathBuf, FileRegistry>,
}

#[derive(Debug, Default)]
struct AnalysisCollector {
    enabled: bool,
    modules: HashMap<PathBuf, AnalyzedModule>,
    diagnostics: HashMap<PathBuf, Vec<QueryDiagnostic>>,
    file_registries: HashMap<PathBuf, FileRegistry>,
}

impl AnalysisCollector {
    fn disabled() -> Self {
        Self::default()
    }

    fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn record_module(&mut self, module_path: &Path, module: AnalyzedModule) {
        if !self.enabled {
            return;
        }
        self.modules.insert(module_path.to_path_buf(), module);
    }

    fn record_diagnostics(&mut self, module_path: &Path, diags: Vec<QueryDiagnostic>) {
        if !self.enabled || diags.is_empty() {
            return;
        }
        self.diagnostics
            .entry(module_path.to_path_buf())
            .or_default()
            .extend(diags);
    }

    fn record_parse_error(&mut self, module_path: &Path, source: &str, err: &anyhow::Error) {
        if !self.enabled {
            return;
        }

        let message = err.to_string();
        if let Some((diag, registry)) =
            parse_failure_diagnostic_from_source(module_path, source, &message)
        {
            self.record_file_registry(module_path, registry);
            self.diagnostics
                .entry(module_path.to_path_buf())
                .or_default()
                .push(diag);
            return;
        }

        self.diagnostics
            .entry(module_path.to_path_buf())
            .or_default()
            .push(QueryDiagnostic {
                code: "E_PARSE",
                message,
                span: None,
            });
    }

    fn record_file_registry(&mut self, module_path: &Path, registry: FileRegistry) {
        if !self.enabled {
            return;
        }
        self.file_registries
            .insert(module_path.to_path_buf(), registry);
    }

    fn into_workspace(self, entry_path: PathBuf) -> WorkspaceAnalysis {
        WorkspaceAnalysis {
            entry_path,
            modules: self.modules,
            diagnostics: self.diagnostics,
            file_registries: self.file_registries,
        }
    }
}

fn parse_failure_diagnostic_from_source(
    module_path: &Path,
    source: &str,
    message: &str,
) -> Option<(QueryDiagnostic, FileRegistry)> {
    let mut registry = FileRegistry::new();
    let file_id = registry.add_file(
        module_path.to_string_lossy().to_string(),
        source.to_string(),
    );

    let span = match crate::syntax::lexer::Lexer::lex(source, file_id) {
        Ok(tokens) => {
            let mut parser = crate::syntax::parser::Parser::new(tokens, file_id);
            match parser.parse_source_file() {
                Ok(_) => return None,
                Err(parse_err) => parse_err.span,
            }
        }
        Err(lex_err) => lex_err.span,
    };

    let query_span = registry.line_col(span).map(|(line, column)| QuerySpan {
        file_id: span.file_id.0,
        line,
        column,
        start: span.start,
        end: span.end,
    });

    Some((
        QueryDiagnostic {
            code: "E_PARSE",
            message: message.to_string(),
            span: query_span,
        },
        registry,
    ))
}

fn record_stage_trace(
    stage_trace: &mut Vec<CompileTraceEvent>,
    module_path: &Path,
    stage: CompileStage,
    cache_hit: bool,
) {
    stage_trace.push(CompileTraceEvent {
        module_path: module_path.to_path_buf(),
        stage,
        cache_hit,
    });
}

/// Compile a single module (file) and all its transitive dependencies.
///
/// When `do_lower` is true, lowers the module to Core IR and accumulates
/// `LoweredModule`s in `state.lowered_modules`.
///
/// Returns `(ModuleExports, FileRegistry)` for the compiled module.
pub fn compile_module(
    file_path: &Path,
    alias: &str,
    ctx: &mut CompilationContext,
    importing_stack: &mut Vec<PathBuf>,
    state: &mut CompileState,
    do_lower: bool,
) -> Result<(ModuleExports, FileRegistry)> {
    let mut stage_trace = Vec::new();
    let mut analysis_collector = AnalysisCollector::disabled();
    compile_module_with_adapter(
        file_path,
        alias,
        ctx,
        importing_stack,
        state,
        do_lower,
        &FsModuleSourceAdapter,
        &mut stage_trace,
        &mut analysis_collector,
    )
}

fn compile_module_with_adapter<A: ModuleSourceAdapter>(
    file_path: &Path,
    alias: &str,
    ctx: &mut CompilationContext,
    importing_stack: &mut Vec<PathBuf>,
    state: &mut CompileState,
    do_lower: bool,
    adapter: &A,
    stage_trace: &mut Vec<CompileTraceEvent>,
    analysis_collector: &mut AnalysisCollector,
) -> Result<(ModuleExports, FileRegistry)> {
    // Canonicalize for deduplication / cycle detection
    let canonical = adapter.canonicalize(file_path);

    // Already compiled? Return cached exports.
    if let Some(exports) = ctx.module_cache.get(&canonical) {
        return Ok((exports.clone(), FileRegistry::new()));
    }

    // Circular import?
    if importing_stack.contains(&canonical) {
        return Err(anyhow!(
            "Circular import detected: '{}' is already being compiled",
            file_path.display()
        ));
    }

    // Parse
    let source = adapter.read_source(&canonical)?;
    let source_hash = query_keys::hash_text(&source);
    let parsed_stage_runner = ModuleStageRunner::new(&canonical, source_hash, 0, 0, false);
    let parsed_result = match parsed_stage_runner.parse(&source) {
        Ok(result) => result,
        Err(err) => {
            analysis_collector.record_parse_error(&canonical, &source, &err);
            return Err(err);
        }
    };
    let parsed = parsed_result.value;
    record_stage_trace(
        stage_trace,
        &canonical,
        CompileStage::Parse,
        parsed_result.cache_hit,
    );
    let ast = parsed.ast.clone();
    let file_registry = parsed.file_registry.clone();

    // Compile dependencies from an explicit plan:
    // source-order imports, then deterministic prelude auto-imports.
    let dep_plan = plan_module_dependencies(file_path, &canonical, &ast, adapter)?;

    importing_stack.push(canonical.clone());

    compile_planned_dependencies(
        &dep_plan.dependencies,
        ctx,
        importing_stack,
        state,
        do_lower,
        adapter,
        stage_trace,
        analysis_collector,
    )?;
    let dep_canonical_paths = dep_plan.canonical_paths;
    let is_internal = dep_plan.is_internal;
    let analyzed_imports: Vec<AnalyzedImport> = dep_plan
        .dependencies
        .iter()
        .filter_map(|dep| match dep.kind {
            PlannedDependencyKind::Import => Some(AnalyzedImport {
                alias: dep.alias.clone(),
                canonical_path: dep.canonical_path.clone(),
            }),
            PlannedDependencyKind::Prelude => None,
        })
        .collect();

    // Validate intrinsic bindings only for user-facing modules, after trusted
    // modules have been projected into the compile env and before typechecking.
    if !is_internal {
        crate::intrinsics::validate::validate_intrinsic_bindings(&state.value_env)
            .map_err(|err| anyhow!("{err}"))?;
    }

    with_global_cache(|cache| cache.set_dependencies(&canonical, &dep_canonical_paths));
    let dep_hash_entries: Vec<(String, u64)> = dep_canonical_paths
        .iter()
        .map(|dep| {
            let dep_hash = state.module_hashes.get(dep).copied().unwrap_or(0);
            (dep.to_string_lossy().to_string(), dep_hash)
        })
        .collect();
    let deps_hash = query_keys::deps_hash(&dep_hash_entries);
    let context_entries: Vec<(String, u64)> = state
        .module_hashes
        .iter()
        .map(|(path, hash)| (path.to_string_lossy().to_string(), *hash))
        .collect();
    let context_hash = query_keys::context_hash(&context_entries);
    let stage_runner = ModuleStageRunner::new(
        &canonical,
        source_hash,
        deps_hash,
        context_hash,
        is_internal,
    );

    // Pre-assign module-local FuncIds for this module's user functions.
    let mut module_next_func_id = prelude::USER_FUNC_START;
    preassign_module_function_ids(&ast, alias, &mut state.func_table, &mut module_next_func_id);

    // Resolve — pure function; takes accumulated envs, returns updated envs
    let mut resolved = if analysis_collector.is_enabled() {
        // Use structured diagnostics path for analysis
        match resolve_stage_with_diagnostics(
            &ast,
            state.type_env.clone(),
            state.value_env.clone(),
            &file_registry,
            is_internal,
        ) {
            Ok(resolved) => {
                record_stage_trace(stage_trace, &canonical, CompileStage::Resolve, false);
                resolved
            }
            Err(diags) => {
                analysis_collector.record_diagnostics(&canonical, diags);
                analysis_collector.record_file_registry(&canonical, file_registry.clone());
                importing_stack.pop();
                return Err(anyhow!("resolve failed with diagnostics"));
            }
        }
    } else {
        match stage_runner.resolve(
            &ast,
            state.type_env.clone(),
            state.value_env.clone(),
            &file_registry,
        ) {
            Ok(result) => {
                record_stage_trace(
                    stage_trace,
                    &canonical,
                    CompileStage::Resolve,
                    result.cache_hit,
                );
                result.value
            }
            Err(err) => {
                importing_stack.pop();
                return Err(err);
            }
        }
    };
    // Register current module's own functions as inherent methods so that
    // p1.method() syntax works within the same file (not just cross-module).
    register_inherent_methods(
        &ast,
        alias,
        &mut resolved.type_env,
        &mut resolved.value_env,
        is_internal,
    );
    state.type_env = resolved.type_env.clone();
    state.value_env = resolved.value_env.clone();

    // Typecheck — pure function; takes explicit envs and returns updated envs + TypeMap
    let typed = if analysis_collector.is_enabled() {
        match typecheck_stage_with_diagnostics_and_options(
            &ast,
            resolved.clone(),
            state.module_aliases.clone(),
            &file_registry,
            is_internal,
        ) {
            Ok(typed) => {
                record_stage_trace(stage_trace, &canonical, CompileStage::Typecheck, false);
                typed
            }
            Err(diags) => {
                analysis_collector.record_diagnostics(&canonical, diags);
                analysis_collector.record_file_registry(&canonical, file_registry.clone());
                importing_stack.pop();
                return Err(anyhow!("typecheck failed with diagnostics"));
            }
        }
    } else {
        match stage_runner.typecheck(
            &ast,
            resolved.clone(),
            state.module_aliases.clone(),
            &file_registry,
        ) {
            Ok(result) => {
                record_stage_trace(
                    stage_trace,
                    &canonical,
                    CompileStage::Typecheck,
                    result.cache_hit,
                );
                result.value
            }
            Err(err) => {
                importing_stack.pop();
                return Err(err);
            }
        }
    };
    let TypedModule {
        type_env,
        value_env,
        type_map,
    } = typed;
    if analysis_collector.is_enabled() {
        analysis_collector.record_module(
            &canonical,
            AnalyzedModule {
                ast: ast.clone(),
                file_registry: file_registry.clone(),
                typed: TypedModule {
                    type_map: type_map.clone(),
                    type_env: type_env.clone(),
                    value_env: value_env.clone(),
                },
                imports: analyzed_imports,
                qualified_func_targets: state.qualified_func_targets.clone(),
            },
        );
    }
    state.type_env = type_env;
    state.value_env = value_env;

    let exports = build_module_exports(&ast, &canonical, alias, state);

    maybe_lower_module(
        do_lower,
        &stage_runner,
        &ast,
        alias,
        &file_registry,
        &canonical,
        &dep_canonical_paths,
        module_next_func_id,
        type_map,
        state,
        importing_stack,
        stage_trace,
    )?;

    let module_hash = query_keys::module_hash(source_hash, deps_hash);
    state.module_hashes.insert(canonical.clone(), module_hash);
    with_global_cache(|cache| cache.set_module_hash(&canonical, module_hash));

    cleanup_module_local_bindings(&ast, alias, state);

    importing_stack.pop();

    // Cache and return
    ctx.module_cache.insert(canonical, exports.clone());
    Ok((exports, file_registry))
}

fn compile_planned_dependencies<A: ModuleSourceAdapter>(
    dependencies: &[planner::PlannedDependency],
    ctx: &mut CompilationContext,
    importing_stack: &mut Vec<PathBuf>,
    state: &mut CompileState,
    do_lower: bool,
    adapter: &A,
    stage_trace: &mut Vec<CompileTraceEvent>,
    analysis_collector: &mut AnalysisCollector,
) -> Result<()> {
    let compile_snapshot = snapshot_compile_env(state);
    let mut projected_snapshot = compile_snapshot.clone();

    for dep in dependencies {
        // Two-phase snapshot/restore:
        //
        // compile_snapshot is the clean environment from before any dependency
        // projections. Each dependency compiles against that same isolated base,
        // so dependency N cannot observe projections from dependencies 1..N-1.
        //
        // projected_snapshot is the accumulated environment after projecting the
        // previously compiled dependencies. Once recursive compilation returns, we
        // restore that accumulated state and then project the current dependency on
        // top of it.
        //
        // State outside the snapshot (for example global counters and hashes)
        // continues to accumulate across both phases.
        restore_compile_env(state, compile_snapshot.clone());
        let result = compile_module_with_adapter(
            &dep.canonical_path,
            &dep.alias,
            ctx,
            importing_stack,
            state,
            do_lower,
            adapter,
            stage_trace,
            analysis_collector,
        );
        restore_compile_env(state, projected_snapshot.clone());
        match result {
            Ok((dep_exports, _)) => {
                let projection = match dep.kind {
                    PlannedDependencyKind::Import => DependencyProjection::Import {
                        alias: dep.alias.as_str(),
                        items: dep.items.as_deref(),
                    },
                    PlannedDependencyKind::Prelude => DependencyProjection::Prelude,
                };
                project_dependency_exports(state, projection, &dep_exports)?;
                projected_snapshot = snapshot_compile_env(state);
            }
            Err(err) => {
                if analysis_collector.is_enabled() {
                    // Continue compiling remaining dependencies; diagnostics
                    // were already recorded inside compile_module_with_adapter.
                    continue;
                }
                importing_stack.pop();
                return Err(err);
            }
        }
    }
    Ok(())
}

fn build_module_exports(
    ast: &crate::syntax::ast::SourceFile,
    canonical: &Path,
    alias: &str,
    state: &CompileState,
) -> ModuleExports {
    // Build ModuleExports from public declarations.
    // Pub let bindings get globally-unique LocalIds starting from state.next_global_local_id.
    let mut exports = ModuleExports::empty();
    exports.canonical_path = canonical.to_path_buf();
    let mut global_offset = state.next_global_local_id;
    for item in &ast.items {
        match item {
            Item::TypeDecl(decl) if decl.is_pub => {
                if let Some(type_id) = state.type_env.lookup_type(&decl.name) {
                    exports.public_types.insert(decl.name.clone(), type_id);
                }
            }
            Item::Function(decl) if decl.is_pub => {
                if let Some(sig) = state.value_env.get_function(&decl.name).cloned() {
                    exports.public_functions.insert(decl.name.clone(), sig);
                }
                // Use qualified name to avoid ambiguity if another module shares
                // the same bare function name.
                let qualified = format!("{}.{}", alias, decl.name);
                if let Some(&func_id) = state.func_table.get(&qualified) {
                    exports.public_func_ids.insert(decl.name.clone(), func_id);
                }
            }
            Item::ExternFunction(decl) if decl.is_pub => {
                // Extern functions use their extern-qualified name (e.g., "console.log")
                let extern_qualified = format!("{}.{}", decl.module, decl.name);
                if let Some(sig) = state.value_env.get_function(&extern_qualified).cloned() {
                    exports
                        .public_functions
                        .insert(extern_qualified.clone(), sig);
                }
                if let Some(&func_id) = state.func_table.get(&extern_qualified) {
                    exports.public_func_ids.insert(extern_qualified, func_id);
                }
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, _),
                is_pub,
                ..
            }) => {
                let local_id = LocalId(global_offset);
                if *is_pub {
                    if let Some(ty) = state.value_env.lookup(name) {
                        exports.public_values.insert(name.clone(), (ty, local_id));
                    }
                }
                global_offset += 1;
            }
            _ => {}
        }
    }
    exports
}

#[allow(clippy::too_many_arguments)]
fn maybe_lower_module(
    do_lower: bool,
    stage_runner: &ModuleStageRunner<'_>,
    ast: &crate::syntax::ast::SourceFile,
    alias: &str,
    file_registry: &FileRegistry,
    canonical: &Path,
    dep_canonical_paths: &[PathBuf],
    module_next_func_id: u32,
    type_map: crate::types::type_map::TypeMap,
    state: &mut CompileState,
    importing_stack: &mut Vec<PathBuf>,
    stage_trace: &mut Vec<CompileTraceEvent>,
) -> Result<()> {
    if !do_lower {
        return Ok(());
    }

    // Merge persistent method_func_targets into qualified_func_targets
    // so the lowerer can resolve transitive method FuncIds.
    let mut merged_func_targets = state.qualified_func_targets.clone();
    for (name, ext_ref) in &state.method_func_targets {
        merged_func_targets
            .entry(name.clone())
            .or_insert_with(|| ext_ref.clone());
    }
    let input = LowerInput {
        type_env: state.type_env.clone(),
        value_env: state.value_env.clone(),
        func_table: state.func_table.clone(),
        module_aliases: state.module_aliases.clone(),
        qualified_value_globals: state.qualified_value_globals.clone(),
        qualified_func_targets: merged_func_targets,
        next_func_id: module_next_func_id,
        next_global_local_id: state.next_global_local_id,
    };
    let lowered_result = match stage_runner.lower(ast, type_map, input, alias, file_registry) {
        Ok(result) => result,
        Err(err) => {
            importing_stack.pop();
            return Err(err);
        }
    };
    let mut lowered = lowered_result.value;
    lowered.module_path = canonical.to_path_buf();
    lowered.dependencies = dep_canonical_paths.to_vec();
    state.next_global_local_id = lowered.next_global_local_id_after;
    state.lowered_modules.push(lowered);
    record_stage_trace(
        stage_trace,
        canonical,
        CompileStage::Lower,
        lowered_result.cache_hit,
    );
    Ok(())
}

/// Assemble a CoreModule from per-module lowered artifacts.
/// User functions are module-local during lowering and remapped here to
/// deterministic global FuncIds.
pub fn link(state: CompileState) -> CoreModule {
    let mut modules = state.lowered_modules;
    let entry_module_key = state
        .entry_module_path
        .as_ref()
        .map(|p| path_key(p.as_path()));

    let order = topo_sort_modules(&modules);
    let key_to_idx = build_module_key_index(&modules);

    let mut local_to_global: HashMap<(usize, u32), FuncId> = HashMap::new();
    let mut next_global = prelude::USER_FUNC_START;
    // Build set of sparse prelude/intrinsic FuncIds to skip during linking
    let prelude_ids: std::collections::HashSet<u32> = crate::intrinsics::registry::all_specs()
        .iter()
        .map(|spec| spec.func_id.0)
        .collect();
    for &idx in &order {
        let mut local_ids: Vec<u32> = modules[idx]
            .functions
            .iter()
            .map(|f| f.func_id.0)
            .filter(|id| *id >= prelude::USER_FUNC_START)
            .collect();
        // Include extern import FuncIds so they get global remapping too
        for &ext_id in modules[idx].extern_imports.keys() {
            if ext_id.0 >= prelude::USER_FUNC_START {
                local_ids.push(ext_id.0);
            }
        }
        local_ids.sort_unstable();
        local_ids.dedup();
        for local_id in local_ids {
            while prelude_ids.contains(&next_global) {
                next_global += 1;
            }
            local_to_global.insert((idx, local_id), FuncId(next_global));
            next_global += 1;
        }
    }

    let mut linked_functions = Vec::new();
    let mut all_init_func_ids = Vec::new();
    let mut entry_init_func_id = None;
    let mut linked_extern_imports: HashMap<FuncId, crate::ir::core::ExternImport> = HashMap::new();

    for idx in order {
        let module_key = module_key(&modules[idx], idx);
        let module_entry = entry_module_key
            .as_ref()
            .is_some_and(|entry_key| entry_key == &module_key);

        let mut module = std::mem::replace(
            &mut modules[idx],
            LoweredModule {
                module_path: PathBuf::new(),
                dependencies: Vec::new(),
                functions: Vec::new(),
                init_func_id: None,
                external_func_refs: HashMap::new(),
                extern_imports: HashMap::new(),
                next_func_id_after: prelude::USER_FUNC_START,
                next_global_local_id_after: 0,
            },
        );
        let external_func_refs = module.external_func_refs.clone();

        for func in &mut module.functions {
            func.func_id = remap_func_id(
                func.func_id,
                idx,
                &external_func_refs,
                &key_to_idx,
                &local_to_global,
            );
            remap_expr_func_ids(
                &mut func.body,
                idx,
                &external_func_refs,
                &key_to_idx,
                &local_to_global,
            );
        }

        if let Some(init_local) = module.init_func_id {
            let mapped = remap_func_id(
                init_local,
                idx,
                &external_func_refs,
                &key_to_idx,
                &local_to_global,
            );
            all_init_func_ids.push(mapped);
            if module_entry {
                entry_init_func_id = Some(mapped);
            }
        }

        // Remap extern import FuncIds
        for (local_id, ext) in module.extern_imports {
            let global_id = remap_func_id(
                local_id,
                idx,
                &external_func_refs,
                &key_to_idx,
                &local_to_global,
            );
            linked_extern_imports.insert(global_id, ext);
        }

        linked_functions.extend(module.functions.into_iter());
    }

    CoreModule {
        functions: linked_functions,
        type_env: state.type_env,
        init_func_id: entry_init_func_id,
        all_init_func_ids,
        extern_imports: linked_extern_imports,
    }
}

fn build_module_key_index(modules: &[LoweredModule]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (idx, module) in modules.iter().enumerate() {
        out.insert(module_key(module, idx), idx);
    }
    out
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn module_key(module: &LoweredModule, idx: usize) -> String {
    if module.module_path.as_os_str().is_empty() {
        format!("<module:{}>", idx)
    } else {
        path_key(module.module_path.as_path())
    }
}

fn topo_sort_modules(modules: &[LoweredModule]) -> Vec<usize> {
    let key_to_idx = build_module_key_index(modules);
    let mut indegree = vec![0usize; modules.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); modules.len()];

    for (idx, module) in modules.iter().enumerate() {
        for dep in &module.dependencies {
            if let Some(&dep_idx) = key_to_idx.get(&path_key(dep.as_path())) {
                indegree[idx] += 1;
                dependents[dep_idx].push(idx);
            }
        }
    }

    let mut ready: BTreeSet<(String, usize)> = BTreeSet::new();
    for idx in 0..modules.len() {
        if indegree[idx] == 0 {
            ready.insert((module_key(&modules[idx], idx), idx));
        }
    }

    let mut order = Vec::with_capacity(modules.len());
    while let Some((_, idx)) = ready.pop_first() {
        order.push(idx);
        for &next in &dependents[idx] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.insert((module_key(&modules[next], next), next));
            }
        }
    }

    if order.len() != modules.len() {
        let mut remaining: Vec<(String, usize)> = (0..modules.len())
            .filter(|idx| !order.contains(idx))
            .map(|idx| (module_key(&modules[idx], idx), idx))
            .collect();
        remaining.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, idx) in remaining {
            order.push(idx);
        }
    }

    order
}

fn remap_func_id(
    id: FuncId,
    module_idx: usize,
    external_func_refs: &HashMap<FuncId, ExternalFuncRef>,
    key_to_idx: &HashMap<String, usize>,
    local_to_global: &HashMap<(usize, u32), FuncId>,
) -> FuncId {
    if id.0 < prelude::USER_FUNC_START {
        return id;
    }

    if let Some(target) = external_func_refs.get(&id) {
        let target_key = path_key(target.module_path.as_path());
        if let Some(&target_idx) = key_to_idx.get(&target_key) {
            if let Some(mapped) = local_to_global.get(&(target_idx, target.local_func_id.0)) {
                return *mapped;
            }
        }
    }

    if let Some(mapped) = local_to_global.get(&(module_idx, id.0)) {
        return *mapped;
    }

    id
}

fn remap_expr_func_ids(
    expr: &mut CoreExpr,
    module_idx: usize,
    external_func_refs: &HashMap<FuncId, ExternalFuncRef>,
    key_to_idx: &HashMap<String, usize>,
    local_to_global: &HashMap<(usize, u32), FuncId>,
) {
    match &mut expr.kind {
        CoreExprKind::GlobalFunc(id) => {
            *id = remap_func_id(
                *id,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::MakeClosure { func_id, .. } => {
            *func_id = remap_func_id(
                *func_id,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Let { value, body, .. } => {
            remap_expr_func_ids(
                value,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                body,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Assign { value, .. } => {
            remap_expr_func_ids(
                value,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::BinOp { left, right, .. } => {
            remap_expr_func_ids(
                left,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                right,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::UnOp { expr, .. } => {
            remap_expr_func_ids(
                expr,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Call { callee, args } => {
            remap_expr_func_ids(
                callee,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            for arg in args {
                remap_expr_func_ids(
                    arg,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::ContractCall { receiver, args, .. } => {
            remap_expr_func_ids(
                receiver,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            for arg in args {
                remap_expr_func_ids(
                    arg,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            remap_expr_func_ids(
                cond,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                then_branch,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                else_branch,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Match { scrutinee, arms } => {
            remap_expr_func_ids(
                scrutinee,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            for MatchArm { body, .. } in arms {
                remap_expr_func_ids(
                    body,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::Loop { body } => {
            remap_expr_func_ids(
                body,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Break { value } => {
            if let Some(value) = value {
                remap_expr_func_ids(
                    value,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::Return { value } => {
            if let Some(value) = value {
                remap_expr_func_ids(
                    value,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::Record { fields, .. } => {
            for (_, value) in fields {
                remap_expr_func_ids(
                    value,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::RecordGet { target, .. } => {
            remap_expr_func_ids(
                target,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Variant { args, .. } => {
            for arg in args {
                remap_expr_func_ids(
                    arg,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::ArrayLit { elements } => {
            for element in elements {
                remap_expr_func_ids(
                    element,
                    module_idx,
                    external_func_refs,
                    key_to_idx,
                    local_to_global,
                );
            }
        }
        CoreExprKind::Index { base, index } => {
            remap_expr_func_ids(
                base,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                index,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            remap_expr_func_ids(
                base,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
            remap_expr_func_ids(
                value,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::Defer(inner) => {
            remap_expr_func_ids(
                inner,
                module_idx,
                external_func_refs,
                key_to_idx,
                local_to_global,
            );
        }
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => {}
    }
}

/// Check-only pipeline (parse + resolve + typecheck, no lowering).
pub fn check_entry(file_path: &str) -> Result<FileRegistry> {
    let path = PathBuf::from(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let alias = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut ctx = CompilationContext::new();
    let mut state = CompileState::initial();
    let (_, registry) = compile_module(&path, &alias, &mut ctx, &mut vec![], &mut state, false)?;
    Ok(registry)
}

/// Register this module's own functions as inherent methods so that dot-syntax
/// (`p.method()`) works within the same file, not just cross-module.
///
/// The resolver has already built function signatures into `value_env`.
/// Two things are needed per method:
///   1. `type_env.add_method(type_id, "method", "module.method")` — for method lookup
///   2. `value_env.add_function(qualified_sig)` — so `synth_method_call` can
///      retrieve the signature by qualified name
fn register_inherent_methods(
    ast: &crate::syntax::ast::SourceFile,
    alias: &str,
    type_env: &mut TypeEnv,
    value_env: &mut ValueEnv,
    is_internal: bool,
) {
    // Collect TypeIds defined in this module so we only register inherent
    // methods for types the module owns.
    let local_type_ids: std::collections::HashSet<TypeId> = ast
        .items
        .iter()
        .filter_map(|item| {
            if let Item::TypeDecl(decl) = item {
                type_env.lookup_type(&decl.name)
            } else {
                None
            }
        })
        .collect();

    let registrations: Vec<(TypeId, String, FunctionSignature, Option<FunctionSignature>)> = ast
        .items
        .iter()
        .filter_map(|item| {
            if let Item::Function(decl) = item {
                if let Some(sig) = value_env.get_function(&decl.name) {
                    if let Some(receiver_ty) = sig.params.first() {
                        if let Some(type_id) = method_receiver_type_id(receiver_ty) {
                            // Only register methods for types defined in this module.
                            // Internal (stdlib/prelude) modules may also register
                            // methods on builtin types.
                            let is_local = local_type_ids.contains(&type_id);
                            if !is_local && !is_internal {
                                return None;
                            }
                            let method_qname = format!("{}.{}", alias, &decl.name);
                            let method_sig = FunctionSignature {
                                name: method_qname,
                                type_params: sig.type_params.clone(),
                                type_param_bounds: sig.type_param_bounds.clone(),
                                param_names: sig.param_names.clone(),
                                params: sig.params.clone(),
                                ret: sig.ret.clone(),
                                doc: sig.doc.clone(),
                                extern_module: sig.extern_module.clone(),
                            };
                            let builtin_sig = builtin_method_alias(type_id).map(|builtin_alias| {
                                FunctionSignature {
                                    name: format!("{}.{}", builtin_alias, &decl.name),
                                    type_params: sig.type_params.clone(),
                                    type_param_bounds: sig.type_param_bounds.clone(),
                                    param_names: sig.param_names.clone(),
                                    params: sig.params.clone(),
                                    ret: sig.ret.clone(),
                                    doc: sig.doc.clone(),
                                    extern_module: sig.extern_module.clone(),
                                }
                            });
                            return Some((type_id, decl.name.clone(), method_sig, builtin_sig));
                        }
                    }
                }
            }
            None
        })
        .collect();
    for (type_id, method_name, qsig, builtin_sig) in registrations {
        type_env.add_method(type_id, method_name, qsig.name.clone(), Some(qsig.clone()));
        value_env.add_function(qsig);
        if let Some(sig) = builtin_sig {
            value_env.add_function(sig);
        }
    }
}

fn cleanup_module_local_bindings(
    ast: &crate::syntax::ast::SourceFile,
    alias: &str,
    state: &mut CompileState,
) {
    // This module's declarations were needed during its own resolve/typecheck/lower
    // passes, but must not leak into subsequent modules. Cross-module access goes
    // through ModuleExports + register_module_exports.
    for item in &ast.items {
        match item {
            Item::TypeDecl(decl) => {
                state.type_env.remove_bare_type_name(&decl.name);
            }
            Item::Function(decl) => {
                let sig = state.value_env.get_function(&decl.name).cloned();

                state.value_env.remove_function(&decl.name);
                state.func_table.remove(&decl.name);

                let qualified = format!("{}.{}", alias, decl.name);
                state.value_env.remove_function(&qualified);
                state.func_table.remove(&qualified);
                state.qualified_func_targets.remove(&qualified);

                if let Some(sig) = sig {
                    if let Some(receiver_ty) = sig.params.first() {
                        if let Some(type_id) = method_receiver_type_id(receiver_ty) {
                            state.type_env.remove_method(type_id, &decl.name);
                            if let Some(builtin_alias) = builtin_method_alias(type_id) {
                                let builtin_name = format!("{}.{}", builtin_alias, decl.name);
                                state.value_env.remove_function(&builtin_name);
                                state.func_table.remove(&builtin_name);
                                state.qualified_func_targets.remove(&builtin_name);
                            }
                        }
                    }
                }
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, _),
                ..
            }) => {
                state.value_env.remove_value(name);
            }
            _ => {}
        }
    }
}

/// Full pipeline (parse + resolve + typecheck + lower).
pub fn compile_entry(file_path: &str) -> Result<(CoreModule, FileRegistry)> {
    let path = PathBuf::from(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let alias = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut ctx = CompilationContext::new();
    let mut state = CompileState::initial();
    state.entry_module_path = Some(path.clone());
    let (_, registry) = compile_module(&path, &alias, &mut ctx, &mut vec![], &mut state, true)?;
    Ok((dce::eliminate_dead_code(link(state)), registry))
}

/// Full pipeline for compiler-owned library modules.
///
/// Unlike `compile_entry`, this preserves the entry module's public functions as
/// DCE roots so the resulting artifact can be emitted as a separate Wasm module
/// and linked by namespace.
pub fn compile_entry_library(file_path: &str) -> Result<(CoreModule, ModuleExports, FileRegistry)> {
    let path = PathBuf::from(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let alias = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut ctx = CompilationContext::new();
    let mut state = CompileState::initial();
    state.entry_module_path = Some(path.clone());
    let (exports, registry) =
        compile_module(&path, &alias, &mut ctx, &mut vec![], &mut state, true)?;
    let linked = link(state);
    let public_func_names = exports
        .public_functions
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let extra_roots = linked
        .functions
        .iter()
        .filter(|func| public_func_names.contains(&func.name))
        .map(|func| func.func_id)
        .collect::<Vec<_>>();
    let linked = dce::eliminate_dead_code_with_roots(linked, &extra_roots);
    Ok((linked, exports, registry))
}

/// Full pipeline (parse + resolve + typecheck + lower) from an in-memory module map.
///
/// `sources` is keyed by absolute or project-root-relative paths to `.tw` files.
/// Imports are resolved logically from `project_root` for user modules and
/// `stdlib_root` for `@std.*` modules.
pub fn compile_entry_from_source_map(
    entry_path: &Path,
    sources: &HashMap<PathBuf, String>,
    project_root: &Path,
    stdlib_root: &Path,
) -> Result<(CoreModule, FileRegistry)> {
    let (core_module, registry, _) =
        compile_entry_from_source_map_with_trace(entry_path, sources, project_root, stdlib_root)?;
    Ok((core_module, registry))
}

/// Full pipeline (parse + resolve + typecheck + lower) from an in-memory module map,
/// with per-module stage trace events.
pub fn compile_entry_from_source_map_with_trace(
    entry_path: &Path,
    sources: &HashMap<PathBuf, String>,
    project_root: &Path,
    stdlib_root: &Path,
) -> Result<(CoreModule, FileRegistry, Vec<CompileTraceEvent>)> {
    let adapter = SourceMapModuleAdapter::new(project_root, stdlib_root, sources);
    let entry = adapter.canonicalize(entry_path);
    if !adapter.exists(&entry) {
        return Err(anyhow!(
            "Cannot read '{}': source not found in module map",
            entry.display()
        ));
    }

    let alias = entry
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut ctx = CompilationContext::new();
    let mut state = CompileState::initial();
    state.entry_module_path = Some(entry.clone());
    let mut stage_trace = Vec::new();
    let mut analysis_collector = AnalysisCollector::disabled();
    let (_, registry) = compile_module_with_adapter(
        &entry,
        &alias,
        &mut ctx,
        &mut vec![],
        &mut state,
        true,
        &adapter,
        &mut stage_trace,
        &mut analysis_collector,
    )?;
    Ok((dce::eliminate_dead_code(link(state)), registry, stage_trace))
}

/// Analysis-only pipeline (parse + resolve + typecheck, no lowering) from an
/// in-memory source map.
///
/// Returns per-module typed artifacts suitable for editor tooling.
pub fn analyze_entry_from_source_map(
    entry_path: &Path,
    sources: &HashMap<PathBuf, String>,
    project_root: &Path,
    stdlib_root: &Path,
) -> Result<WorkspaceAnalysis> {
    let adapter = SourceMapModuleAdapter::new(project_root, stdlib_root, sources);
    let entry = adapter.canonicalize(entry_path);
    if !adapter.exists(&entry) {
        return Err(anyhow!(
            "Cannot read '{}': source not found in module map",
            entry.display()
        ));
    }

    let alias = entry
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    let mut ctx = CompilationContext::new();
    let mut state = CompileState::initial();
    state.entry_module_path = Some(entry.clone());
    let mut stage_trace = Vec::new();
    let mut analysis_collector = AnalysisCollector::enabled();
    // Errors are recorded as diagnostics in the analysis collector;
    // we intentionally ignore the Result to return partial analysis.
    let _ = compile_module_with_adapter(
        &entry,
        &alias,
        &mut ctx,
        &mut vec![],
        &mut state,
        false,
        &adapter,
        &mut stage_trace,
        &mut analysis_collector,
    );
    Ok(analysis_collector.into_workspace(entry))
}
