use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::module::{AnalyzedModule, WorkspaceAnalysis};
use crate::syntax::ast::{
    Block, CaseArm, Expr, ExprId, ExprKind, Item, Pattern, SourceFile, Stmt, StringPart,
};
use crate::syntax::span::Span;
use crate::types::ty::method_receiver_type_id;

use super::index::ExprSpanIndex;
use super::position::{PositionUtf16, file_position_utf16_to_byte_offset};
use super::type_index::find_smallest_type_at_offset;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionTarget {
    pub path: PathBuf,
    pub span: Span,
}

pub fn definition_at_workspace(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    position: PositionUtf16,
) -> Option<DefinitionTarget> {
    let module = workspace.modules.get(module_path)?;
    let file_id = module.ast.span.file_id;
    let byte_offset = file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)?;
    let index = ExprSpanIndex::build(&module.ast);
    if let Some(entry) = index.find_smallest_containing(file_id, byte_offset) {
        if let Some(expr) = find_expr_by_id(&module.ast, entry.expr_id) {
            match &expr.kind {
                ExprKind::Ident(name) => {
                    let span = resolve_ident_binding_span(&module.ast, entry.expr_id, name)?;
                    return Some(DefinitionTarget {
                        path: module_path.to_path_buf(),
                        span,
                    });
                }
                ExprKind::FieldAccess { base, field } => {
                    if !is_offset_on_field(base, expr.span, byte_offset) {
                        return None;
                    }

                    if let Some(target) =
                        resolve_import_qualified_target(workspace, module, base, field)
                    {
                        return Some(target);
                    }

                    if let Some(target) =
                        resolve_method_target(workspace, module_path, module, base, field)
                    {
                        return Some(target);
                    }
                }
                _ => {}
            }
        }
    }

    let ty = find_smallest_type_at_offset(&module.ast, file_id, byte_offset)?;
    resolve_type_definition_target(workspace, module_path, module, ty)
}

fn resolve_import_qualified_target(
    workspace: &WorkspaceAnalysis,
    module: &AnalyzedModule,
    base: &Expr,
    field: &str,
) -> Option<DefinitionTarget> {
    let ExprKind::Ident(alias) = &base.kind else {
        return None;
    };
    let import = module.imports.iter().find(|entry| entry.alias == *alias)?;
    let span = find_top_level_any_named_span(workspace, &import.canonical_path, field)?;
    Some(DefinitionTarget {
        path: import.canonical_path.clone(),
        span,
    })
}

fn resolve_type_definition_target(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    module: &AnalyzedModule,
    ty: &crate::syntax::ast::Type,
) -> Option<DefinitionTarget> {
    let crate::syntax::ast::Type::Named { name, .. } = ty else {
        return None;
    };

    if let Some((alias, type_name)) = name.split_once('.') {
        let current_alias = module_alias(module_path);
        if alias == current_alias {
            let span = find_top_level_type_span(workspace, module_path, type_name)?;
            return Some(DefinitionTarget {
                path: module_path.to_path_buf(),
                span,
            });
        }

        let import = module.imports.iter().find(|entry| entry.alias == alias)?;
        let span = find_top_level_type_span(workspace, &import.canonical_path, type_name)?;
        return Some(DefinitionTarget {
            path: import.canonical_path.clone(),
            span,
        });
    }

    let span = find_top_level_type_span(workspace, module_path, name)?;
    Some(DefinitionTarget {
        path: module_path.to_path_buf(),
        span,
    })
}

fn resolve_method_target(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    module: &AnalyzedModule,
    base: &Expr,
    field: &str,
) -> Option<DefinitionTarget> {
    let base_ty = module.typed.type_map.get_expr_type(base.id)?;
    let receiver_type_id = method_receiver_type_id(base_ty)?;
    let qualified_name = module
        .typed
        .type_env
        .get_method_function(receiver_type_id, field)?;
    resolve_qualified_function_target(workspace, module_path, module, qualified_name)
}

fn resolve_qualified_function_target(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    module: &AnalyzedModule,
    qualified_name: &str,
) -> Option<DefinitionTarget> {
    let (_, local_name) = qualified_name.rsplit_once('.')?;

    if let Some(target) = module.qualified_func_targets.get(qualified_name) {
        let span = find_top_level_function_span(workspace, &target.module_path, local_name)?;
        return Some(DefinitionTarget {
            path: target.module_path.clone(),
            span,
        });
    }

    let (alias, _) = qualified_name.rsplit_once('.')?;
    let current_alias = module_alias(module_path);
    if alias == current_alias {
        let span = find_top_level_function_span(workspace, module_path, local_name)?;
        return Some(DefinitionTarget {
            path: module_path.to_path_buf(),
            span,
        });
    }

    let import = module.imports.iter().find(|entry| entry.alias == alias)?;
    let span = find_top_level_function_span(workspace, &import.canonical_path, local_name)?;
    Some(DefinitionTarget {
        path: import.canonical_path.clone(),
        span,
    })
}

fn is_offset_on_field(base: &Expr, span: Span, offset: u32) -> bool {
    offset > base.span.end && offset < span.end
}

fn module_alias(path: &Path) -> &str {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("main")
}

fn find_top_level_any_named_span(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    name: &str,
) -> Option<Span> {
    find_top_level_function_span(workspace, module_path, name)
        .or_else(|| find_top_level_value_span(workspace, module_path, name))
        .or_else(|| find_top_level_type_span(workspace, module_path, name))
}

fn find_top_level_function_span(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    name: &str,
) -> Option<Span> {
    let module = workspace.modules.get(module_path)?;
    for item in &module.ast.items {
        if let Item::Function(decl) = item {
            if decl.name == name {
                return identifier_span_after_keyword(module, decl.span, "fn", &decl.name)
                    .or(Some(decl.span));
            }
        }
    }
    None
}

fn find_top_level_value_span(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    name: &str,
) -> Option<Span> {
    let module = workspace.modules.get(module_path)?;
    for item in &module.ast.items {
        if let Item::Stmt(Stmt::Let {
            pattern: Pattern::Ident(binding, span),
            ..
        }) = item
        {
            if binding == name {
                return Some(*span);
            }
        }
    }
    None
}

fn find_top_level_type_span(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    name: &str,
) -> Option<Span> {
    let module = workspace.modules.get(module_path)?;
    for item in &module.ast.items {
        if let Item::TypeDecl(decl) = item {
            if decl.name == name {
                return identifier_span_after_keyword(module, decl.span, "type", &decl.name)
                    .or(Some(decl.span));
            }
        }
    }
    None
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

fn resolve_ident_binding_span(
    ast: &SourceFile,
    target_expr_id: ExprId,
    target_name: &str,
) -> Option<Span> {
    let mut resolver = LocalResolver::new(ast, target_expr_id, target_name);
    resolver.visit_source_file(ast);
    resolver.result
}

struct LocalResolver<'a> {
    target_expr_id: ExprId,
    target_name: &'a str,
    result: Option<Span>,
    scopes: Vec<HashMap<String, Span>>,
    module_let_bindings: HashMap<String, Span>,
}

impl<'a> LocalResolver<'a> {
    fn new(ast: &SourceFile, target_expr_id: ExprId, target_name: &'a str) -> Self {
        let mut global_functions = HashMap::new();
        let mut module_let_bindings = HashMap::new();
        for item in &ast.items {
            match item {
                Item::Function(decl) => {
                    global_functions.insert(decl.name.clone(), decl.span);
                }
                Item::Stmt(Stmt::Let {
                    pattern: Pattern::Ident(name, span),
                    ..
                }) => {
                    module_let_bindings.insert(name.clone(), *span);
                }
                _ => {}
            }
        }

        Self {
            target_expr_id,
            target_name,
            result: None,
            scopes: vec![global_functions],
            module_let_bindings,
        }
    }

    fn visit_source_file(&mut self, ast: &SourceFile) {
        for item in &ast.items {
            match item {
                Item::Function(decl) => self.visit_function_decl(decl),
                Item::Stmt(stmt) => self.visit_stmt(stmt),
                Item::Import(_) | Item::TypeDecl(_) => {}
            }
        }
    }

    fn visit_function_decl(&mut self, decl: &crate::syntax::ast::FunctionDecl) {
        self.push_scope();
        for (name, span) in self.module_let_bindings.clone() {
            self.bind(name, span);
        }
        for param in &decl.params {
            self.bind(param.name.clone(), param.span);
        }
        self.visit_block(&decl.body);
        self.pop_scope();
    }

    fn visit_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
        self.pop_scope();
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                self.visit_expr(value);
                self.bind_pattern(pattern);
            }
            Stmt::For {
                pattern,
                index_pattern,
                iter,
                body,
                ..
            } => {
                self.visit_expr(iter);
                self.push_scope();
                self.bind_pattern(pattern);
                if let Some(index_pattern) = index_pattern {
                    self.bind_pattern(index_pattern);
                }
                self.visit_block(body);
                self.pop_scope();
            }
            Stmt::ForCond { cond, body, .. } => {
                self.visit_expr(cond);
                self.visit_block(body);
            }
            Stmt::Expr(expr) => self.visit_expr(expr),
            Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.visit_expr(value);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Defer { expr, .. } => self.visit_expr(expr),
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Literal(_) => {}
            ExprKind::Ident(name) => {
                if self.result.is_none()
                    && expr.id == self.target_expr_id
                    && name == self.target_name
                {
                    self.result = self.lookup(name);
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExprKind::Unary { expr, .. } => self.visit_expr(expr),
            ExprKind::Call { callee, args } => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            ExprKind::FieldAccess { base, .. } => self.visit_expr(base),
            ExprKind::Index { base, index } => {
                self.visit_expr(base);
                self.visit_expr(index);
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(cond);
                self.visit_expr(then_branch);
                if let Some(else_branch) = else_branch {
                    self.visit_expr(else_branch);
                }
            }
            ExprKind::Case { scrutinee, arms } => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    self.visit_case_arm(arm);
                }
            }
            ExprKind::Block(block) => self.visit_block(block),
            ExprKind::Array { elements } => {
                for element in elements {
                    self.visit_expr(element);
                }
            }
            ExprKind::RecordLit { fields, .. } => {
                for (_, value) in fields {
                    self.visit_expr(value);
                }
            }
            ExprKind::VariantLit { fields, .. } => {
                for field in fields {
                    self.visit_expr(field);
                }
            }
            ExprKind::Function(func) => {
                self.push_scope();
                for param in &func.params {
                    self.bind(param.name.clone(), param.span);
                }
                self.visit_expr(&func.body);
                self.pop_scope();
            }
            ExprKind::Collect {
                pattern,
                index_pattern,
                iter,
                body,
            } => {
                self.visit_expr(iter);
                self.push_scope();
                self.bind_pattern(pattern);
                if let Some(index_pattern) = index_pattern {
                    self.bind_pattern(index_pattern);
                }
                self.visit_expr(body);
                self.pop_scope();
            }
            ExprKind::CollectWhile { cond, body } => {
                self.visit_expr(cond);
                self.visit_expr(body);
            }
            ExprKind::Try { expr } => self.visit_expr(expr),
            ExprKind::StringInterpolation { parts } => {
                for part in parts {
                    if let StringPart::Interpolation(expr) = part {
                        self.visit_expr(expr);
                    }
                }
            }
        }
    }

    fn visit_case_arm(&mut self, arm: &CaseArm) {
        self.push_scope();
        self.bind_pattern(&arm.pattern);
        self.visit_expr(&arm.body);
        self.pop_scope();
    }

    fn bind_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Ident(name, span) => self.bind(name.clone(), *span),
            Pattern::Variant { fields, .. } => {
                for field in fields {
                    self.bind_pattern(field);
                }
            }
            Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
        }
    }

    fn lookup(&self, name: &str) -> Option<Span> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn bind(&mut self, name: String, span: Span) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, span);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        let _ = self.scopes.pop();
    }
}

fn find_expr_by_id(ast: &SourceFile, target_id: ExprId) -> Option<&Expr> {
    for item in &ast.items {
        if let Some(expr) = find_expr_by_id_in_item(item, target_id) {
            return Some(expr);
        }
    }
    None
}

fn find_expr_by_id_in_item(item: &Item, target_id: ExprId) -> Option<&Expr> {
    match item {
        Item::Import(_) | Item::TypeDecl(_) => None,
        Item::Function(decl) => find_expr_by_id_in_block(&decl.body, target_id),
        Item::Stmt(stmt) => find_expr_by_id_in_stmt(stmt, target_id),
    }
}

fn find_expr_by_id_in_block(block: &Block, target_id: ExprId) -> Option<&Expr> {
    for stmt in &block.stmts {
        if let Some(expr) = find_expr_by_id_in_stmt(stmt, target_id) {
            return Some(expr);
        }
    }
    None
}

fn find_expr_by_id_in_stmt(stmt: &Stmt, target_id: ExprId) -> Option<&Expr> {
    match stmt {
        Stmt::Let { value, .. } => find_expr_by_id_in_expr(value, target_id),
        Stmt::For { iter, body, .. } => find_expr_by_id_in_expr(iter, target_id)
            .or_else(|| find_expr_by_id_in_block(body, target_id)),
        Stmt::ForCond { cond, body, .. } => find_expr_by_id_in_expr(cond, target_id)
            .or_else(|| find_expr_by_id_in_block(body, target_id)),
        Stmt::Expr(expr) => find_expr_by_id_in_expr(expr, target_id),
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|expr| find_expr_by_id_in_expr(expr, target_id)),
        Stmt::Continue { .. } => None,
        Stmt::Defer { expr, .. } => find_expr_by_id_in_expr(expr, target_id),
    }
}

fn find_expr_by_id_in_expr(expr: &Expr, target_id: ExprId) -> Option<&Expr> {
    if expr.id == target_id {
        return Some(expr);
    }

    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => None,
        ExprKind::Binary { left, right, .. } => find_expr_by_id_in_expr(left, target_id)
            .or_else(|| find_expr_by_id_in_expr(right, target_id)),
        ExprKind::Unary { expr, .. } => find_expr_by_id_in_expr(expr, target_id),
        ExprKind::Call { callee, args } => {
            if let Some(found) = find_expr_by_id_in_expr(callee, target_id) {
                return Some(found);
            }
            for arg in args {
                if let Some(found) = find_expr_by_id_in_expr(arg, target_id) {
                    return Some(found);
                }
            }
            None
        }
        ExprKind::FieldAccess { base, .. } => find_expr_by_id_in_expr(base, target_id),
        ExprKind::Index { base, index } => find_expr_by_id_in_expr(base, target_id)
            .or_else(|| find_expr_by_id_in_expr(index, target_id)),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_expr_by_id_in_expr(cond, target_id)
            .or_else(|| find_expr_by_id_in_expr(then_branch, target_id))
            .or_else(|| {
                else_branch
                    .as_ref()
                    .and_then(|expr| find_expr_by_id_in_expr(expr, target_id))
            }),
        ExprKind::Case { scrutinee, arms } => {
            if let Some(found) = find_expr_by_id_in_expr(scrutinee, target_id) {
                return Some(found);
            }
            for arm in arms {
                if let Some(found) = find_expr_by_id_in_expr(&arm.body, target_id) {
                    return Some(found);
                }
            }
            None
        }
        ExprKind::Block(block) => find_expr_by_id_in_block(block, target_id),
        ExprKind::Array { elements } => {
            for element in elements {
                if let Some(found) = find_expr_by_id_in_expr(element, target_id) {
                    return Some(found);
                }
            }
            None
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                if let Some(found) = find_expr_by_id_in_expr(value, target_id) {
                    return Some(found);
                }
            }
            None
        }
        ExprKind::VariantLit { fields, .. } => {
            for field in fields {
                if let Some(found) = find_expr_by_id_in_expr(field, target_id) {
                    return Some(found);
                }
            }
            None
        }
        ExprKind::Function(func) => find_expr_by_id_in_expr(&func.body, target_id),
        ExprKind::Collect { iter, body, .. } => find_expr_by_id_in_expr(iter, target_id)
            .or_else(|| find_expr_by_id_in_expr(body, target_id)),
        ExprKind::CollectWhile { cond, body } => find_expr_by_id_in_expr(cond, target_id)
            .or_else(|| find_expr_by_id_in_expr(body, target_id)),
        ExprKind::Try { expr } => find_expr_by_id_in_expr(expr, target_id),
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let StringPart::Interpolation(expr) = part {
                    if let Some(found) = find_expr_by_id_in_expr(expr, target_id) {
                        return Some(found);
                    }
                }
            }
            None
        }
    }
}
