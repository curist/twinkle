pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod index;
pub mod position;
pub mod session;
pub mod type_index;

use std::collections::HashMap;

use crate::module::AnalyzedModule;
use crate::syntax::ast::{Expr, ExprId, ExprKind, Item, Pattern, Stmt, Type};
use crate::syntax::span::{FileId, Span};
use crate::types::error::TypeError;
use crate::types::ty::{
    FunctionSignature, MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID, TypeDef, builtin_method_alias,
    method_receiver_type_id,
};

use index::ExprSpanIndex;
use position::{PositionUtf16, file_position_utf16_to_byte_offset};
use type_index::find_smallest_type_at_offset;

pub fn expr_id_at_position(module: &AnalyzedModule, position: PositionUtf16) -> Option<ExprId> {
    let file_id = module.ast.span.file_id;
    let byte_offset = file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)?;
    let source = module.file_registry.source(file_id)?;
    let span_offset = u32::try_from(source[..byte_offset as usize].chars().count()).ok()?;
    let index = ExprSpanIndex::build(&module.ast);
    index
        .find_smallest_containing(file_id, span_offset)
        .map(|entry| entry.expr_id)
}

pub fn hover_at_module(module: &AnalyzedModule, position: PositionUtf16) -> Option<String> {
    let file_id = module.ast.span.file_id;
    let byte_offset = file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)?;
    let source = module.file_registry.source(file_id)?;
    let span_offset = u32::try_from(source[..byte_offset as usize].chars().count()).ok()?;

    if let Some(function_type) = hover_function_signature_at_offset(module, file_id, span_offset) {
        return Some(function_type);
    }

    if let Some(definition_hover) = hover_definition_at_offset(module, file_id, span_offset) {
        return Some(definition_hover);
    }

    if let Some(variant_hover) = hover_case_pattern_variant_at_offset(module, file_id, span_offset)
    {
        return Some(variant_hover);
    }

    let index = ExprSpanIndex::build(&module.ast);
    for entry in index.find_containing(file_id, span_offset) {
        if let Some(ty) = module.typed.type_map.get_expr_type(entry.expr_id) {
            let type_str = ty.format_with_names(&module.typed.type_env);
            // If this expression is an identifier referring to a function with docs,
            // append the doc string below the type.
            if let Some(doc) = find_expr_doc(module, &entry) {
                return Some(format!("{type_str}\n\n{doc}"));
            }
            return Some(type_str);
        }
    }

    let type_node = find_smallest_type_at_offset(&module.ast, file_id, span_offset)?;
    Some(format_type_hover(module, type_node))
}

/// Look up doc string for an expression that references a named function.
/// Uses the source span to extract the identifier text and look up the function signature.
fn find_expr_doc(module: &AnalyzedModule, entry: &index::ExprSpanEntry) -> Option<String> {
    let source = module.file_registry.source(entry.span.file_id)?;
    let start = span_offset_to_byte_offset(source, entry.span.start)?;
    let end = span_offset_to_byte_offset(source, entry.span.end)?;
    let snippet = source.get(start..end)?;
    // Only look up docs for simple identifiers (no dots, no operators)
    if snippet.contains('.') || snippet.contains(' ') || snippet.contains('(') {
        return None;
    }
    // Check function signatures first (intrinsic signatures with docs)
    if let Some(sig) = module.typed.value_env.get_function(snippet) {
        return sig.doc.clone();
    }
    // Check builtin functions (println, error, range, etc.)
    builtin_value_doc(snippet).map(str::to_string)
}

fn span_offset_to_byte_offset(source: &str, span_offset: u32) -> Option<usize> {
    if span_offset == 0 {
        return Some(0);
    }
    let mut chars_seen = 0u32;
    for (idx, _) in source.char_indices() {
        if chars_seen == span_offset {
            return Some(idx);
        }
        chars_seen += 1;
    }
    if chars_seen == span_offset {
        Some(source.len())
    } else {
        None
    }
}

/// Hard-coded doc strings for builtin values registered in ValueEnv::builtins.
fn builtin_value_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "println" => "Print a string to stdout followed by a newline.",
        "print" => "Print a string to stdout without a trailing newline.",
        "eprintln" => "Print a string to stderr followed by a newline.",
        "eprint" => "Print a string to stderr without a trailing newline.",
        "error" => "Abort execution with an error message (trap).",
        _ => return None,
    })
}

fn format_type_hover(module: &AnalyzedModule, ty: &Type) -> String {
    let mut errors: Vec<TypeError> = Vec::new();
    if let Ok(mono) = module.typed.type_env.resolve_type(ty, &mut errors) {
        let type_str = mono.format_with_names(&module.typed.type_env);
        if let MonoType::Named { type_id, .. } = mono
            && let Some(def) = module.typed.type_env.get_def(type_id)
            && let Some(doc) = def.doc()
        {
            return format!("{type_str}\n\n{doc}");
        }
        return type_str;
    }
    format_ast_type(ty)
}

fn format_ast_type(ty: &Type) -> String {
    match ty {
        Type::Named { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                let args_str = args
                    .iter()
                    .map(format_ast_type)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args_str}>")
            }
        }
        Type::Function { params, ret, .. } => {
            let params_str = params
                .iter()
                .map(format_ast_type)
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({params_str}) {}", format_ast_type(ret))
        }
    }
}

fn hover_function_signature_at_offset(
    module: &AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
) -> Option<String> {
    let candidate = find_smallest_field_access_on_field(&module.ast, file_id, byte_offset)?;

    if let ExprKind::Ident(alias) = &candidate.base.kind {
        let qualified = format!("{}.{}", alias, candidate.field);
        let sig = module
            .typed
            .value_env
            .get_function(&qualified)
            .or_else(|| module.typed.value_env.get_function(&candidate.field));
        if let Some(sig) = sig {
            return Some(format_function_signature(sig, &module.typed.type_env));
        }
    }

    let base_ty = module.typed.type_map.get_expr_type(candidate.base.id)?;
    let receiver_type_id = method_receiver_type_id(base_ty)?;
    let sig = module
        .typed
        .type_env
        .get_method_function(receiver_type_id, &candidate.field)
        .and_then(|name| module.typed.value_env.get_function(name))
        .or_else(|| module.typed.value_env.get_function(&candidate.field))
        .or_else(|| {
            builtin_method_alias(receiver_type_id).and_then(|alias| {
                let name = format!("{alias}.{}", candidate.field);
                module.typed.value_env.get_function(&name)
            })
        })?;
    Some(format_function_signature(sig, &module.typed.type_env))
}

fn format_function_signature(
    sig: &FunctionSignature,
    type_env: &crate::types::env::TypeEnv,
) -> String {
    let params = sig
        .params
        .iter()
        .map(|p| p.format_with_names(type_env))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = sig.ret.clone().unwrap_or(MonoType::Void);
    let type_str = format!("fn({params}) {}", ret.format_with_names(type_env));
    match &sig.doc {
        Some(doc) => format!("{type_str}\n\n{doc}"),
        None => type_str,
    }
}

fn hover_definition_at_offset(
    module: &AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
) -> Option<String> {
    for item in &module.ast.items {
        match item {
            Item::Function(decl) => {
                let Some(name_span) =
                    identifier_span_after_keyword(module, decl.span, "fn", &decl.name)
                else {
                    continue;
                };
                if name_span.file_id == file_id && name_span.contains(byte_offset) {
                    return hover_named_value(module, &decl.name);
                }
            }
            Item::TypeDecl(decl) => {
                let Some(name_span) =
                    identifier_span_after_keyword(module, decl.span, "type", &decl.name)
                else {
                    continue;
                };
                if name_span.file_id == file_id && name_span.contains(byte_offset) {
                    return Some(match &decl.doc {
                        Some(doc) => format!("{}\n\n{}", decl.name, doc),
                        None => decl.name.clone(),
                    });
                }
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, span),
                ..
            }) => {
                if span.file_id == file_id && span.contains(byte_offset) {
                    return hover_named_value(module, name);
                }
            }
            Item::Import(_) | Item::Stmt(_) => {}
        }
    }
    None
}

fn hover_named_value(module: &AnalyzedModule, name: &str) -> Option<String> {
    if let Some(ty) = module.typed.value_env.lookup(name) {
        return Some(ty.format_with_names(&module.typed.type_env));
    }

    let file_name = module.file_registry.file_name(module.ast.span.file_id)?;
    let module_name = std::path::Path::new(file_name).file_stem()?.to_str()?;
    let qualified = format!("{module_name}.{name}");
    let ty = module.typed.value_env.lookup(&qualified)?;
    Some(ty.format_with_names(&module.typed.type_env))
}

fn identifier_span_after_keyword(
    module: &AnalyzedModule,
    decl_span: Span,
    keyword: &str,
    identifier: &str,
) -> Option<Span> {
    let text = module.file_registry.snippet(decl_span)?;
    let keyword_start = find_word(text, keyword)?;
    let mut idx = keyword_start + keyword.len();
    let bytes = text.as_bytes();
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx + identifier.len() > bytes.len() {
        return None;
    }
    if &text[idx..idx + identifier.len()] != identifier {
        return None;
    }

    let start = decl_span.start.checked_add(u32::try_from(idx).ok()?)?;
    let end = start.checked_add(u32::try_from(identifier.len()).ok()?)?;
    Some(Span {
        file_id: decl_span.file_id,
        start,
        end,
    })
}

fn find_word(text: &str, word: &str) -> Option<usize> {
    let mut start = 0usize;
    while let Some(rel) = text[start..].find(word) {
        let idx = start + rel;
        let before_ok = idx == 0 || !is_ident_byte(text.as_bytes()[idx - 1]);
        let after_idx = idx + word.len();
        let after_ok = after_idx >= text.len() || !is_ident_byte(text.as_bytes()[after_idx]);
        if before_ok && after_ok {
            return Some(idx);
        }
        start = idx + word.len();
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[derive(Clone, Copy)]
struct CasePatternVariantCandidate<'a> {
    pattern_span: Span,
    variant_name: &'a str,
    scrutinee_expr_id: ExprId,
}

fn hover_case_pattern_variant_at_offset(
    module: &AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
) -> Option<String> {
    let candidate = find_smallest_case_pattern_variant_at_offset(module, file_id, byte_offset)?;
    let scrutinee_ty = module
        .typed
        .type_map
        .get_expr_type(candidate.scrutinee_expr_id)?;
    let MonoType::Named { type_id, args } = scrutinee_ty else {
        return None;
    };
    let def = module.typed.type_env.get_def(*type_id)?;
    let TypeDef::Sum {
        name,
        type_params,
        variants,
        ..
    } = def
    else {
        return None;
    };
    let variant = variants.iter().find(|v| v.name == candidate.variant_name)?;
    let field_types = instantiate_variant_field_types(
        *type_id,
        args,
        type_params,
        &variant.fields,
        candidate.variant_name,
    );
    if field_types.is_empty() {
        Some(format!("{name}.{}", candidate.variant_name))
    } else {
        let payload = field_types
            .iter()
            .map(|t| t.format_with_names(&module.typed.type_env))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("{name}.{}({payload})", candidate.variant_name))
    }
}

fn instantiate_variant_field_types(
    type_id: crate::types::ty::TypeId,
    args: &[MonoType],
    type_params: &[String],
    fields: &[MonoType],
    variant_name: &str,
) -> Vec<MonoType> {
    if type_id == OPTION_TYPE_ID {
        return match variant_name {
            "None" => vec![],
            "Some" => vec![args.first().cloned().unwrap_or(MonoType::Void)],
            _ => fields.to_vec(),
        };
    }
    if type_id == RESULT_TYPE_ID {
        return match variant_name {
            "Ok" => vec![args.first().cloned().unwrap_or(MonoType::Void)],
            "Err" => vec![args.get(1).cloned().unwrap_or(MonoType::Void)],
            _ => fields.to_vec(),
        };
    }
    if args.is_empty() || type_params.is_empty() {
        return fields.to_vec();
    }

    let subst: HashMap<&str, MonoType> = type_params
        .iter()
        .zip(args.iter().cloned())
        .map(|(name, ty)| (name.as_str(), ty))
        .collect();
    fields
        .iter()
        .map(|field| substitute_type_vars(field, &subst))
        .collect()
}

fn substitute_type_vars(ty: &MonoType, subst: &HashMap<&str, MonoType>) -> MonoType {
    match ty {
        MonoType::Var(name) => subst
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(|| MonoType::Var(name.clone())),
        MonoType::Vector(elem) => MonoType::Vector(Box::new(substitute_type_vars(elem, subst))),
        MonoType::Dict(k, v) => MonoType::Dict(
            Box::new(substitute_type_vars(k, subst)),
            Box::new(substitute_type_vars(v, subst)),
        ),
        MonoType::Function { params, ret } => MonoType::Function {
            params: params
                .iter()
                .map(|param| substitute_type_vars(param, subst))
                .collect(),
            ret: Box::new(substitute_type_vars(ret, subst)),
        },
        MonoType::Named { type_id, args } => MonoType::Named {
            type_id: *type_id,
            args: args
                .iter()
                .map(|arg| substitute_type_vars(arg, subst))
                .collect(),
        },
        _ => ty.clone(),
    }
}

fn find_smallest_case_pattern_variant_at_offset<'a>(
    module: &'a AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
) -> Option<CasePatternVariantCandidate<'a>> {
    let mut best = None;
    for item in &module.ast.items {
        visit_item_case_pattern_variant(item, module, file_id, byte_offset, &mut best);
    }
    best
}

fn visit_item_case_pattern_variant<'a>(
    item: &'a Item,
    module: &'a AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<CasePatternVariantCandidate<'a>>,
) {
    match item {
        Item::Import(_) | Item::TypeDecl(_) => {}
        Item::Function(decl) => {
            for stmt in &decl.body.stmts {
                visit_stmt_case_pattern_variant(stmt, module, file_id, byte_offset, best);
            }
        }
        Item::Stmt(stmt) => {
            visit_stmt_case_pattern_variant(stmt, module, file_id, byte_offset, best)
        }
    }
}

fn visit_stmt_case_pattern_variant<'a>(
    stmt: &'a Stmt,
    module: &'a AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<CasePatternVariantCandidate<'a>>,
) {
    match stmt {
        Stmt::Let { value, .. } => {
            visit_expr_case_pattern_variant(value, module, file_id, byte_offset, best)
        }
        Stmt::For { iter, body, .. } => {
            visit_expr_case_pattern_variant(iter, module, file_id, byte_offset, best);
            for stmt in &body.stmts {
                visit_stmt_case_pattern_variant(stmt, module, file_id, byte_offset, best);
            }
        }
        Stmt::ForCond { cond, body, .. } => {
            visit_expr_case_pattern_variant(cond, module, file_id, byte_offset, best);
            for stmt in &body.stmts {
                visit_stmt_case_pattern_variant(stmt, module, file_id, byte_offset, best);
            }
        }
        Stmt::Expr(expr) => {
            visit_expr_case_pattern_variant(expr, module, file_id, byte_offset, best)
        }
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            if let Some(value) = value {
                visit_expr_case_pattern_variant(value, module, file_id, byte_offset, best);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::Defer { expr, .. } => {
            visit_expr_case_pattern_variant(expr, module, file_id, byte_offset, best)
        }
    }
}

fn visit_expr_case_pattern_variant<'a>(
    expr: &'a Expr,
    module: &'a AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<CasePatternVariantCandidate<'a>>,
) {
    if expr.span.file_id != file_id || !expr.span.contains(byte_offset) {
        return;
    }

    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
        ExprKind::Binary { left, right, .. } => {
            visit_expr_case_pattern_variant(left, module, file_id, byte_offset, best);
            visit_expr_case_pattern_variant(right, module, file_id, byte_offset, best);
        }
        ExprKind::Unary { expr, .. } => {
            visit_expr_case_pattern_variant(expr, module, file_id, byte_offset, best)
        }
        ExprKind::Call { callee, args } => {
            visit_expr_case_pattern_variant(callee, module, file_id, byte_offset, best);
            for arg in args {
                visit_expr_case_pattern_variant(arg, module, file_id, byte_offset, best);
            }
        }
        ExprKind::FieldAccess { base, .. } => {
            visit_expr_case_pattern_variant(base, module, file_id, byte_offset, best)
        }
        ExprKind::Index { base, index } => {
            visit_expr_case_pattern_variant(base, module, file_id, byte_offset, best);
            visit_expr_case_pattern_variant(index, module, file_id, byte_offset, best);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visit_expr_case_pattern_variant(cond, module, file_id, byte_offset, best);
            visit_expr_case_pattern_variant(then_branch, module, file_id, byte_offset, best);
            if let Some(else_branch) = else_branch {
                visit_expr_case_pattern_variant(else_branch, module, file_id, byte_offset, best);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            for arm in arms {
                visit_pattern_case_variant(
                    &arm.pattern,
                    scrutinee.id,
                    module,
                    file_id,
                    byte_offset,
                    best,
                );
            }
            visit_expr_case_pattern_variant(scrutinee, module, file_id, byte_offset, best);
            for arm in arms {
                visit_expr_case_pattern_variant(&arm.body, module, file_id, byte_offset, best);
            }
        }
        ExprKind::Block(block) => {
            for stmt in &block.stmts {
                visit_stmt_case_pattern_variant(stmt, module, file_id, byte_offset, best);
            }
        }
        ExprKind::Array { elements } => {
            for element in elements {
                visit_expr_case_pattern_variant(element, module, file_id, byte_offset, best);
            }
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                visit_expr_case_pattern_variant(value, module, file_id, byte_offset, best);
            }
        }
        ExprKind::VariantLit { fields, .. } => {
            for field in fields {
                visit_expr_case_pattern_variant(field, module, file_id, byte_offset, best);
            }
        }
        ExprKind::Function(func) => {
            visit_expr_case_pattern_variant(&func.body, module, file_id, byte_offset, best)
        }
        ExprKind::Collect { iter, body, .. } => {
            visit_expr_case_pattern_variant(iter, module, file_id, byte_offset, best);
            visit_expr_case_pattern_variant(body, module, file_id, byte_offset, best);
        }
        ExprKind::CollectWhile { cond, body } => {
            visit_expr_case_pattern_variant(cond, module, file_id, byte_offset, best);
            visit_expr_case_pattern_variant(body, module, file_id, byte_offset, best);
        }
        ExprKind::Try { expr } => {
            visit_expr_case_pattern_variant(expr, module, file_id, byte_offset, best)
        }
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let crate::syntax::ast::StringPart::Interpolation(expr) = part {
                    visit_expr_case_pattern_variant(expr, module, file_id, byte_offset, best);
                }
            }
        }
    }
}

fn visit_pattern_case_variant<'a>(
    pattern: &'a Pattern,
    scrutinee_expr_id: ExprId,
    module: &'a AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<CasePatternVariantCandidate<'a>>,
) {
    match pattern {
        Pattern::Variant {
            name, fields, span, ..
        } => {
            if span.file_id == file_id && span.contains(byte_offset) {
                let on_variant_name = find_identifier_span_within_span(module, *span, name)
                    .is_some_and(|name_span| name_span.contains(byte_offset));
                if on_variant_name {
                    let is_better = best.as_ref().is_none_or(|current| {
                        span.len() < current.pattern_span.len()
                            || (span.len() == current.pattern_span.len()
                                && span.start > current.pattern_span.start)
                    });
                    if is_better {
                        *best = Some(CasePatternVariantCandidate {
                            pattern_span: *span,
                            variant_name: name,
                            scrutinee_expr_id,
                        });
                    }
                }
            }
            for field in fields {
                visit_pattern_case_variant(
                    field,
                    scrutinee_expr_id,
                    module,
                    file_id,
                    byte_offset,
                    best,
                );
            }
        }
        Pattern::Wildcard(_) | Pattern::Ident(_, _) | Pattern::Literal(_, _) => {}
    }
}

fn find_identifier_span_within_span(
    module: &AnalyzedModule,
    containing_span: Span,
    identifier: &str,
) -> Option<Span> {
    let text = module.file_registry.snippet(containing_span)?;
    let idx = find_word(text, identifier)?;
    let start = containing_span
        .start
        .checked_add(u32::try_from(idx).ok()?)?;
    let end = start.checked_add(u32::try_from(identifier.len()).ok()?)?;
    Some(Span {
        file_id: containing_span.file_id,
        start,
        end,
    })
}

struct FieldAccessCandidate<'a> {
    span: Span,
    base: &'a Expr,
    field: String,
}

fn find_smallest_field_access_on_field(
    ast: &crate::syntax::ast::SourceFile,
    file_id: FileId,
    byte_offset: u32,
) -> Option<FieldAccessCandidate<'_>> {
    let mut best: Option<FieldAccessCandidate<'_>> = None;
    for item in &ast.items {
        visit_item_field_access(item, file_id, byte_offset, &mut best);
    }
    best
}

fn visit_item_field_access<'a>(
    item: &'a Item,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<FieldAccessCandidate<'a>>,
) {
    match item {
        Item::Import(_) | Item::TypeDecl(_) => {}
        Item::Function(decl) => {
            for stmt in &decl.body.stmts {
                visit_stmt_field_access(stmt, file_id, byte_offset, best);
            }
        }
        Item::Stmt(stmt) => visit_stmt_field_access(stmt, file_id, byte_offset, best),
    }
}

fn visit_stmt_field_access<'a>(
    stmt: &'a Stmt,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<FieldAccessCandidate<'a>>,
) {
    match stmt {
        Stmt::Let { value, .. } => visit_expr_field_access(value, file_id, byte_offset, best),
        Stmt::For { iter, body, .. } => {
            visit_expr_field_access(iter, file_id, byte_offset, best);
            for stmt in &body.stmts {
                visit_stmt_field_access(stmt, file_id, byte_offset, best);
            }
        }
        Stmt::ForCond { cond, body, .. } => {
            visit_expr_field_access(cond, file_id, byte_offset, best);
            for stmt in &body.stmts {
                visit_stmt_field_access(stmt, file_id, byte_offset, best);
            }
        }
        Stmt::Expr(expr) => visit_expr_field_access(expr, file_id, byte_offset, best),
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            if let Some(value) = value {
                visit_expr_field_access(value, file_id, byte_offset, best);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::Defer { expr, .. } => visit_expr_field_access(expr, file_id, byte_offset, best),
    }
}

fn visit_expr_field_access<'a>(
    expr: &'a Expr,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<FieldAccessCandidate<'a>>,
) {
    if expr.span.file_id != file_id || !expr.span.contains(byte_offset) {
        return;
    }

    if let ExprKind::FieldAccess { base, field } = &expr.kind {
        if is_offset_on_field(field, expr.span, byte_offset) {
            let is_better = best.as_ref().is_none_or(|current| {
                expr.span.len() < current.span.len()
                    || (expr.span.len() == current.span.len()
                        && expr.span.start > current.span.start)
            });
            if is_better {
                *best = Some(FieldAccessCandidate {
                    span: expr.span,
                    base,
                    field: field.clone(),
                });
            }
        }
    }

    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
        ExprKind::Binary { left, right, .. } => {
            visit_expr_field_access(left, file_id, byte_offset, best);
            visit_expr_field_access(right, file_id, byte_offset, best);
        }
        ExprKind::Unary { expr, .. } => visit_expr_field_access(expr, file_id, byte_offset, best),
        ExprKind::Call { callee, args } => {
            visit_expr_field_access(callee, file_id, byte_offset, best);
            for arg in args {
                visit_expr_field_access(arg, file_id, byte_offset, best);
            }
        }
        ExprKind::FieldAccess { base, .. } => {
            visit_expr_field_access(base, file_id, byte_offset, best)
        }
        ExprKind::Index { base, index } => {
            visit_expr_field_access(base, file_id, byte_offset, best);
            visit_expr_field_access(index, file_id, byte_offset, best);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visit_expr_field_access(cond, file_id, byte_offset, best);
            visit_expr_field_access(then_branch, file_id, byte_offset, best);
            if let Some(else_branch) = else_branch {
                visit_expr_field_access(else_branch, file_id, byte_offset, best);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            visit_expr_field_access(scrutinee, file_id, byte_offset, best);
            for arm in arms {
                visit_expr_field_access(&arm.body, file_id, byte_offset, best);
            }
        }
        ExprKind::Block(block) => {
            for stmt in &block.stmts {
                visit_stmt_field_access(stmt, file_id, byte_offset, best);
            }
        }
        ExprKind::Array { elements } => {
            for element in elements {
                visit_expr_field_access(element, file_id, byte_offset, best);
            }
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                visit_expr_field_access(value, file_id, byte_offset, best);
            }
        }
        ExprKind::VariantLit { fields, .. } => {
            for field in fields {
                visit_expr_field_access(field, file_id, byte_offset, best);
            }
        }
        ExprKind::Function(func) => visit_expr_field_access(&func.body, file_id, byte_offset, best),
        ExprKind::Collect { iter, body, .. } => {
            visit_expr_field_access(iter, file_id, byte_offset, best);
            visit_expr_field_access(body, file_id, byte_offset, best);
        }
        ExprKind::CollectWhile { cond, body } => {
            visit_expr_field_access(cond, file_id, byte_offset, best);
            visit_expr_field_access(body, file_id, byte_offset, best);
        }
        ExprKind::Try { expr } => visit_expr_field_access(expr, file_id, byte_offset, best),
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let crate::syntax::ast::StringPart::Interpolation(expr) = part {
                    visit_expr_field_access(expr, file_id, byte_offset, best);
                }
            }
        }
    }
}

fn is_offset_on_field(field: &str, span: Span, offset: u32) -> bool {
    let Ok(field_len) = u32::try_from(field.len()) else {
        return false;
    };
    let field_start = span.end.saturating_sub(field_len);
    offset >= field_start && offset <= span.end
}
