use std::collections::HashMap;
use std::path::Path;

use crate::module::{AnalyzedModule, WorkspaceAnalysis};
use crate::syntax::ast::{Block, Expr, ExprKind, Item, Pattern, SourceFile, Stmt, StringPart};
use crate::syntax::span::FileId;
use crate::types::ty::{MonoType, TypeDef, method_receiver_type_id};

use super::position::{PositionUtf16, file_position_utf16_to_byte_offset};

/// LSP CompletionItem kind numbers (from the LSP spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Function = 3,
    Variable = 6,
    Field = 5,
    Method = 2,
    Module = 9,
    Keyword = 14,
    Struct = 22,
    EnumMember = 20,
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

/// Compute completions for a position in a module.
pub fn completions_at_module(
    workspace: &WorkspaceAnalysis,
    module_path: &Path,
    position: PositionUtf16,
) -> Vec<CompletionItem> {
    let Some(module) = workspace.modules.get(module_path) else {
        return Vec::new();
    };
    let file_id = module.ast.span.file_id;
    let Some(byte_offset) =
        file_position_utf16_to_byte_offset(&module.file_registry, file_id, position)
    else {
        return Vec::new();
    };

    let source = match module.file_registry.source(file_id) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let context = classify_context(source, byte_offset as usize);

    match context {
        CompletionContext::Dot { dot_offset } => {
            dot_completions(workspace, module, file_id, dot_offset as u32)
        }
        CompletionContext::Identifier { prefix } => {
            identifier_completions(module, file_id, byte_offset, &prefix)
        }
    }
}

enum CompletionContext {
    /// Cursor is after a `.` — complete fields/methods/module members.
    Dot { dot_offset: usize },
    /// Cursor is at a bare identifier position.
    Identifier { prefix: String },
}

/// Classify the completion context by looking at source text before the cursor.
fn classify_context(source: &str, byte_offset: usize) -> CompletionContext {
    let before = &source[..byte_offset.min(source.len())];

    // Walk backwards over identifier characters to find any prefix being typed
    let prefix_start = before
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    let prefix = &before[before.len() - prefix_start..];

    // Check if the character before the prefix is a dot
    let before_prefix = &before[..before.len() - prefix_start];
    let trimmed = before_prefix.trim_end();
    if trimmed.ends_with('.') {
        // Find the dot's byte offset in the original source
        let dot_byte = trimmed.len() - 1;
        return CompletionContext::Dot {
            dot_offset: dot_byte,
        };
    }

    CompletionContext::Identifier {
        prefix: prefix.to_string(),
    }
}

/// Completions after a dot: fields, methods, or module-qualified names.
fn dot_completions(
    workspace: &WorkspaceAnalysis,
    module: &AnalyzedModule,
    file_id: FileId,
    dot_offset: u32,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Find the expression just before the dot using ExprSpanIndex
    let index = super::index::ExprSpanIndex::build(&module.ast);

    // The expression before the dot should end at or near dot_offset.
    // Find the smallest expression containing the position just before the dot.
    if let Some(entry) = index.find_smallest_containing(file_id, dot_offset.saturating_sub(1)) {
        // Check if the base is an import alias
        if let Some(expr) = find_expr_by_id(&module.ast, entry.expr_id) {
            if let ExprKind::Ident(alias) = &expr.kind {
                // Check if this identifier is an import alias
                if let Some(import) = module.imports.iter().find(|imp| imp.alias == *alias) {
                    // Offer exported names from the imported module
                    if let Some(target_module) = workspace.modules.get(&import.canonical_path) {
                        add_module_exports(&mut items, target_module);
                        sort_completion_items(&mut items);
                        return items;
                    }
                }
            }

            // Otherwise, try to get the type of the base expression for method/field completion
            if let Some(base_ty) = module.typed.type_map.get_expr_type(entry.expr_id) {
                add_type_members(&mut items, module, base_ty);
                sort_completion_items(&mut items);
                return items;
            }
        }
    }

    sort_completion_items(&mut items);
    items
}

/// Add exported names from a module (pub functions, pub types, pub values).
fn add_module_exports(items: &mut Vec<CompletionItem>, module: &AnalyzedModule) {
    for item in &module.ast.items {
        match item {
            Item::Function(decl) if decl.is_pub => {
                let detail = format_function_detail(module, &decl.name);
                items.push(CompletionItem {
                    label: decl.name.clone(),
                    kind: CompletionKind::Function,
                    detail: detail.clone(),
                    documentation: function_doc(module, &decl.name),
                });
            }
            Item::TypeDecl(decl) if decl.is_pub => {
                items.push(CompletionItem {
                    label: decl.name.clone(),
                    kind: CompletionKind::Struct,
                    detail: None,
                    documentation: decl.doc.clone(),
                });
            }
            Item::Stmt(Stmt::Let {
                is_pub: true,
                pattern: Pattern::Ident(name, _),
                doc,
                ..
            }) => {
                let detail = module
                    .typed
                    .value_env
                    .lookup(name)
                    .map(|ty| ty.format_with_names(&module.typed.type_env));
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: CompletionKind::Variable,
                    detail,
                    documentation: doc.clone(),
                });
            }
            _ => {}
        }
    }
}

/// Add fields and methods for a type.
fn add_type_members(items: &mut Vec<CompletionItem>, module: &AnalyzedModule, ty: &MonoType) {
    // Record fields
    if let MonoType::Named { type_id, .. } = ty {
        if let Some(fields) = module.typed.type_env.get_record_fields(*type_id) {
            for field in fields {
                items.push(CompletionItem {
                    label: field.name.clone(),
                    kind: CompletionKind::Field,
                    detail: Some(field.ty.format_with_names(&module.typed.type_env)),
                    documentation: None,
                });
            }
        }
    }

    // Methods
    if let Some(receiver_type_id) = method_receiver_type_id(ty) {
        let methods = module.typed.type_env.methods_for_type(receiver_type_id);
        for (method_name, qualified_fn) in methods {
            let detail = format_method_detail(module, qualified_fn);
            let documentation = method_doc(module, qualified_fn);
            items.push(CompletionItem {
                label: method_name.to_string(),
                kind: CompletionKind::Method,
                detail,
                documentation,
            });
        }
    }
}

/// Completions for a bare identifier position.
fn identifier_completions(
    module: &AnalyzedModule,
    file_id: FileId,
    byte_offset: u32,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Lexical locals/params — walk the AST to collect in-scope bindings at cursor
    let locals = collect_locals_at_offset(&module.ast, file_id, byte_offset);
    for (name, _span) in &locals {
        if matches_prefix(name, prefix) && seen.insert(name.clone()) {
            let (kind, detail, documentation) =
                if let Some(sig) = module.typed.value_env.get_function(name) {
                    (
                        CompletionKind::Function,
                        Some(format_sig_detail(sig, &module.typed.type_env)),
                        sig.doc.clone(),
                    )
                } else {
                    (
                        CompletionKind::Variable,
                        module
                            .typed
                            .value_env
                            .lookup(name)
                            .map(|ty| ty.format_with_names(&module.typed.type_env)),
                        None,
                    )
                };
            items.push(CompletionItem {
                label: name.clone(),
                kind,
                detail,
                documentation,
            });
        }
    }

    // 2. Current module top-level functions and values
    for item in &module.ast.items {
        match item {
            Item::Function(decl) => {
                if matches_prefix(&decl.name, prefix) && seen.insert(decl.name.clone()) {
                    let detail = format_function_detail(module, &decl.name);
                    items.push(CompletionItem {
                        label: decl.name.clone(),
                        kind: CompletionKind::Function,
                        detail: detail.clone(),
                        documentation: function_doc(module, &decl.name),
                    });
                }
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, _),
                doc,
                ..
            }) => {
                if matches_prefix(name, prefix) && seen.insert(name.clone()) {
                    let detail = module
                        .typed
                        .value_env
                        .lookup(name)
                        .map(|ty| ty.format_with_names(&module.typed.type_env));
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: CompletionKind::Variable,
                        detail,
                        documentation: doc.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    // 3. Import aliases (for `alias.` completion trigger)
    for import in &module.imports {
        if matches_prefix(&import.alias, prefix) && seen.insert(import.alias.clone()) {
            items.push(CompletionItem {
                label: import.alias.clone(),
                kind: CompletionKind::Module,
                detail: None,
                documentation: None,
            });
        }
    }

    // 4. Builtin functions (user-facing ones only)
    for (name, ty) in module.typed.value_env.all_builtins() {
        if name.starts_with("__") {
            continue; // Skip internal builtins
        }
        if matches_prefix(name, prefix) && seen.insert(name.to_string()) {
            let documentation = module
                .typed
                .value_env
                .get_function(name)
                .and_then(|sig| sig.doc.clone())
                .or_else(|| builtin_value_doc(name).map(str::to_string));
            items.push(CompletionItem {
                label: name.to_string(),
                kind: CompletionKind::Function,
                detail: Some(ty.format_with_names(&module.typed.type_env)),
                documentation,
            });
        }
    }

    // 5. Type names (for constructors, not dotted)
    for (name, type_id) in module.typed.type_env.all_type_names() {
        // Skip internal/qualified names
        if name.contains('.') || name.starts_with("__") {
            continue;
        }
        if matches_prefix(name, prefix) && seen.insert(name.to_string()) {
            let def = module.typed.type_env.get_def(type_id);
            let kind = match def {
                Some(TypeDef::Sum { .. }) => CompletionKind::Struct,
                Some(TypeDef::Record { .. }) => CompletionKind::Struct,
                _ => CompletionKind::Struct,
            };
            items.push(CompletionItem {
                label: name.to_string(),
                kind,
                detail: None,
                documentation: def.and_then(|d| d.doc().map(str::to_string)),
            });
        }
    }

    // 6. Variant constructors from known sum types
    for type_id_idx in 0..module.typed.type_env.type_count() {
        let type_id = crate::types::ty::TypeId(type_id_idx as u32);
        if let Some(TypeDef::Sum { variants, .. }) = module.typed.type_env.get_def(type_id) {
            for variant in variants {
                if matches_prefix(&variant.name, prefix) && seen.insert(variant.name.clone()) {
                    items.push(CompletionItem {
                        label: variant.name.clone(),
                        kind: CompletionKind::EnumMember,
                        detail: None,
                        documentation: None,
                    });
                }
            }
        }
    }

    // 7. Keywords
    let keywords = [
        "if", "else", "case", "for", "in", "fn", "type", "use", "pub", "let", "return", "break",
        "continue", "true", "false", "try", "collect", "defer",
    ];
    for kw in &keywords {
        if matches_prefix(kw, prefix) && seen.insert(kw.to_string()) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: CompletionKind::Keyword,
                detail: None,
                documentation: None,
            });
        }
    }

    sort_completion_items(&mut items);
    items
}

fn sort_completion_items(items: &mut [CompletionItem]) {
    items.sort_by(|a, b| {
        let kind_a = a.kind as u8;
        let kind_b = b.kind as u8;
        a.label
            .cmp(&b.label)
            .then_with(|| kind_a.cmp(&kind_b))
            .then_with(|| a.detail.cmp(&b.detail))
            .then_with(|| a.documentation.cmp(&b.documentation))
    });
}

fn matches_prefix(name: &str, prefix: &str) -> bool {
    prefix.is_empty() || name.starts_with(prefix)
}

fn format_function_detail(module: &AnalyzedModule, name: &str) -> Option<String> {
    // Try qualified name first (module.func), then bare name
    let module_name = module
        .file_registry
        .file_name(module.ast.span.file_id)
        .and_then(|f| std::path::Path::new(f).file_stem())
        .and_then(|s| s.to_str());
    let qualified = module_name.map(|m| format!("{m}.{name}"));
    let sig = qualified
        .as_deref()
        .and_then(|q| module.typed.value_env.get_function(q))
        .or_else(|| module.typed.value_env.get_function(name));
    sig.map(|s| format_sig_detail(s, &module.typed.type_env))
}

fn format_method_detail(module: &AnalyzedModule, qualified_fn: &str) -> Option<String> {
    module
        .typed
        .value_env
        .get_function(qualified_fn)
        .map(|s| format_sig_detail(s, &module.typed.type_env))
}

fn method_doc(module: &AnalyzedModule, qualified_fn: &str) -> Option<String> {
    module
        .typed
        .value_env
        .get_function(qualified_fn)
        .and_then(|s| s.doc.clone())
}

fn function_doc(module: &AnalyzedModule, name: &str) -> Option<String> {
    // Try qualified name first (module.func), then bare name
    let module_name = module
        .file_registry
        .file_name(module.ast.span.file_id)
        .and_then(|f| std::path::Path::new(f).file_stem())
        .and_then(|s| s.to_str());
    let qualified = module_name.map(|m| format!("{m}.{name}"));
    qualified
        .as_deref()
        .and_then(|q| module.typed.value_env.get_function(q))
        .or_else(|| module.typed.value_env.get_function(name))
        .and_then(|s| s.doc.clone())
}

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

fn format_sig_detail(
    sig: &crate::types::ty::FunctionSignature,
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

/// Collect local bindings that are in scope at a given byte offset.
/// This walks the AST, tracking scopes, and returns all bindings visible at the cursor.
fn collect_locals_at_offset(
    ast: &SourceFile,
    file_id: FileId,
    byte_offset: u32,
) -> Vec<(String, crate::syntax::span::Span)> {
    let mut collector = LocalCollector {
        file_id,
        byte_offset,
        scopes: vec![HashMap::new()],
        found: false,
    };

    // Collect top-level function names first
    for item in &ast.items {
        if let Item::Function(decl) = item {
            collector.scopes[0].insert(decl.name.clone(), decl.span);
        }
    }

    for item in &ast.items {
        match item {
            Item::Function(decl) => {
                if decl.span.file_id == file_id && decl.span.contains(byte_offset) {
                    collector.visit_function_decl(decl);
                }
            }
            Item::Stmt(stmt) => collector.visit_stmt(stmt),
            Item::TypeDecl(_) | Item::Import(_) => {}
        }
        if collector.found {
            break;
        }
    }

    // Flatten scopes to a list of bindings
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for scope in collector.scopes.iter().rev() {
        for (name, span) in scope {
            if seen.insert(name.clone()) {
                result.push((name.clone(), *span));
            }
        }
    }
    result
}

struct LocalCollector {
    file_id: FileId,
    byte_offset: u32,
    scopes: Vec<HashMap<String, crate::syntax::span::Span>>,
    found: bool,
}

impl LocalCollector {
    fn visit_function_decl(&mut self, decl: &crate::syntax::ast::FunctionDecl) {
        self.push_scope();
        for param in &decl.params {
            self.bind(&param.name, param.span);
        }
        self.visit_block(&decl.body);
        if !self.found {
            self.pop_scope();
        }
    }

    fn visit_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            // Only process statements that appear before or contain the cursor
            self.visit_stmt(stmt);
            if self.found {
                return;
            }
        }
        self.pop_scope();
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                // If cursor is in the value expression, don't bind the pattern yet
                if value.span.file_id == self.file_id && value.span.contains(self.byte_offset) {
                    self.found = true;
                    return;
                }
                self.bind_pattern(pattern);
                // If cursor is after this let binding, continue
                if let Pattern::Ident(_, span) = pattern {
                    if span.end <= self.byte_offset {
                        return;
                    }
                }
            }
            Stmt::For {
                pattern,
                index_pattern,
                iter,
                body,
                ..
            } => {
                if body.span.file_id == self.file_id && body.span.contains(self.byte_offset) {
                    self.push_scope();
                    self.bind_pattern(pattern);
                    if let Some(idx) = index_pattern {
                        self.bind_pattern(idx);
                    }
                    self.visit_block(body);
                    if !self.found {
                        self.pop_scope();
                    }
                    return;
                }
                self.visit_expr_contains(iter);
            }
            Stmt::ForCond { body, cond, .. } => {
                if body.span.file_id == self.file_id && body.span.contains(self.byte_offset) {
                    self.visit_block(body);
                    return;
                }
                self.visit_expr_contains(cond);
            }
            Stmt::Expr(expr) => self.visit_expr_contains(expr),
            Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.visit_expr_contains(v);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Defer { expr, .. } => self.visit_expr_contains(expr),
        }
    }

    fn visit_expr_contains(&mut self, expr: &Expr) {
        if expr.span.file_id != self.file_id || !expr.span.contains(self.byte_offset) {
            return;
        }
        match &expr.kind {
            ExprKind::Block(block) => self.visit_block(block),
            ExprKind::Function(func) => {
                self.push_scope();
                for param in &func.params {
                    self.bind(&param.name, param.span);
                }
                self.visit_expr_contains(&func.body);
                if !self.found {
                    self.pop_scope();
                }
            }
            ExprKind::Case { arms, .. } => {
                for arm in arms {
                    if arm.body.span.file_id == self.file_id
                        && arm.body.span.contains(self.byte_offset)
                    {
                        self.push_scope();
                        self.bind_pattern(&arm.pattern);
                        self.visit_expr_contains(&arm.body);
                        if !self.found {
                            self.pop_scope();
                        }
                        return;
                    }
                }
            }
            ExprKind::Collect {
                pattern,
                index_pattern,
                body,
                ..
            } => {
                if body.span.file_id == self.file_id && body.span.contains(self.byte_offset) {
                    self.push_scope();
                    self.bind_pattern(pattern);
                    if let Some(idx) = index_pattern {
                        self.bind_pattern(idx);
                    }
                    self.visit_expr_contains(body);
                    if !self.found {
                        self.pop_scope();
                    }
                    return;
                }
            }
            ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.visit_expr_contains(then_branch);
                if self.found {
                    return;
                }
                if let Some(else_expr) = else_branch {
                    self.visit_expr_contains(else_expr);
                }
            }
            _ => {
                // For other expression kinds, just mark found if cursor is within
                self.found = true;
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Ident(name, span) => self.bind(name, *span),
            Pattern::Variant { fields, .. } => {
                for field in fields {
                    self.bind_pattern(field);
                }
            }
            Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
        }
    }

    fn bind(&mut self, name: &str, span: crate::syntax::span::Span) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), span);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

fn find_expr_by_id(ast: &SourceFile, target_id: crate::syntax::ast::ExprId) -> Option<&Expr> {
    for item in &ast.items {
        match item {
            Item::Function(decl) => {
                if let Some(e) = find_in_block(&decl.body, target_id) {
                    return Some(e);
                }
            }
            Item::Stmt(stmt) => {
                if let Some(e) = find_in_stmt(stmt, target_id) {
                    return Some(e);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_in_block(block: &Block, target: crate::syntax::ast::ExprId) -> Option<&Expr> {
    for stmt in &block.stmts {
        if let Some(e) = find_in_stmt(stmt, target) {
            return Some(e);
        }
    }
    None
}

fn find_in_stmt(stmt: &Stmt, target: crate::syntax::ast::ExprId) -> Option<&Expr> {
    match stmt {
        Stmt::Let { value, .. } => find_in_expr(value, target),
        Stmt::For { iter, body, .. } => {
            find_in_expr(iter, target).or_else(|| find_in_block(body, target))
        }
        Stmt::ForCond { cond, body, .. } => {
            find_in_expr(cond, target).or_else(|| find_in_block(body, target))
        }
        Stmt::Expr(expr) => find_in_expr(expr, target),
        Stmt::Break { value, .. } | Stmt::Return { value, .. } => {
            value.as_ref().and_then(|e| find_in_expr(e, target))
        }
        Stmt::Continue { .. } => None,
        Stmt::Defer { expr, .. } => find_in_expr(expr, target),
    }
}

fn find_in_expr(expr: &Expr, target: crate::syntax::ast::ExprId) -> Option<&Expr> {
    if expr.id == target {
        return Some(expr);
    }
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Ident(_) => None,
        ExprKind::Binary { left, right, .. } => {
            find_in_expr(left, target).or_else(|| find_in_expr(right, target))
        }
        ExprKind::Unary { expr, .. } | ExprKind::Try { expr } => find_in_expr(expr, target),
        ExprKind::Call { callee, args } => find_in_expr(callee, target)
            .or_else(|| args.iter().find_map(|a| find_in_expr(a, target))),
        ExprKind::FieldAccess { base, .. } => find_in_expr(base, target),
        ExprKind::Index { base, index } => {
            find_in_expr(base, target).or_else(|| find_in_expr(index, target))
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_in_expr(cond, target)
            .or_else(|| find_in_expr(then_branch, target))
            .or_else(|| else_branch.as_ref().and_then(|e| find_in_expr(e, target))),
        ExprKind::Case { scrutinee, arms } => find_in_expr(scrutinee, target)
            .or_else(|| arms.iter().find_map(|a| find_in_expr(&a.body, target))),
        ExprKind::Block(block) => find_in_block(block, target),
        ExprKind::Array { elements } => elements.iter().find_map(|e| find_in_expr(e, target)),
        ExprKind::RecordLit { fields, .. } => {
            fields.iter().find_map(|(_, v)| find_in_expr(v, target))
        }
        ExprKind::VariantLit { fields, .. } => fields.iter().find_map(|f| find_in_expr(f, target)),
        ExprKind::Function(func) => find_in_expr(&func.body, target),
        ExprKind::Collect { iter, body, .. } => {
            find_in_expr(iter, target).or_else(|| find_in_expr(body, target))
        }
        ExprKind::CollectWhile { cond, body } => {
            find_in_expr(cond, target).or_else(|| find_in_expr(body, target))
        }
        ExprKind::StringInterpolation { parts } => parts.iter().find_map(|p| {
            if let StringPart::Interpolation(e) = p {
                find_in_expr(e, target)
            } else {
                None
            }
        }),
    }
}
