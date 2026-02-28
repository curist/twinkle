pub mod artifacts;
pub mod context;
pub mod loader;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::ir::lower::LowerInput;
use crate::ir::CoreModule;
use crate::ir::core::{CoreExpr, CoreExprKind, FuncId, LocalId, MatchArm};
use crate::ir::lower::prelude;
use crate::query::api::{
    lower_stage, parse_file, preassign_module_function_ids, resolve_stage, typecheck_stage,
};
use crate::query::cache::with_global_cache;
use crate::query::keys as query_keys;
use crate::syntax::ast::{Item, Pattern, Stmt};
use crate::syntax::span::FileRegistry;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{FunctionSignature, MonoType, TypeId};

pub use context::{CompilationContext, CompileState, ModuleExports};
pub use artifacts::{ExternalFuncRef, LoweredModule, ResolvedModule, TypedModule};
pub use loader::{find_project_root, resolve_module_path};

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
    // Canonicalize for deduplication / cycle detection
    let canonical = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());

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
    let source = fs::read_to_string(file_path)
        .map_err(|e| anyhow!("Cannot read '{}': {}", file_path.display(), e))?;
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
        let parsed = parse_file(file_path)?;
        with_global_cache(|cache| cache.put_parsed(&canonical, parse_key, parsed.clone()));
        parsed
    };
    let ast = parsed.ast.clone();
    let file_registry = parsed.file_registry.clone();

    // Compile dependencies first (in source order)
    importing_stack.push(canonical.clone());
    let root = find_project_root(file_path.parent().unwrap_or(Path::new(".")));
    let mut dep_canonical_paths = Vec::new();

    for item in &ast.items {
        if let Item::Import(import) = item {
            if import.is_stdlib {
                importing_stack.pop();
                return Err(anyhow!(
                    "@stdlib modules are not yet implemented (used in '{}')",
                    file_path.display()
                ));
            }
            let dep_path = resolve_module_path(&root, &import.module_path);
            let dep_canonical = dep_path
                .canonicalize()
                .unwrap_or_else(|_| dep_path.clone());
            dep_canonical_paths.push(dep_canonical);
            let dep_alias = import.module_name().to_string();
            let result = compile_module(&dep_path, &dep_alias, ctx, importing_stack, state, do_lower);
            match result {
                Ok((dep_exports, _)) => state.register_module_exports(&dep_alias, &dep_exports),
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
    preassign_module_function_ids(
        &ast,
        alias,
        &mut state.func_table,
        &mut module_next_func_id,
    );

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
        query_keys::typecheck_key(&canonical, source_hash, deps_hash),
        context_hash,
    );
    let typed = if let Some(cached) =
        with_global_cache(|cache| cache.get_typed(&canonical, typecheck_key))
    {
        cached
    } else {
        let type_env_for_errs = resolved.type_env.clone();
        let typed = match typecheck_stage(
            &ast,
            resolved.clone(),
            state.module_aliases.clone(),
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
            Item::Stmt(Stmt::Let { pattern: Pattern::Ident(name, _), is_pub, .. }) => {
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

    // Remove this module's bare type names from the shared TypeEnv.
    // They were needed during resolve and typecheck, but must not persist into
    // subsequent modules' resolution — otherwise two modules declaring a type
    // with the same name would silently overwrite each other's TypeId.
    // Cross-module access goes through qualified aliases ("module.TypeName")
    // registered by the importing module via register_module_exports.
    for item in &ast.items {
        if let Item::TypeDecl(decl) = item {
            state.type_env.remove_bare_type_name(&decl.name);
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
                    let msgs: Vec<String> =
                        errs.iter().map(|e| e.format(&file_registry)).collect();
                    importing_stack.pop();
                    return Err(anyhow!("Lowering failed:\n{}", msgs.join("\n")));
                }
            }
        }
    }

    let module_hash = query_keys::module_hash(source_hash, deps_hash);
    state.module_hashes.insert(canonical.clone(), module_hash);
    with_global_cache(|cache| cache.set_module_hash(&canonical, module_hash));

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
            *id = remap_func_id(*id, module_idx, external_func_refs, key_to_idx, local_to_global);
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
            remap_expr_func_ids(left, module_idx, external_func_refs, key_to_idx, local_to_global);
            remap_expr_func_ids(right, module_idx, external_func_refs, key_to_idx, local_to_global);
        }
        CoreExprKind::UnOp { expr, .. } => {
            remap_expr_func_ids(expr, module_idx, external_func_refs, key_to_idx, local_to_global);
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
            remap_expr_func_ids(cond, module_idx, external_func_refs, key_to_idx, local_to_global);
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
                remap_expr_func_ids(body, module_idx, external_func_refs, key_to_idx, local_to_global);
            }
        }
        CoreExprKind::Loop { body } => {
            remap_expr_func_ids(body, module_idx, external_func_refs, key_to_idx, local_to_global);
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
            remap_expr_func_ids(base, module_idx, external_func_refs, key_to_idx, local_to_global);
            remap_expr_func_ids(index, module_idx, external_func_refs, key_to_idx, local_to_global);
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            remap_expr_func_ids(base, module_idx, external_func_refs, key_to_idx, local_to_global);
            remap_expr_func_ids(value, module_idx, external_func_refs, key_to_idx, local_to_global);
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
    let registrations: Vec<(TypeId, String, FunctionSignature)> = ast.items.iter()
        .filter_map(|item| {
            if let Item::Function(decl) = item {
                if let Some(sig) = value_env.get_function(&decl.name) {
                    if let Some(MonoType::Named { type_id, .. }) = sig.params.first() {
                        let qname = format!("{}.{}", alias, &decl.name);
                        let qsig = FunctionSignature {
                            name: qname,
                            type_params: sig.type_params.clone(),
                            params: sig.params.clone(),
                            ret: sig.ret.clone(),
                        };
                        return Some((*type_id, decl.name.clone(), qsig));
                    }
                }
            }
            None
        })
        .collect();
    for (type_id, method_name, qsig) in registrations {
        type_env.add_method(type_id, method_name, qsig.name.clone());
        value_env.add_function(qsig);
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
    Ok((link(state), registry))
}
