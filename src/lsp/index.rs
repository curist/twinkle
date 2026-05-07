use crate::syntax::ast::{
    Block, CaseArm, Expr, ExprId, ExprKind, Item, Pattern, SourceFile, Stmt, StringPart,
};
use crate::syntax::span::{FileId, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprSpanEntry {
    pub span: Span,
    pub expr_id: ExprId,
}

#[derive(Debug, Clone, Default)]
pub struct ExprSpanIndex {
    entries: Vec<ExprSpanEntry>,
}

impl ExprSpanIndex {
    pub fn build(ast: &SourceFile) -> Self {
        let mut entries = Vec::new();
        collect_source_file(ast, &mut entries);
        entries.sort_by_key(|entry| (entry.span.start, entry.span.len(), entry.expr_id.0));
        Self { entries }
    }

    pub fn find_smallest_containing(
        &self,
        file_id: FileId,
        byte_offset: u32,
    ) -> Option<ExprSpanEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.span.file_id == file_id && entry.span.contains(byte_offset))
            .min_by_key(|entry| (entry.span.len(), std::cmp::Reverse(entry.span.start)))
            .copied()
    }

    pub fn find_containing(&self, file_id: FileId, byte_offset: u32) -> Vec<ExprSpanEntry> {
        let mut entries: Vec<ExprSpanEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.span.file_id == file_id && entry.span.contains(byte_offset))
            .copied()
            .collect();
        entries.sort_by_key(|entry| (entry.span.len(), std::cmp::Reverse(entry.span.start)));
        entries
    }
}

fn collect_source_file(ast: &SourceFile, entries: &mut Vec<ExprSpanEntry>) {
    for item in &ast.items {
        collect_item(item, entries);
    }
}

fn collect_item(item: &Item, entries: &mut Vec<ExprSpanEntry>) {
    match item {
        Item::Import(_) | Item::TypeDecl(_) | Item::ExternFunction(_) => {}
        Item::Function(decl) => collect_block(&decl.body, entries),
        Item::Stmt(stmt) => collect_stmt(stmt, entries),
    }
}

fn collect_block(block: &Block, entries: &mut Vec<ExprSpanEntry>) {
    for stmt in &block.stmts {
        collect_stmt(stmt, entries);
    }
}

fn collect_stmt(stmt: &Stmt, entries: &mut Vec<ExprSpanEntry>) {
    match stmt {
        Stmt::Let { value, .. } => collect_expr(value, entries),
        Stmt::For {
            pattern,
            index_pattern,
            iter,
            body,
            ..
        } => {
            collect_pattern(pattern, entries);
            if let Some(index_pattern) = index_pattern {
                collect_pattern(index_pattern, entries);
            }
            collect_expr(iter, entries);
            collect_block(body, entries);
        }
        Stmt::ForCond { cond, body, .. } => {
            collect_expr(cond, entries);
            collect_block(body, entries);
        }
        Stmt::Expr(expr) => collect_expr(expr, entries),
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_expr(value, entries);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::Defer { expr, .. } => collect_expr(expr, entries),
    }
}

fn collect_pattern(pattern: &Pattern, _entries: &mut Vec<ExprSpanEntry>) {
    // Patterns currently do not contain Expr nodes.
    let _ = pattern;
}

fn collect_case_arm(arm: &CaseArm, entries: &mut Vec<ExprSpanEntry>) {
    collect_pattern(&arm.pattern, entries);
    collect_expr(&arm.body, entries);
}

fn collect_expr(expr: &Expr, entries: &mut Vec<ExprSpanEntry>) {
    entries.push(ExprSpanEntry {
        span: expr.span,
        expr_id: expr.id,
    });

    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => {}
        ExprKind::Unary { expr, .. } => collect_expr(expr, entries),
        ExprKind::Binary { left, right, .. } => {
            collect_expr(left, entries);
            collect_expr(right, entries);
        }
        ExprKind::Call { callee, args } => {
            collect_expr(callee, entries);
            for arg in args {
                collect_expr(arg, entries);
            }
        }
        ExprKind::FieldAccess { base, .. } => collect_expr(base, entries),
        ExprKind::Index { base, index } => {
            collect_expr(base, entries);
            collect_expr(index, entries);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr(cond, entries);
            collect_expr(then_branch, entries);
            if let Some(else_branch) = else_branch {
                collect_expr(else_branch, entries);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            collect_expr(scrutinee, entries);
            for arm in arms {
                collect_case_arm(arm, entries);
            }
        }
        ExprKind::Block(block) => collect_block(block, entries),
        ExprKind::Array { elements } => {
            for element in elements {
                collect_expr(element, entries);
            }
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                collect_expr(value, entries);
            }
        }
        ExprKind::VariantLit { fields, .. } => {
            for field in fields {
                collect_expr(field, entries);
            }
        }
        ExprKind::Function(func) => collect_expr(&func.body, entries),
        ExprKind::Collect {
            pattern,
            index_pattern,
            iter,
            body,
        } => {
            collect_pattern(pattern, entries);
            if let Some(index_pattern) = index_pattern {
                collect_pattern(index_pattern, entries);
            }
            collect_expr(iter, entries);
            collect_expr(body, entries);
        }
        ExprKind::CollectWhile { cond, body } => {
            collect_expr(cond, entries);
            collect_expr(body, entries);
        }
        ExprKind::Try { expr } => collect_expr(expr, entries),
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let StringPart::Interpolation(expr) = part {
                    collect_expr(expr, entries);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_smallest_expression_for_nested_offsets() {
        let source = "value := add(1 + 2, 3)\n";
        let (ast, registry) = crate::syntax::parse_source(source, "test.tw").expect("parse");
        let file_id = ast.span.file_id;
        let index = ExprSpanIndex::build(&ast);

        let offset_literal = source.find('2').expect("literal present") as u32;
        let entry_literal = index
            .find_smallest_containing(file_id, offset_literal)
            .expect("literal expr should be indexed");
        assert_eq!(registry.snippet(entry_literal.span), Some("2"));

        let offset_plus = source.find('+').expect("plus present") as u32;
        let entry_plus = index
            .find_smallest_containing(file_id, offset_plus)
            .expect("binary expr should be indexed");
        let plus_snippet = registry.snippet(entry_plus.span).expect("span snippet");
        assert!(
            plus_snippet.contains("1 + 2"),
            "expected binary subexpression, got: {plus_snippet}"
        );
    }

    #[test]
    fn non_expression_offsets_return_none() {
        let source = "value := 1\n";
        let (ast, _registry) = crate::syntax::parse_source(source, "test.tw").expect("parse");
        let file_id = ast.span.file_id;
        let index = ExprSpanIndex::build(&ast);

        let offset_binding = source.find('v').expect("binding present") as u32;
        assert_eq!(
            index.find_smallest_containing(file_id, offset_binding),
            None
        );
    }
}
