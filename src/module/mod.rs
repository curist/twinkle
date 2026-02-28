pub mod artifacts;
pub mod context;
pub mod loader;

use std::fs;
use std::mem;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::ir::lower::{LowerInput, Lowerer};
use crate::ir::CoreModule;
use crate::ir::core::LocalId;
use crate::syntax::ast::{Item, Pattern, Stmt};
use crate::syntax::span::FileRegistry;
use crate::types::check::TypeChecker;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::resolve::Resolver;
use crate::types::ty::{FunctionSignature, MonoType, TypeId};

pub use context::{CompilationContext, CompileState, ModuleExports};
pub use artifacts::{LoweredModule, ResolvedModule, TypedModule};
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

    // Read source
    let source = fs::read_to_string(file_path)
        .map_err(|e| anyhow!("Cannot read '{}': {}", file_path.display(), e))?;

    // Parse
    let (ast, file_registry) =
        crate::syntax::parse_source(&source, &file_path.to_string_lossy())?;

    // Compile dependencies first (in source order)
    importing_stack.push(canonical.clone());
    let root = find_project_root(file_path.parent().unwrap_or(Path::new(".")));

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

    // Pre-assign FuncIds for this module's functions
    for item in &ast.items {
        if let Item::Function(decl) = item {
            let func_id = state.alloc_func_id();
            // Register unqualified name for same-module calls (used by the lowerer
            // to assign FuncId to each FunctionDef — see lower_function).
            state.func_table.insert(decl.name.clone(), func_id);
            // Register qualified name for cross-module calls
            let qualified = format!("{}.{}", alias, decl.name);
            state.func_table.insert(qualified, func_id);
        }
    }

    // Resolve — pure function; takes accumulated envs, returns updated envs
    let resolved = {
        let type_env = mem::replace(&mut state.type_env, TypeEnv::new());
        let value_env = mem::replace(&mut state.value_env, ValueEnv::new());
        match Resolver::resolve(&ast, type_env, value_env) {
            Ok(r) => r,
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.format(&file_registry, Some(&state.type_env)))
                    .collect();
                importing_stack.pop();
                return Err(anyhow!("{}", msgs.join("\n")));
            }
        }
    };
    state.type_env = resolved.type_env;
    state.value_env = resolved.value_env;

    // Register current module's own functions as inherent methods so that
    // p1.method() syntax works within the same file (not just cross-module).
    register_inherent_methods(&ast, alias, &mut state.type_env, &mut state.value_env);

    // Typecheck — pure function; takes accumulated envs, returns updated envs + TypeMap
    let typed = {
        let type_env = mem::replace(&mut state.type_env, TypeEnv::new());
        let value_env = mem::replace(&mut state.value_env, ValueEnv::new());
        match TypeChecker::check_module(&ast, type_env, value_env, state.module_aliases.clone()) {
            Ok(t) => t,
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.format(&file_registry, Some(&state.type_env)))
                    .collect();
                importing_stack.pop();
                return Err(anyhow!("{}", msgs.join("\n")));
            }
        }
    };
    state.type_env = typed.type_env;
    state.value_env = typed.value_env;

    // Build ModuleExports from public declarations.
    // Pub let bindings get globally-unique LocalIds starting from state.next_global_local_id.
    let mut exports = ModuleExports::empty();
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
        let input = LowerInput {
            type_env: state.type_env.clone(),
            func_table: state.func_table.clone(),
            module_aliases: state.module_aliases.clone(),
            qualified_value_globals: state.qualified_value_globals.clone(),
            next_func_id: state.next_func_id,
            next_global_local_id: state.next_global_local_id,
        };
        let lowerer = Lowerer::new_from_input(typed.type_map, input);
        match lowerer.lower_module_funcs(&ast) {
            Ok(lowered) => {
                state.next_global_local_id = lowered.next_global_local_id_after;
                state.next_func_id = lowered.next_func_id_after;
                let init_id = lowered.init_func_id;
                state.lowered_modules.push(lowered);
                if let Some(id) = init_id {
                    state.init_order.push(id);
                    state.entry_init_func_id = Some(id);
                }
            }
            Err(errs) => {
                let msgs: Vec<String> =
                    errs.iter().map(|e| e.format(&file_registry)).collect();
                importing_stack.pop();
                return Err(anyhow!("Lowering failed:\n{}", msgs.join("\n")));
            }
        }
    }

    importing_stack.pop();

    // Cache and return
    ctx.module_cache.insert(canonical, exports.clone());
    Ok((exports, file_registry))
}

/// Assemble a CoreModule from all accumulated lowered modules.
/// FuncIds are already globally stable — no remapping needed until
/// module-local IDs land (see docs/query-pipeline.md).
pub fn link(state: CompileState) -> CoreModule {
    let functions = state.lowered_modules
        .into_iter()
        .flat_map(|m| m.functions)
        .collect();
    CoreModule {
        functions,
        type_env: state.type_env,
        init_func_id: state.entry_init_func_id,
        all_init_func_ids: state.init_order,
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
    let (_, registry) = compile_module(&path, &alias, &mut ctx, &mut vec![], &mut state, true)?;
    Ok((link(state), registry))
}
