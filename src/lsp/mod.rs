pub mod definition;
pub mod index;
pub mod position;
pub mod session;
pub mod type_index;

use crate::module::AnalyzedModule;
use crate::syntax::ast::{Expr, ExprId, ExprKind, Item, Pattern, Stmt, Type};
use crate::syntax::span::{FileId, Span};
use crate::types::error::TypeError;
use crate::types::ty::{FunctionSignature, MonoType, method_receiver_type_id};

use index::ExprSpanIndex;
use position::{PositionUtf16, file_position_utf16_to_byte_offset};
use type_index::find_smallest_type_at_offset;

pub fn expr_id_at_position(module: &AnalyzedModule, position: PositionUtf16) -> Option<ExprId> {
    let file_id = module.ast.span.file_id;
    let byte_offset = file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)?;
    let index = ExprSpanIndex::build(&module.ast);
    index
        .find_smallest_containing(file_id, byte_offset)
        .map(|entry| entry.expr_id)
}

pub fn hover_at_module(module: &AnalyzedModule, position: PositionUtf16) -> Option<String> {
    let file_id = module.ast.span.file_id;
    let byte_offset = file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)?;

    if let Some(function_type) = hover_function_signature_at_offset(module, file_id, byte_offset) {
        return Some(function_type);
    }

    if let Some(definition_hover) = hover_definition_at_offset(module, file_id, byte_offset) {
        return Some(definition_hover);
    }

    let index = ExprSpanIndex::build(&module.ast);
    for entry in index.find_containing(file_id, byte_offset) {
        if let Some(ty) = module.typed.type_map.get_expr_type(entry.expr_id) {
            return Some(ty.format_with_names(&module.typed.type_env));
        }
    }

    let type_node = find_smallest_type_at_offset(&module.ast, file_id, byte_offset)?;
    Some(format_type_hover(module, type_node))
}

fn format_type_hover(module: &AnalyzedModule, ty: &Type) -> String {
    let mut errors: Vec<TypeError> = Vec::new();
    if let Ok(mono) = module.typed.type_env.resolve_type(ty, &mut errors) {
        return mono.format_with_names(&module.typed.type_env);
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
        if let Some(sig) = module.typed.value_env.get_function(&qualified) {
            return Some(format_function_signature(sig, &module.typed.type_env));
        }
    }

    let base_ty = module.typed.type_map.get_expr_type(candidate.base.id)?;
    let receiver_type_id = method_receiver_type_id(base_ty)?;
    let qualified = module
        .typed
        .type_env
        .get_method_function(receiver_type_id, &candidate.field)?;
    let sig = module.typed.value_env.get_function(qualified)?;
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
    format!("fn({params}) {}", ret.format_with_names(type_env))
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
                    return Some(decl.name.clone());
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
        if is_offset_on_field(base, expr.span, byte_offset) {
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

fn is_offset_on_field(base: &Expr, span: Span, offset: u32) -> bool {
    offset > base.span.end && offset < span.end
}
