pub mod artifacts;
pub mod context;
pub mod dce;
pub mod loader;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::ir::CoreModule;
use crate::ir::core::{CoreExpr, CoreExprKind, FuncId, LocalId, MatchArm};
use crate::ir::lower::LowerInput;
use crate::ir::lower::prelude;
use crate::query::api::{
    lower_stage, parse_source_module, preassign_module_function_ids, resolve_stage,
    typecheck_stage_with_options,
};
use crate::query::cache::with_global_cache;
use crate::query::keys as query_keys;
use crate::syntax::ast::{ImportDecl, Item, Pattern, Stmt};
use crate::syntax::span::FileRegistry;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{FunctionSignature, TypeId, builtin_method_alias, method_receiver_type_id};

pub use artifacts::{ExternalFuncRef, LoweredModule, ResolvedModule, TypedModule};
pub use context::{CompilationContext, CompileState, ModuleExports};
pub use loader::{
    find_project_root, list_prelude_modules_default, resolve_module_path,
    resolve_stdlib_module_path, resolve_stdlib_module_path_from_root,
};

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

    fn resolve_import_path(&self, _importing_file: &Path, import: &ImportDecl) -> PathBuf {
        if import.is_stdlib {
            resolve_stdlib_module_path_from_root(&self.stdlib_root, &import.module_path)
        } else {
            resolve_module_path(&self.project_root, &import.module_path)
        }
    }

    fn list_prelude_modules(&self) -> Vec<PathBuf> {
        // List prelude modules from the source map — prelude/ is a sibling of stdlib/
        let prelude_root = self.prelude_root();
        let mut paths: Vec<PathBuf> = self
            .sources
            .keys()
            .filter(|p| p.starts_with(&prelude_root) && p.extension().is_some_and(|e| e == "tw"))
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
    compile_module_with_adapter(
        file_path,
        alias,
        ctx,
        importing_stack,
        state,
        do_lower,
        &FsModuleSourceAdapter,
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
    let parse_key = query_keys::parse_key(&canonical, source_hash);
    let (cached_parsed, had_parse_entry) = with_global_cache(|cache| {
        let had = cache.has_parse_entry(&canonical);
        let parsed = cache.get_parsed(&canonical, parse_key);
        (parsed, had)
    });
    let parsed = if let Some(parsed) = cached_parsed {
        parsed
    } else {
        if had_parse_entry {
            with_global_cache(|cache| cache.invalidate_changed_module(&canonical));
        }
        let parsed = parse_source_module(&source, &canonical)?;
        with_global_cache(|cache| cache.put_parsed(&canonical, parse_key, parsed.clone()));
        parsed
    };
    let ast = parsed.ast.clone();
    let file_registry = parsed.file_registry.clone();

    // Compile dependencies first (in source order)
    importing_stack.push(canonical.clone());
    let mut dep_canonical_paths = Vec::new();

    for item in &ast.items {
        if let Item::Import(import) = item {
            let dep_path = adapter.resolve_import_path(file_path, import);
            let dep_canonical = adapter.canonicalize(&dep_path);
            if !adapter.exists(&dep_canonical) {
                importing_stack.pop();
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
            dep_canonical_paths.push(dep_canonical.clone());
            let dep_alias = import.module_name().to_string();
            let result = compile_module_with_adapter(
                &dep_canonical,
                &dep_alias,
                ctx,
                importing_stack,
                state,
                do_lower,
                adapter,
            );
            match result {
                Ok((dep_exports, _)) => state.register_module_exports(&dep_alias, &dep_exports),
                Err(e) => {
                    importing_stack.pop();
                    return Err(e);
                }
            }
        }
    }

    // Prelude auto-import: inject stdlib/prelude/*.tw modules unless:
    //  - This module is inside stdlib or prelude itself (avoid cycles)
    //  - The prelude module is already an explicit dependency (canonical-path dedupe)
    let stdlib_root_canonical = adapter.canonicalize(&adapter.stdlib_root());
    let prelude_root_canonical = adapter.canonicalize(&adapter.prelude_root());
    let is_internal = canonical.starts_with(&stdlib_root_canonical)
        || canonical.starts_with(&prelude_root_canonical);
    if !is_internal {
        let prelude_modules = adapter.list_prelude_modules();
        for prelude_path in &prelude_modules {
            let prelude_canonical = adapter.canonicalize(prelude_path);
            // Skip if already explicitly imported (canonical-path dedupe)
            if dep_canonical_paths.contains(&prelude_canonical) {
                continue;
            }
            if !adapter.exists(&prelude_canonical) {
                continue;
            }
            let prelude_alias = format!(
                "__prelude_{}",
                prelude_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );
            dep_canonical_paths.push(prelude_canonical.clone());
            let result = compile_module_with_adapter(
                &prelude_canonical,
                &prelude_alias,
                ctx,
                importing_stack,
                state,
                do_lower,
                adapter,
            );
            match result {
                Ok((dep_exports, _)) => {
                    state.register_module_exports(&prelude_alias, &dep_exports);
                }
                Err(e) => {
                    importing_stack.pop();
                    return Err(e);
                }
            }
        }
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

    // Pre-assign module-local FuncIds for this module's user functions.
    let mut module_next_func_id = prelude::USER_FUNC_START;
    preassign_module_function_ids(&ast, alias, &mut state.func_table, &mut module_next_func_id);

    // Resolve — pure function; takes accumulated envs, returns updated envs
    let resolve_key = query_keys::with_context(
        query_keys::resolve_key(&canonical, source_hash, deps_hash),
        context_hash,
    );
    let mut resolved = if let Some(cached) =
        with_global_cache(|cache| cache.get_resolved(&canonical, resolve_key))
    {
        cached
    } else {
        let type_env = state.type_env.clone();
        let value_env = state.value_env.clone();
        let type_env_for_errs = type_env.clone();
        let resolved = match resolve_stage(&ast, type_env, value_env) {
            Ok(r) => r,
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.format(&file_registry, Some(&type_env_for_errs)))
                    .collect();
                importing_stack.pop();
                return Err(anyhow!("{}", msgs.join("\n")));
            }
        };
        with_global_cache(|cache| cache.put_resolved(&canonical, resolve_key, resolved.clone()));
        resolved
    };
    // Register current module's own functions as inherent methods so that
    // p1.method() syntax works within the same file (not just cross-module).
    register_inherent_methods(&ast, alias, &mut resolved.type_env, &mut resolved.value_env);
    state.type_env = resolved.type_env.clone();
    state.value_env = resolved.value_env.clone();

    // Typecheck — pure function; takes explicit envs and returns updated envs + TypeMap
    let typecheck_key = query_keys::with_context(
        query_keys::typecheck_key(&canonical, source_hash, deps_hash, is_internal),
        context_hash,
    );
    let typed = if let Some(cached) =
        with_global_cache(|cache| cache.get_typed(&canonical, typecheck_key))
    {
        cached
    } else {
        let type_env_for_errs = resolved.type_env.clone();
        let typed = match typecheck_stage_with_options(
            &ast,
            resolved.clone(),
            state.module_aliases.clone(),
            is_internal,
        ) {
            Ok(t) => t,
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.format(&file_registry, Some(&type_env_for_errs)))
                    .collect();
                importing_stack.pop();
                return Err(anyhow!("{}", msgs.join("\n")));
            }
        };
        with_global_cache(|cache| cache.put_typed(&canonical, typecheck_key, typed.clone()));
        typed
    };
    state.type_env = typed.type_env;
    state.value_env = typed.value_env;

    // Build ModuleExports from public declarations.
    // Pub let bindings get globally-unique LocalIds starting from state.next_global_local_id.
    let mut exports = ModuleExports::empty();
    exports.canonical_path = canonical.clone();
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

    // Lower (if requested) — pure function via explicit LowerInput
    if do_lower {
        let lower_key = query_keys::with_context(
            query_keys::lower_key(
                &canonical,
                source_hash,
                deps_hash,
                state.next_global_local_id,
            ),
            context_hash,
        );
        let input = LowerInput {
            type_env: state.type_env.clone(),
            value_env: state.value_env.clone(),
            func_table: state.func_table.clone(),
            module_aliases: state.module_aliases.clone(),
            qualified_value_globals: state.qualified_value_globals.clone(),
            qualified_func_targets: state.qualified_func_targets.clone(),
            next_func_id: module_next_func_id,
            next_global_local_id: state.next_global_local_id,
        };
        let cached_lowered = with_global_cache(|cache| cache.get_lowered(&canonical, lower_key));
        if let Some(mut lowered) = cached_lowered {
            lowered.module_path = canonical.clone();
            lowered.dependencies = dep_canonical_paths.clone();
            state.next_global_local_id = lowered.next_global_local_id_after;
            state.lowered_modules.push(lowered);
        } else {
            match lower_stage(&ast, typed.type_map, input, alias) {
                Ok(mut lowered) => {
                    lowered.module_path = canonical.clone();
                    lowered.dependencies = dep_canonical_paths.clone();
                    with_global_cache(|cache| {
                        cache.put_lowered(&canonical, lower_key, lowered.clone());
                    });
                    state.next_global_local_id = lowered.next_global_local_id_after;
                    state.lowered_modules.push(lowered);
                }
                Err(errs) => {
                    let msgs: Vec<String> = errs.iter().map(|e| e.format(&file_registry)).collect();
                    importing_stack.pop();
                    return Err(anyhow!("Lowering failed:\n{}", msgs.join("\n")));
                }
            }
        }
    }

    let module_hash = query_keys::module_hash(source_hash, deps_hash);
    state.module_hashes.insert(canonical.clone(), module_hash);
    with_global_cache(|cache| cache.set_module_hash(&canonical, module_hash));

    cleanup_module_local_bindings(&ast, alias, state);

    importing_stack.pop();

    // Cache and return
    ctx.module_cache.insert(canonical, exports.clone());
    Ok((exports, file_registry))
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
    for &idx in &order {
        let mut local_ids: Vec<u32> = modules[idx]
            .functions
            .iter()
            .map(|f| f.func_id.0)
            .filter(|id| *id >= prelude::USER_FUNC_START)
            .collect();
        local_ids.sort_unstable();
        local_ids.dedup();
        for local_id in local_ids {
            local_to_global.insert((idx, local_id), FuncId(next_global));
            next_global += 1;
        }
    }

    let mut linked_functions = Vec::new();
    let mut all_init_func_ids = Vec::new();
    let mut entry_init_func_id = None;

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

        linked_functions.extend(module.functions.into_iter());
    }

    CoreModule {
        functions: linked_functions,
        type_env: state.type_env,
        init_func_id: entry_init_func_id,
        all_init_func_ids,
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
) {
    let registrations: Vec<(TypeId, String, FunctionSignature, Option<FunctionSignature>)> = ast
        .items
        .iter()
        .filter_map(|item| {
            if let Item::Function(decl) = item {
                if let Some(sig) = value_env.get_function(&decl.name) {
                    if let Some(receiver_ty) = sig.params.first() {
                        if let Some(type_id) = method_receiver_type_id(receiver_ty) {
                            let method_qname = format!("{}.{}", alias, &decl.name);
                            let method_sig = FunctionSignature {
                                name: method_qname,
                                type_params: sig.type_params.clone(),
                                params: sig.params.clone(),
                                ret: sig.ret.clone(),
                            };
                            let builtin_sig = builtin_method_alias(type_id).map(|builtin_alias| {
                                FunctionSignature {
                                    name: format!("{}.{}", builtin_alias, &decl.name),
                                    type_params: sig.type_params.clone(),
                                    params: sig.params.clone(),
                                    ret: sig.ret.clone(),
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
        type_env.add_method(type_id, method_name, qsig.name.clone());
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
    let (_, registry) = compile_module_with_adapter(
        &entry,
        &alias,
        &mut ctx,
        &mut vec![],
        &mut state,
        true,
        &adapter,
    )?;
    Ok((dce::eliminate_dead_code(link(state)), registry))
}
