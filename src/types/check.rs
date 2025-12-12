use crate::syntax::ast::{
    BinOp, Block, Expr, ExprKind, FunctionDecl, Item, Literal, Pattern, SourceFile, Stmt,
    StringPart, Type as AstType, UnOp,
};
use crate::syntax::span::Span;
use super::env::{LocalEnv, TypeEnv, ValueEnv};
use super::error::TypeError;
use super::patterns::PatternChecker;
use super::ty::MonoType;

/// Bidirectional type checker
///
/// Uses synthesis mode (infer type) and checking mode (validate against expected type)
pub struct TypeChecker {
    type_env: TypeEnv,
    value_env: ValueEnv,
    local_env: LocalEnv,
    errors: Vec<TypeError>,

    // Track current function's return type for return statement checking
    current_function_ret: Option<MonoType>,
}

impl TypeChecker {
    /// Type-check a complete module (source file)
    /// Returns Ok(()) if type checking succeeds, or a list of errors
    pub fn check_module(
        ast: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
    ) -> Result<(), Vec<TypeError>> {
        let mut checker = TypeChecker {
            type_env,
            value_env,
            local_env: LocalEnv::new(),
            errors: Vec::new(),
            current_function_ret: None,
        };

        // Pass 1: Check all top-level lets and add to ValueEnv
        // This makes them available to all functions
        for item in &ast.items {
            if let Item::Stmt(stmt) = item {
                if let Stmt::Let { pattern, ty, value, span } = stmt {
                    // Only simple identifier patterns for top-level lets
                    if let Pattern::Ident(name, _) = pattern {
                        // Determine the expected type
                        let value_ty = if let Some(ann_ty) = ty {
                            // Type annotation provided - check mode
                            let expected = match checker.resolve_type(ann_ty) {
                                Ok(t) => t,
                                Err(()) => continue, // Error already recorded
                            };
                            match checker.check_expr(value, &expected) {
                                Ok(()) => expected,
                                Err(()) => continue, // Error already recorded
                            }
                        } else {
                            // No annotation - synthesis mode
                            match checker.synth_expr(value) {
                                Ok(t) => t,
                                Err(()) => continue, // Error already recorded
                            }
                        };

                        // Add to ValueEnv so it's accessible from functions
                        checker.value_env.add_value(name.clone(), value_ty);
                    } else {
                        checker.errors.push(TypeError::UnsupportedFeature {
                            feature: "pattern matching in top-level let bindings",
                            span: *span,
                            note: "Only simple identifiers are supported for top-level lets".to_string(),
                        });
                    }
                } else {
                    // Non-let statements at top-level
                    checker.errors.push(TypeError::InvalidTopLevelItem {
                        span: checker.stmt_span(stmt),
                        note: "Only let bindings are allowed at top-level".to_string(),
                    });
                }
            }
        }

        // Pass 2: Type-check all functions
        // Functions can now reference top-level lets
        for item in &ast.items {
            match item {
                Item::TypeDecl(_) => {
                    // Type declarations were already processed by the resolver
                }
                Item::Function(decl) => {
                    checker.check_function(decl);
                }
                Item::Stmt(_) => {
                    // Already checked in Pass 1
                }
                Item::Import(_) => {
                    // Imports were already rejected by the resolver
                }
            }
        }

        if checker.errors.is_empty() {
            Ok(())
        } else {
            Err(checker.errors)
        }
    }

    //
    // Top-level checking
    //

    fn check_function(&mut self, decl: &FunctionDecl) {
        // Push a new scope for the function body
        self.local_env.push_scope();

        // Get the function signature from ValueEnv (clone to avoid borrowing issues)
        let sig = match self.value_env.get_function(&decl.name) {
            Some(s) => s.clone(),
            None => {
                // Should not happen - resolver should have caught this
                self.errors.push(TypeError::UndefinedVariable {
                    name: decl.name.clone(),
                    span: decl.span,
                });
                self.local_env.pop_scope();
                return;
            }
        };

        // Bind parameters in local environment
        for (param, param_ty) in decl.params.iter().zip(sig.params.iter()) {
            self.local_env.bind(param.name.clone(), param_ty.clone());
        }

        // Set current function return type
        self.current_function_ret = sig.ret.clone();

        // Type-check the function body
        // The body is a Block, which should evaluate to the return type
        if let Some(expected_ret) = &sig.ret {
            // Explicit return type - check body against it
            let body_ty = self.synth_block(&decl.body);
            if let Ok(ty) = body_ty {
                if self.unify(&ty, expected_ret, decl.body.span).is_err() {
                    // Error already recorded in unify
                }
            }
        } else {
            // No explicit return type - infer from body
            match self.synth_block(&decl.body) {
                Ok(body_ty) => {
                    // Update the function signature with the inferred return type
                    let mut updated_sig = sig.clone();
                    updated_sig.ret = Some(body_ty);
                    self.value_env.update_function(updated_sig);
                }
                Err(()) => {
                    // Type checking failed, can't infer return type
                }
            }
        }

        // Clean up
        self.current_function_ret = None;
        self.local_env.pop_scope();
    }

    fn stmt_span(&self, stmt: &Stmt) -> Span {
        match stmt {
            Stmt::Let { span, .. } => *span,
            Stmt::For { span, .. } => *span,
            Stmt::ForCond { span, .. } => *span,
            Stmt::Expr(e) => e.span,
            Stmt::Break { span, .. } => *span,
            Stmt::Continue { span } => *span,
            Stmt::Return { span, .. } => *span,
        }
    }

    /// Resolve an AST type annotation to a MonoType
    /// Delegates to TypeEnv's shared implementation
    fn resolve_type(&mut self, ty: &AstType) -> Result<MonoType, ()> {
        self.type_env.resolve_type(ty, &mut self.errors)
    }

    //
    // Synthesis mode: infer type of expression
    //

    fn synth_expr(&mut self, expr: &Expr) -> Result<MonoType, ()> {
        match &expr.kind {
            ExprKind::Literal(lit) => Ok(self.synth_literal(lit)),

            ExprKind::Ident(name) => {
                // Look up in local environment first, then value environment
                if let Some(ty) = self.local_env.lookup(name) {
                    Ok(ty.clone())
                } else if let Some(ty) = self.value_env.lookup(name) {
                    Ok(ty)
                } else {
                    self.errors.push(TypeError::UndefinedVariable {
                        name: name.clone(),
                        span: expr.span,
                    });
                    Err(())
                }
            }

            ExprKind::Binary { op, left, right } => {
                self.synth_binary(*op, left, right, expr.span)
            }

            ExprKind::Unary { op, expr: inner } => {
                self.synth_unary(*op, inner, expr.span)
            }

            ExprKind::Call { callee, args } => {
                self.synth_call(callee, args, expr.span)
            }

            ExprKind::Block(block) => self.synth_block(block),

            ExprKind::If { cond, then_branch, else_branch } => {
                self.synth_if(cond, then_branch, else_branch.as_deref(), expr.span)
            }

            ExprKind::FieldAccess { base, field } => {
                self.synth_field_access(base, field, expr.span)
            }

            ExprKind::Index { base, index } => {
                self.synth_index(base, index, expr.span)
            }

            ExprKind::Array { elements } => {
                self.synth_array(elements, expr.span)
            }

            ExprKind::RecordLit { name, fields } => {
                self.synth_record_lit(name.as_deref(), fields, expr.span)
            }

            ExprKind::VariantLit { name, fields } => {
                self.synth_variant_lit(name, fields, expr.span)
            }

            ExprKind::Case { scrutinee, arms } => {
                self.synth_case(scrutinee, arms, expr.span)
            }

            ExprKind::StringInterpolation { parts } => {
                // Type-check each interpolated expression
                for part in parts {
                    if let StringPart::Interpolation(e) = part {
                        // Any type is ok, will be stringified
                        let _ = self.synth_expr(e);
                    }
                }
                Ok(MonoType::String)
            }

            // Unsupported features
            ExprKind::Function(_) => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "lambda expressions",
                    span: expr.span,
                    note: "Lambda expressions will be supported in later phases".to_string(),
                });
                Err(())
            }

            ExprKind::Collect { .. } => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "collect expressions",
                    span: expr.span,
                    note: "Collect will be supported after IR lowering in Stage 3".to_string(),
                });
                Err(())
            }

            ExprKind::Try { .. } => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "try expressions",
                    span: expr.span,
                    note: "Try will be supported after IR lowering in Stage 3".to_string(),
                });
                Err(())
            }
        }
    }

    //
    // Checking mode: validate expression against expected type
    //

    fn check_expr(&mut self, expr: &Expr, expected: &MonoType) -> Result<(), ()> {
        match &expr.kind {
            // Anonymous record literals REQUIRE checking mode
            ExprKind::RecordLit { name: None, fields } => {
                self.check_anon_record_lit(fields, expected, expr.span)
            }

            // Variant literals can be checked against expected sum type
            ExprKind::VariantLit { name, fields } => {
                self.check_variant_lit(name, fields, expected, expr.span)
            }

            // For most expressions, synthesize and unify
            _ => {
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }
        }
    }

    //
    // Literal synthesis
    //

    fn synth_literal(&self, lit: &Literal) -> MonoType {
        match lit {
            Literal::Int(_) => MonoType::Int,
            Literal::Float(_) => MonoType::Float,
            Literal::Bool(_) => MonoType::Bool,
            Literal::String(_) => MonoType::String,
        }
    }

    //
    // Binary operators
    //

    fn synth_binary(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Result<MonoType, ()> {
        match op {
            // Arithmetic: Int × Int → Int, Float × Float → Float
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let left_ty = self.synth_expr(left)?;
                let right_ty = self.synth_expr(right)?;

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Int) => Ok(MonoType::Int),
                    (MonoType::Float, MonoType::Float) => Ok(MonoType::Float),
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: left_ty.clone(),
                            actual: right_ty,
                            span: right.span,
                        });
                        Err(())
                    }
                }
            }

            // Comparison: T × T → Bool (for primitive types)
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let left_ty = self.synth_expr(left)?;
                let right_ty = self.synth_expr(right)?;

                self.unify(&left_ty, &right_ty, right.span)?;
                Ok(MonoType::Bool)
            }

            // Logical: Bool × Bool → Bool
            BinOp::And | BinOp::Or => {
                self.check_expr(left, &MonoType::Bool)?;
                self.check_expr(right, &MonoType::Bool)?;
                Ok(MonoType::Bool)
            }

            // Assignment operators - not allowed in Stage 2
            BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign
            | BinOp::MulAssign | BinOp::DivAssign | BinOp::ModAssign => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "assignment operators",
                    span,
                    note: "Twinkle uses immutable bindings only in Stage 2".to_string(),
                });
                Err(())
            }
        }
    }

    //
    // Unary operators
    //

    fn synth_unary(&mut self, op: UnOp, expr: &Expr, _span: Span) -> Result<MonoType, ()> {
        match op {
            UnOp::Neg => {
                let ty = self.synth_expr(expr)?;
                match &ty {
                    MonoType::Int => Ok(MonoType::Int),
                    MonoType::Float => Ok(MonoType::Float),
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Int,
                            actual: ty,
                            span: expr.span,
                        });
                        Err(())
                    }
                }
            }
            UnOp::Not => {
                self.check_expr(expr, &MonoType::Bool)?;
                Ok(MonoType::Bool)
            }
        }
    }

    //
    // Function calls
    //

    fn synth_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        // Special case: len() builtin
        if let ExprKind::Ident(name) = &callee.kind {
            if name == "len" {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }

                let arg_ty = self.synth_expr(&args[0])?;
                match &arg_ty {
                    MonoType::String | MonoType::Array(_) => return Ok(MonoType::Int),
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::String,
                            actual: arg_ty,
                            span: args[0].span,
                        });
                        return Err(());
                    }
                }
            }
        }

        // Normal function call
        let callee_ty = self.synth_expr(callee)?;

        match callee_ty {
            MonoType::Function { params, ret } => {
                // Check arity
                if params.len() != args.len() {
                    self.errors.push(TypeError::WrongArity {
                        expected: params.len(),
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }

                // Check each argument
                for (arg, expected_ty) in args.iter().zip(params.iter()) {
                    self.check_expr(arg, expected_ty)?;
                }

                Ok(*ret)
            }
            _ => {
                self.errors.push(TypeError::NotAFunction {
                    ty: callee_ty,
                    span: callee.span,
                });
                Err(())
            }
        }
    }

    //
    // Blocks
    //

    fn synth_block(&mut self, block: &Block) -> Result<MonoType, ()> {
        self.local_env.push_scope();

        let mut result_ty = MonoType::Void;

        for stmt in &block.stmts {
            match stmt {
                Stmt::Let { pattern, ty, value, span: _ } => {
                    self.check_let_stmt(pattern, ty.as_ref(), value);
                }
                Stmt::Expr(e) => {
                    // Expression statement
                    // If it's the last statement, its type becomes the block's type
                    result_ty = self.synth_expr(e)?;
                }
                Stmt::Return { value, span } => {
                    if let Some(ret_ty) = self.current_function_ret.clone() {
                        if let Some(val) = value {
                            self.check_expr(val, &ret_ty)?;
                        } else {
                            self.unify(&MonoType::Void, &ret_ty, *span)?;
                        }
                    } else {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "return outside function",
                            span: *span,
                            note: "Return statements are only allowed inside functions".to_string(),
                        });
                    }
                    result_ty = MonoType::Void;
                }
                Stmt::For { .. } | Stmt::ForCond { .. } => {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "for loops",
                        span: self.stmt_span(stmt),
                        note: "For loops will be supported in later phases".to_string(),
                    });
                    result_ty = MonoType::Void;
                }
                Stmt::Break { .. } | Stmt::Continue { .. } => {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "break/continue",
                        span: self.stmt_span(stmt),
                        note: "Break/continue will be supported with for loops".to_string(),
                    });
                    result_ty = MonoType::Void;
                }
            }
        }

        self.local_env.pop_scope();
        Ok(result_ty)
    }

    //
    // Let statements
    //

    fn check_let_stmt(&mut self, pattern: &Pattern, ty: Option<&crate::syntax::ast::Type>, value: &Expr) {
        // For now, only support simple identifier patterns
        match pattern {
            Pattern::Ident(name, _span) => {
                // Determine the expected type
                let value_ty = if let Some(ann_ty) = ty {
                    // Type annotation provided - check mode
                    let expected = match self.resolve_type(ann_ty) {
                        Ok(t) => t,
                        Err(()) => return, // Error already recorded
                    };
                    match self.check_expr(value, &expected) {
                        Ok(()) => expected,
                        Err(()) => return, // Error already recorded
                    }
                } else {
                    // No annotation - synthesis mode
                    match self.synth_expr(value) {
                        Ok(t) => t,
                        Err(()) => return,
                    }
                };

                // Bind the variable
                self.local_env.bind(name.clone(), value_ty);
            }
            Pattern::Wildcard(_) => {
                // Just evaluate the value for side effects
                let _ = self.synth_expr(value);
            }
            Pattern::Variant { .. } | Pattern::Literal(..) => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "pattern matching in let bindings",
                    span: value.span,
                    note: "Only simple identifiers are supported in let bindings for now".to_string(),
                });
            }
        }
    }

    //
    // If expressions
    //

    fn synth_if(&mut self, cond: &Expr, then_branch: &Expr, else_branch: Option<&Expr>, _span: Span) -> Result<MonoType, ()> {
        // Condition must be Bool
        self.check_expr(cond, &MonoType::Bool)?;

        // Synthesize then branch type
        let then_ty = self.synth_expr(then_branch)?;

        // If there's an else branch, both branches must have the same type
        if let Some(else_expr) = else_branch {
            let else_ty = self.synth_expr(else_expr)?;
            self.unify(&then_ty, &else_ty, else_expr.span)?;
            Ok(then_ty)
        } else {
            // No else branch - result type is Void
            self.unify(&then_ty, &MonoType::Void, then_branch.span)?;
            Ok(MonoType::Void)
        }
    }

    //
    // Field access
    //

    fn synth_field_access(&mut self, base: &Expr, field: &str, span: Span) -> Result<MonoType, ()> {
        let base_ty = self.synth_expr(base)?;

        match base_ty {
            MonoType::Named { type_id, .. } => {
                // Look up the record fields
                if let Some(fields) = self.type_env.get_record_fields(type_id) {
                    // Find the field
                    for f in fields {
                        if f.name == field {
                            return Ok(f.ty.clone());
                        }
                    }

                    // Field not found
                    let record_name = self.type_env.get_def(type_id)
                        .map(|d| d.name().to_string())
                        .unwrap_or_else(|| format!("Type#{}", type_id.0));

                    self.errors.push(TypeError::NoSuchField {
                        record_type: record_name,
                        field: field.to_string(),
                        span,
                    });
                    Err(())
                } else {
                    // Not a record type
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Int, // Dummy
                        actual: base_ty,
                        span: base.span,
                    });
                    Err(())
                }
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Int, // Dummy
                    actual: base_ty,
                    span: base.span,
                });
                Err(())
            }
        }
    }

    //
    // Array indexing
    //

    fn synth_index(&mut self, base: &Expr, index: &Expr, _span: Span) -> Result<MonoType, ()> {
        let base_ty = self.synth_expr(base)?;

        // Index must be Int
        self.check_expr(index, &MonoType::Int)?;

        match base_ty {
            MonoType::Array(elem_ty) => Ok(*elem_ty),
            MonoType::String => Ok(MonoType::String), // String indexing returns String (single char)
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Array(Box::new(MonoType::Int)), // Dummy
                    actual: base_ty,
                    span: base.span,
                });
                Err(())
            }
        }
    }

    //
    // Array literals
    //

    fn synth_array(&mut self, elements: &[Expr], span: Span) -> Result<MonoType, ()> {
        if elements.is_empty() {
            // Empty array - we can't infer the type
            // For now, error - require type annotation
            self.errors.push(TypeError::UnsupportedFeature {
                feature: "empty array literals",
                span,
                note: "Empty arrays require type annotations (not yet supported)".to_string(),
            });
            return Err(());
        }

        // Infer type from first element
        let first_ty = self.synth_expr(&elements[0])?;

        // Check all other elements match
        for elem in &elements[1..] {
            self.check_expr(elem, &first_ty)?;
        }

        Ok(MonoType::Array(Box::new(first_ty)))
    }

    //
    // Record literals
    //

    fn synth_record_lit(&mut self, name: Option<&str>, fields: &[(String, Expr)], span: Span) -> Result<MonoType, ()> {
        if let Some(type_name) = name {
            // Named record literal: Point.{ x: 1, y: 2 }
            let type_id = match self.type_env.lookup_type(type_name) {
                Some(id) => id,
                None => {
                    self.errors.push(TypeError::UndefinedType {
                        name: type_name.to_string(),
                        span,
                    });
                    return Err(());
                }
            };

            let expected_ty = MonoType::named(type_id);
            self.check_record_lit_fields(type_id, fields, span)?;
            Ok(expected_ty)
        } else {
            // Anonymous record literal: .{ x: 1, y: 2 }
            // This requires expected type from context - error in synthesis mode
            self.errors.push(TypeError::AnonymousRecordWithoutContext { span });
            Err(())
        }
    }

    fn check_anon_record_lit(&mut self, fields: &[(String, Expr)], expected: &MonoType, span: Span) -> Result<(), ()> {
        match expected {
            MonoType::Named { type_id, .. } => {
                self.check_record_lit_fields(*type_id, fields, span)
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: expected.clone(),
                    actual: MonoType::Void, // Dummy
                    span,
                });
                Err(())
            }
        }
    }

    fn check_record_lit_fields(&mut self, type_id: crate::types::ty::TypeId, fields: &[(String, Expr)], span: Span) -> Result<(), ()> {
        let expected_fields = match self.type_env.get_record_fields(type_id) {
            Some(f) => f,
            None => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::named(type_id),
                    actual: MonoType::Void, // Dummy
                    span,
                });
                return Err(());
            }
        };

        // Check all expected fields are present and have correct types
        // Clone the fields to avoid borrowing issues
        let expected_fields_vec: Vec<_> = expected_fields.iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();

        let expected_names: Vec<String> = expected_fields_vec.iter()
            .map(|(name, _)| name.clone())
            .collect();

        for (field_name, field_ty) in &expected_fields_vec {
            let provided = fields.iter().find(|(name, _)| name == field_name);

            if let Some((_, value)) = provided {
                self.check_expr(value, field_ty)?;
            } else {
                // Missing field
                let record_name = self.type_env.get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                self.errors.push(TypeError::NoSuchField {
                    record_type: record_name,
                    field: field_name.clone(),
                    span,
                });
                return Err(());
            }
        }

        // Check for extra fields

        for (provided_name, _) in fields {
            if !expected_names.contains(provided_name) {
                let record_name = self.type_env.get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                self.errors.push(TypeError::NoSuchField {
                    record_type: record_name,
                    field: provided_name.clone(),
                    span,
                });
                return Err(());
            }
        }

        Ok(())
    }

    //
    // Variant literals
    //

    fn synth_variant_lit(&mut self, variant_name: &str, _fields: &[Expr], span: Span) -> Result<MonoType, ()> {
        // Variant literals require type context to disambiguate which sum type they belong to
        // Multiple sum types may have variants with the same name
        // Use checking mode with type annotation: `x: Option<Int> = .Some(42)`

        self.errors.push(TypeError::UnsupportedFeature {
            feature: "variant literals without type context",
            span,
            note: format!("Cannot infer type for variant .{}(...) - provide a type annotation", variant_name),
        });
        Err(())
    }

    fn check_variant_lit(&mut self, variant_name: &str, fields: &[Expr], expected: &MonoType, span: Span) -> Result<(), ()> {
        // Expected type must be a sum type
        match expected {
            MonoType::Named { type_id, .. } => {
                // Get the variants for this sum type
                let variants = match self.type_env.get_variants(*type_id) {
                    Some(v) => v,
                    None => {
                        // Not a sum type
                        self.errors.push(TypeError::TypeMismatch {
                            expected: expected.clone(),
                            actual: MonoType::Void, // Placeholder
                            span,
                        });
                        return Err(());
                    }
                };

                // Find the variant with the matching name
                let variant = variants.iter().find(|v| &v.name == variant_name);

                match variant {
                    Some(v) => {
                        // Check arity
                        if v.fields.len() != fields.len() {
                            self.errors.push(TypeError::WrongArity {
                                expected: v.fields.len(),
                                actual: fields.len(),
                                span,
                            });
                            return Err(());
                        }

                        // Clone field types before checking to avoid borrowing issues
                        let field_types: Vec<MonoType> = v.fields.clone();

                        // Check each field
                        for (field_expr, field_ty) in fields.iter().zip(field_types.iter()) {
                            self.check_expr(field_expr, field_ty)?;
                        }

                        Ok(())
                    }
                    None => {
                        // Variant not found in this sum type
                        let sum_type_name = self.type_env
                            .get_def(*type_id)
                            .map(|d| d.name().to_string())
                            .unwrap_or_else(|| format!("Type#{}", type_id.0));

                        self.errors.push(TypeError::NoSuchVariant {
                            sum_type: sum_type_name,
                            variant: variant_name.to_string(),
                            span,
                        });
                        Err(())
                    }
                }
            }
            _ => {
                // Expected type is not a sum type
                self.errors.push(TypeError::TypeMismatch {
                    expected: expected.clone(),
                    actual: MonoType::Void, // Placeholder - we don't know the actual type
                    span,
                });
                Err(())
            }
        }
    }

    //
    // Case expressions
    //

    fn synth_case(&mut self, scrutinee: &Expr, arms: &[crate::syntax::ast::CaseArm], span: Span) -> Result<MonoType, ()> {
        let scrut_ty = self.synth_expr(scrutinee)?;

        // Scrutinee must be a sum type
        if !scrut_ty.is_sum(&self.type_env) {
            self.errors.push(TypeError::CaseScrutineeNotSumType {
                actual_type: scrut_ty.clone(),
                span: scrutinee.span,
            });
            return Err(());
        }

        if arms.is_empty() {
            self.errors.push(TypeError::NonExhaustiveMatch {
                missing: vec!["(all patterns)".to_string()],
                span,
            });
            return Err(());
        }

        // Check exhaustiveness
        PatternChecker::check_exhaustiveness(
            &self.type_env,
            &mut self.errors,
            &scrut_ty,
            arms,
            span,
        )?;

        // Type-check first arm to get result type
        let result_ty = self.synth_case_arm(&arms[0], &scrut_ty)?;

        // Check all other arms match
        for arm in &arms[1..] {
            let arm_ty = self.synth_case_arm(arm, &scrut_ty)?;
            self.unify(&arm_ty, &result_ty, arm.span)?;
        }

        Ok(result_ty)
    }

    fn synth_case_arm(&mut self, arm: &crate::syntax::ast::CaseArm, scrut_ty: &MonoType) -> Result<MonoType, ()> {
        // Push a new scope for pattern bindings
        self.local_env.push_scope();

        // Check the pattern and bind variables
        let mut pattern_checker =
            PatternChecker::new(&self.type_env, &mut self.local_env, &mut self.errors);
        pattern_checker.check_pattern(&arm.pattern, scrut_ty)?;

        // Type-check the arm body
        let body_ty = self.synth_expr(&arm.body)?;

        self.local_env.pop_scope();
        Ok(body_ty)
    }

    //
    // Unification
    //

    fn unify(&mut self, actual: &MonoType, expected: &MonoType, span: Span) -> Result<(), ()> {
        if actual == expected {
            Ok(())
        } else {
            self.errors.push(TypeError::TypeMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
                span,
            });
            Err(())
        }
    }
}
