pub mod context;
pub mod loader;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::ir::lower::Lowerer;
use crate::ir::CoreModule;
use crate::syntax::ast::Item;
use crate::syntax::span::FileRegistry;
use crate::types::check::TypeChecker;
use crate::types::resolve::Resolver;

pub use context::{CompilationContext, ModuleExports};
pub use loader::{find_project_root, resolve_module_path};

/// Compile a single module (file) and all its transitive dependencies.
///
/// When `do_lower` is true, lowers the module to Core IR and accumulates
/// `FunctionDef`s in `ctx.all_functions`.
///
/// Returns `(ModuleExports, FileRegistry)` for the compiled module.
pub fn compile_module(
    file_path: &Path,
    alias: &str,
    ctx: &mut CompilationContext,
    importing_stack: &mut Vec<PathBuf>,
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
            let result = compile_module(&dep_path, &dep_alias, ctx, importing_stack, do_lower);
            match result {
                Ok((dep_exports, _)) => ctx.register_module_exports(&dep_alias, &dep_exports),
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
            let func_id = ctx.alloc_func_id();
            // Register unqualified name for same-module calls (used by the lowerer
            // to assign FuncId to each FunctionDef — see lower_function).
            ctx.func_table.insert(decl.name.clone(), func_id);
            // Register qualified name for cross-module calls
            let qualified = format!("{}.{}", alias, decl.name);
            ctx.func_table.insert(qualified, func_id);
        }
    }

    // Resolve names using the shared context
    let resolve_result = Resolver::resolve_with_context(&ast, ctx);
    if let Err(errors) = resolve_result {
        let msgs: Vec<String> = errors
            .iter()
            .map(|e| e.format(&file_registry, Some(&ctx.type_env)))
            .collect();
        importing_stack.pop();
        return Err(anyhow!("{}", msgs.join("\n")));
    }

    // Type-check using shared context
    let type_map = match TypeChecker::check_module_with_context(&ast, ctx) {
        Ok(tm) => tm,
        Err(errors) => {
            let msgs: Vec<String> = errors
                .iter()
                .map(|e| e.format(&file_registry, Some(&ctx.type_env)))
                .collect();
            importing_stack.pop();
            return Err(anyhow!("{}", msgs.join("\n")));
        }
    };

    // Build ModuleExports from public declarations
    let mut exports = ModuleExports::empty();
    for item in &ast.items {
        match item {
            Item::TypeDecl(decl) if decl.is_pub => {
                if let Some(type_id) = ctx.type_env.lookup_type(&decl.name) {
                    exports.public_types.insert(decl.name.clone(), type_id);
                }
            }
            Item::Function(decl) if decl.is_pub => {
                if let Some(sig) = ctx.value_env.get_function(&decl.name).cloned() {
                    exports.public_functions.insert(decl.name.clone(), sig);
                }
                // Use qualified name to avoid ambiguity if another module shares
                // the same bare function name.
                let qualified = format!("{}.{}", alias, decl.name);
                if let Some(&func_id) = ctx.func_table.get(&qualified) {
                    exports.public_func_ids.insert(decl.name.clone(), func_id);
                }
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
            ctx.type_env.remove_bare_type_name(&decl.name);
        }
    }

    // Lower (if requested)
    if do_lower {
        let lowerer = Lowerer::new_with_context(type_map, ctx);
        match lowerer.lower_module_funcs(&ast) {
            Ok((functions, init_func_id)) => {
                ctx.all_functions.extend(functions);
                if let Some(id) = init_func_id {
                    ctx.init_func_id = Some(id);
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
    let (_, registry) = compile_module(&path, &alias, &mut ctx, &mut vec![], false)?;
    Ok(registry)
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
    let (_, registry) = compile_module(&path, &alias, &mut ctx, &mut vec![], true)?;
    Ok((
        CoreModule {
            functions: ctx.all_functions,
            type_env: ctx.type_env,
            init_func_id: ctx.init_func_id,
        },
        registry,
    ))
}
