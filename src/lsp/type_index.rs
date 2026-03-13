use crate::syntax::ast::{
    Block, Expr, ExprKind, FunctionExpr, Item, Pattern, SourceFile, Stmt, StringPart, Type, TypeDef,
};
use crate::syntax::span::FileId;

pub fn find_smallest_type_at_offset(
    ast: &SourceFile,
    file_id: FileId,
    byte_offset: u32,
) -> Option<&Type> {
    let mut best = None;
    visit_source_file_types(ast, file_id, byte_offset, &mut best);
    best
}

fn visit_source_file_types<'a>(
    ast: &'a SourceFile,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    for item in &ast.items {
        visit_item_types(item, file_id, byte_offset, best);
    }
}

fn visit_item_types<'a>(
    item: &'a Item,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    match item {
        Item::Import(_) => {}
        Item::TypeDecl(decl) => visit_type_def(&decl.definition, file_id, byte_offset, best),
        Item::Function(decl) => {
            for param in &decl.params {
                if let Some(ty) = &param.ty {
                    visit_type(ty, file_id, byte_offset, best);
                }
            }
            if let Some(ret) = &decl.return_type {
                visit_type(ret, file_id, byte_offset, best);
            }
            visit_block_types(&decl.body, file_id, byte_offset, best);
        }
        Item::Stmt(stmt) => visit_stmt_types(stmt, file_id, byte_offset, best),
    }
}

fn visit_type_def<'a>(
    def: &'a TypeDef,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    match def {
        TypeDef::Record { fields } => {
            for field in fields {
                visit_type(&field.ty, file_id, byte_offset, best);
            }
        }
        TypeDef::Sum { variants } => {
            for variant in variants {
                for field in &variant.fields {
                    visit_type(field, file_id, byte_offset, best);
                }
            }
        }
        TypeDef::Alias { ty } => visit_type(ty, file_id, byte_offset, best),
    }
}

fn visit_block_types<'a>(
    block: &'a Block,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    for stmt in &block.stmts {
        visit_stmt_types(stmt, file_id, byte_offset, best);
    }
}

fn visit_stmt_types<'a>(
    stmt: &'a Stmt,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    match stmt {
        Stmt::Let {
            pattern, ty, value, ..
        } => {
            if let Some(ty) = ty {
                visit_type(ty, file_id, byte_offset, best);
            }
            visit_pattern_types(pattern, file_id, byte_offset, best);
            visit_expr_types(value, file_id, byte_offset, best);
        }
        Stmt::For {
            pattern,
            index_pattern,
            iter,
            body,
            ..
        } => {
            visit_pattern_types(pattern, file_id, byte_offset, best);
            if let Some(index_pattern) = index_pattern {
                visit_pattern_types(index_pattern, file_id, byte_offset, best);
            }
            visit_expr_types(iter, file_id, byte_offset, best);
            visit_block_types(body, file_id, byte_offset, best);
        }
        Stmt::ForCond { cond, body, .. } => {
            visit_expr_types(cond, file_id, byte_offset, best);
            visit_block_types(body, file_id, byte_offset, best);
        }
        Stmt::Expr(expr) => visit_expr_types(expr, file_id, byte_offset, best),
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            if let Some(value) = value {
                visit_expr_types(value, file_id, byte_offset, best);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::Defer { expr, .. } => visit_expr_types(expr, file_id, byte_offset, best),
    }
}

fn visit_pattern_types<'a>(
    pattern: &'a Pattern,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    match pattern {
        Pattern::Variant { fields, .. } => {
            for field in fields {
                visit_pattern_types(field, file_id, byte_offset, best);
            }
        }
        Pattern::Wildcard(_) | Pattern::Ident(_, _) | Pattern::Literal(_, _) => {}
    }
}

fn visit_expr_types<'a>(
    expr: &'a Expr,
    file_id: FileId,
    byte_offset: u32,
    best: &mut Option<&'a Type>,
) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
        ExprKind::Binary { left, right, .. } => {
            visit_expr_types(left, file_id, byte_offset, best);
            visit_expr_types(right, file_id, byte_offset, best);
        }
        ExprKind::Unary { expr, .. } => visit_expr_types(expr, file_id, byte_offset, best),
        ExprKind::Call { callee, args } => {
            visit_expr_types(callee, file_id, byte_offset, best);
            for arg in args {
                visit_expr_types(arg, file_id, byte_offset, best);
            }
        }
        ExprKind::FieldAccess { base, .. } => visit_expr_types(base, file_id, byte_offset, best),
        ExprKind::Index { base, index } => {
            visit_expr_types(base, file_id, byte_offset, best);
            visit_expr_types(index, file_id, byte_offset, best);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visit_expr_types(cond, file_id, byte_offset, best);
            visit_expr_types(then_branch, file_id, byte_offset, best);
            if let Some(else_branch) = else_branch {
                visit_expr_types(else_branch, file_id, byte_offset, best);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            visit_expr_types(scrutinee, file_id, byte_offset, best);
            for arm in arms {
                visit_pattern_types(&arm.pattern, file_id, byte_offset, best);
                visit_expr_types(&arm.body, file_id, byte_offset, best);
            }
        }
        ExprKind::Block(block) => visit_block_types(block, file_id, byte_offset, best),
        ExprKind::Array { elements } => {
            for element in elements {
                visit_expr_types(element, file_id, byte_offset, best);
            }
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                visit_expr_types(value, file_id, byte_offset, best);
            }
        }
        ExprKind::VariantLit { fields, .. } => {
            for field in fields {
                visit_expr_types(field, file_id, byte_offset, best);
            }
        }
        ExprKind::Function(FunctionExpr {
            params,
            return_type,
            body,
            ..
        }) => {
            for param in params {
                if let Some(ty) = &param.ty {
                    visit_type(ty, file_id, byte_offset, best);
                }
            }
            if let Some(ret) = return_type {
                visit_type(ret, file_id, byte_offset, best);
            }
            visit_expr_types(body, file_id, byte_offset, best);
        }
        ExprKind::Collect {
            pattern,
            index_pattern,
            iter,
            body,
        } => {
            visit_pattern_types(pattern, file_id, byte_offset, best);
            if let Some(index_pattern) = index_pattern {
                visit_pattern_types(index_pattern, file_id, byte_offset, best);
            }
            visit_expr_types(iter, file_id, byte_offset, best);
            visit_expr_types(body, file_id, byte_offset, best);
        }
        ExprKind::CollectWhile { cond, body } => {
            visit_expr_types(cond, file_id, byte_offset, best);
            visit_expr_types(body, file_id, byte_offset, best);
        }
        ExprKind::Try { expr } => visit_expr_types(expr, file_id, byte_offset, best),
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let StringPart::Interpolation(expr) = part {
                    visit_expr_types(expr, file_id, byte_offset, best);
                }
            }
        }
    }
}

fn visit_type<'a>(ty: &'a Type, file_id: FileId, byte_offset: u32, best: &mut Option<&'a Type>) {
    let span = ty.span();
    if span.file_id == file_id && span.contains(byte_offset) {
        match best {
            None => *best = Some(ty),
            Some(current) => {
                let current_span = current.span();
                if span.len() < current_span.len()
                    || (span.len() == current_span.len() && span.start > current_span.start)
                {
                    *best = Some(ty);
                }
            }
        }
    }

    match ty {
        Type::Named { args, .. } => {
            for arg in args {
                visit_type(arg, file_id, byte_offset, best);
            }
        }
        Type::Function { params, ret, .. } => {
            for param in params {
                visit_type(param, file_id, byte_offset, best);
            }
            visit_type(ret, file_id, byte_offset, best);
        }
    }
}
