use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::ir::core::{FuncId, LocalId};
use crate::ir::error::LowerError;
use crate::ir::lower::{LowerInput, Lowerer};
use crate::module::artifacts::{ExternalFuncRef, LoweredModule, ResolvedModule, TypedModule};
use crate::module::context::{default_func_table, default_module_aliases};
use crate::syntax::ast::{Item, Pattern, SourceFile, Stmt};
use crate::syntax::span::FileRegistry;
use crate::syntax::span::Span;
use crate::types::check::TypeChecker;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::error::TypeError;
use crate::types::resolve::Resolver;
use crate::types::ty::{FunctionSignature, TypeDef};
use crate::types::type_map::TypeMap;

/// Parsed source module artifact.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub ast: SourceFile,
    pub file_registry: FileRegistry,
    pub canonical_path: PathBuf,
    pub alias: String,
}

#[derive(Debug, Clone)]
pub struct QueryContext {
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
    pub func_table: HashMap<String, FuncId>,
    pub module_aliases: HashSet<String>,
    pub qualified_value_globals: HashMap<String, LocalId>,
    pub qualified_func_targets: HashMap<String, ExternalFuncRef>,
    pub next_global_local_id: u32,
}

impl QueryContext {
    pub fn lower_input(&self, type_env: TypeEnv, next_func_id: u32) -> LowerInput {
        LowerInput {
            type_env,
            value_env: self.value_env.clone(),
            func_table: self.func_table.clone(),
            module_aliases: self.module_aliases.clone(),
            qualified_value_globals: self.qualified_value_globals.clone(),
            qualified_func_targets: self.qualified_func_targets.clone(),
            next_func_id,
            next_global_local_id: self.next_global_local_id,
        }
    }
}

pub fn default_query_context() -> QueryContext {
    QueryContext {
        type_env: TypeEnv::new(),
        value_env: ValueEnv::new(),
        func_table: default_func_table(),
        module_aliases: default_module_aliases(),
        qualified_value_globals: HashMap::new(),
        qualified_func_targets: HashMap::new(),
        next_global_local_id: 0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySpan {
    pub file_id: u32,
    pub line: usize,
    pub column: usize,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Option<QuerySpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuerySymbolKind {
    Type,
    Function,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySymbol {
    pub name: String,
    pub kind: QuerySymbolKind,
    pub detail: String,
    pub span: Option<QuerySpan>,
    pub is_pub: bool,
}

/// Parse a source file into an AST artifact.
pub fn parse_file(file_path: &Path) -> Result<ParsedModule> {
    let canonical_path = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let source = fs::read_to_string(file_path)
        .map_err(|e| anyhow!("Cannot read '{}': {}", file_path.display(), e))?;
    parse_source_module(&source, &canonical_path)
}

/// Parse an in-memory source string into a module artifact.
///
/// This is the pure parsing entrypoint used by source-map based compilation.
pub fn parse_source_module(source: &str, canonical_path: &Path) -> Result<ParsedModule> {
    let (ast, file_registry) =
        crate::syntax::parse_source(source, &canonical_path.to_string_lossy())?;
    let alias = canonical_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();
    Ok(ParsedModule {
        ast,
        file_registry,
        canonical_path: canonical_path.to_path_buf(),
        alias,
    })
}

/// Resolve names/types for a module with explicit input environments.
pub fn resolve_stage(
    ast: &SourceFile,
    type_env: TypeEnv,
    value_env: ValueEnv,
) -> Result<ResolvedModule, Vec<TypeError>> {
    Resolver::resolve(ast, type_env, value_env)
}

/// Resolve with structured diagnostics suitable for tooling.
pub fn resolve_stage_with_diagnostics(
    ast: &SourceFile,
    type_env: TypeEnv,
    value_env: ValueEnv,
    registry: &FileRegistry,
) -> Result<ResolvedModule, Vec<QueryDiagnostic>> {
    let type_env_for_fmt = type_env.clone();
    match resolve_stage(ast, type_env, value_env) {
        Ok(r) => Ok(r),
        Err(errors) => Err(errors
            .iter()
            .map(|e| type_error_to_diagnostic(e, registry, Some(&type_env_for_fmt)))
            .collect()),
    }
}

/// Type-check a module using the resolver artifact and explicit module aliases.
pub fn typecheck_stage(
    ast: &SourceFile,
    resolved: ResolvedModule,
    module_aliases: HashSet<String>,
) -> Result<TypedModule, Vec<TypeError>> {
    TypeChecker::check_module(ast, resolved.type_env, resolved.value_env, module_aliases)
}

/// Type-check with structured diagnostics suitable for tooling.
pub fn typecheck_stage_with_diagnostics(
    ast: &SourceFile,
    resolved: ResolvedModule,
    module_aliases: HashSet<String>,
    registry: &FileRegistry,
) -> Result<TypedModule, Vec<QueryDiagnostic>> {
    let type_env_for_fmt = resolved.type_env.clone();
    match typecheck_stage(ast, resolved, module_aliases) {
        Ok(t) => Ok(t),
        Err(errors) => Err(errors
            .iter()
            .map(|e| type_error_to_diagnostic(e, registry, Some(&type_env_for_fmt)))
            .collect()),
    }
}

/// Pre-assign function IDs for a module into a function table.
///
/// This is idempotent for already-preassigned function names and keeps
/// `next_func_id` consistent across repeated calls.
pub fn preassign_module_function_ids(
    ast: &SourceFile,
    alias: &str,
    func_table: &mut HashMap<String, FuncId>,
    next_func_id: &mut u32,
) {
    for item in &ast.items {
        if let Item::Function(decl) = item {
            let qualified = format!("{}.{}", alias, decl.name);
            let func_id = if let Some(&existing) = func_table.get(&qualified) {
                existing
            } else {
                let id = FuncId(*next_func_id);
                *next_func_id += 1;
                id
            };

            // Bare names must point at the current module during lowering.
            func_table.insert(decl.name.clone(), func_id);
            // Register qualified name for cross-module and method calls.
            func_table.insert(qualified, func_id);
        }
    }
}

/// Lower a module with explicit lower input and a typed expression map.
///
/// The helper pre-assigns module function IDs (if missing) before lowering.
pub fn lower_stage(
    ast: &SourceFile,
    type_map: TypeMap,
    mut input: LowerInput,
    alias: &str,
) -> Result<LoweredModule, Vec<LowerError>> {
    preassign_module_function_ids(ast, alias, &mut input.func_table, &mut input.next_func_id);
    let lowerer = Lowerer::new_from_input(type_map, input);
    lowerer.lower_module_funcs(ast)
}

/// Collect user-facing symbol information for one module after type checking.
pub fn symbols_stage(
    ast: &SourceFile,
    typed: &TypedModule,
    registry: &FileRegistry,
) -> Vec<QuerySymbol> {
    let mut out = Vec::new();

    for item in &ast.items {
        match item {
            Item::TypeDecl(decl) => {
                let detail = typed
                    .type_env
                    .lookup_type(&decl.name)
                    .and_then(|type_id| typed.type_env.get_def(type_id))
                    .map(type_def_detail)
                    .unwrap_or_else(|| "type".to_string());
                out.push(QuerySymbol {
                    name: decl.name.clone(),
                    kind: QuerySymbolKind::Type,
                    detail,
                    span: span_to_query_span(registry, decl.span),
                    is_pub: decl.is_pub,
                });
            }
            Item::Function(decl) => {
                let detail = typed
                    .value_env
                    .get_function(&decl.name)
                    .map(|sig| format_signature(sig, &typed.type_env))
                    .unwrap_or_else(|| "fn".to_string());
                out.push(QuerySymbol {
                    name: decl.name.clone(),
                    kind: QuerySymbolKind::Function,
                    detail,
                    span: span_to_query_span(registry, decl.span),
                    is_pub: decl.is_pub,
                });
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, span),
                is_pub,
                ..
            }) => {
                let detail = typed
                    .value_env
                    .lookup(name)
                    .map(|ty| ty.format_with_names(&typed.type_env))
                    .unwrap_or_else(|| "value".to_string());
                out.push(QuerySymbol {
                    name: name.clone(),
                    kind: QuerySymbolKind::Value,
                    detail,
                    span: span_to_query_span(registry, *span),
                    is_pub: *is_pub,
                });
            }
            _ => {}
        }
    }

    out
}

fn type_error_to_diagnostic(
    err: &TypeError,
    registry: &FileRegistry,
    type_env: Option<&TypeEnv>,
) -> QueryDiagnostic {
    QueryDiagnostic {
        code: type_error_code(err),
        message: err.format(registry, type_env),
        span: span_to_query_span(registry, type_error_primary_span(err)),
    }
}

fn span_to_query_span(registry: &FileRegistry, span: Span) -> Option<QuerySpan> {
    registry.line_col(span).map(|(line, column)| QuerySpan {
        file_id: span.file_id.0,
        line,
        column,
        start: span.start,
        end: span.end,
    })
}

fn type_error_primary_span(err: &TypeError) -> Span {
    match err {
        TypeError::UndefinedType { span, .. } => *span,
        TypeError::UndefinedVariable { span, .. } => *span,
        TypeError::TypeMismatch { span, .. } => *span,
        TypeError::NonExhaustiveMatch { span, .. } => *span,
        TypeError::NotAFunction { span, .. } => *span,
        TypeError::WrongArity { span, .. } => *span,
        TypeError::NoSuchField { span, .. } => *span,
        TypeError::NoSuchVariant { span, .. } => *span,
        TypeError::DuplicateDefinition { second, .. } => *second,
        TypeError::CircularTypeAlias { span, .. } => *span,
        TypeError::AnonymousRecordWithoutContext { span } => *span,
        TypeError::GenericNotSupported { span, .. } => *span,
        TypeError::UnsupportedFeature { span, .. } => *span,
        TypeError::InvalidTopLevelItem { span, .. } => *span,
        TypeError::CaseScrutineeNotSumType { span, .. } => *span,
        TypeError::FieldMethodCollision { span, .. } => *span,
        TypeError::InvalidDictKey { span, .. } => *span,
        TypeError::ModuleScopeRebinding { span, .. } => *span,
        TypeError::OccursCheckFailed { span } => *span,
        TypeError::AmbiguousType { span, .. } => *span,
    }
}

fn type_error_code(err: &TypeError) -> &'static str {
    match err {
        TypeError::UndefinedType { .. } => "E_UNDEFINED_TYPE",
        TypeError::UndefinedVariable { .. } => "E_UNDEFINED_VARIABLE",
        TypeError::TypeMismatch { .. } => "E_TYPE_MISMATCH",
        TypeError::NonExhaustiveMatch { .. } => "E_NON_EXHAUSTIVE_MATCH",
        TypeError::NotAFunction { .. } => "E_NOT_A_FUNCTION",
        TypeError::WrongArity { .. } => "E_WRONG_ARITY",
        TypeError::NoSuchField { .. } => "E_NO_SUCH_FIELD",
        TypeError::NoSuchVariant { .. } => "E_NO_SUCH_VARIANT",
        TypeError::DuplicateDefinition { .. } => "E_DUPLICATE_DEFINITION",
        TypeError::CircularTypeAlias { .. } => "E_CIRCULAR_ALIAS",
        TypeError::AnonymousRecordWithoutContext { .. } => "E_ANON_RECORD_NO_CONTEXT",
        TypeError::GenericNotSupported { .. } => "E_GENERIC_NOT_SUPPORTED",
        TypeError::UnsupportedFeature { .. } => "E_UNSUPPORTED_FEATURE",
        TypeError::InvalidTopLevelItem { .. } => "E_INVALID_TOP_LEVEL_ITEM",
        TypeError::CaseScrutineeNotSumType { .. } => "E_CASE_SCRUTINEE_NOT_SUM",
        TypeError::FieldMethodCollision { .. } => "E_FIELD_METHOD_COLLISION",
        TypeError::InvalidDictKey { .. } => "E_INVALID_DICT_KEY",
        TypeError::ModuleScopeRebinding { .. } => "E_MODULE_SCOPE_REBINDING",
        TypeError::OccursCheckFailed { .. } => "E_OCCURS_CHECK_FAILED",
        TypeError::AmbiguousType { .. } => "E_AMBIGUOUS_TYPE",
    }
}

fn format_signature(sig: &FunctionSignature, type_env: &TypeEnv) -> String {
    let params = sig
        .params
        .iter()
        .map(|p| p.format_with_names(type_env))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig
        .ret
        .as_ref()
        .map(|r| r.format_with_names(type_env))
        .unwrap_or_else(|| "_".to_string());
    if sig.type_params.is_empty() {
        format!("fn({}) {}", params, ret)
    } else {
        format!("fn<{}>({}) {}", sig.type_params.join(", "), params, ret)
    }
}

fn type_def_detail(def: &TypeDef) -> String {
    let params = if def.type_params().is_empty() {
        String::new()
    } else {
        format!("<{}>", def.type_params().join(", "))
    };
    match def {
        TypeDef::Record { name, .. } => format!("type {}{} record", name, params),
        TypeDef::Sum { name, .. } => format!("type {}{} sum", name, params),
        TypeDef::Alias { name, .. } => format!("type {}{} alias", name, params),
    }
}
