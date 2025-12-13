use crate::syntax::ast::ExprId;
use crate::ir::FuncId;
use super::ty::MonoType;
use std::collections::HashMap;

/// Maps expressions to their inferred types and method call resolutions
///
/// This is populated by the type checker during Stage 2 and consumed by
/// the lowering pass in Stage 3.
#[derive(Debug, Clone, Default)]
pub struct TypeMap {
    /// Type of each expression indexed by its ExprId
    expr_types: HashMap<ExprId, MonoType>,

    /// For method calls (receiver.method), maps the call expression's ExprId
    /// to the resolved FuncId of the method being called
    method_calls: HashMap<ExprId, FuncId>,
}

impl TypeMap {
    /// Create a new empty TypeMap
    pub fn new() -> Self {
        Self {
            expr_types: HashMap::new(),
            method_calls: HashMap::new(),
        }
    }

    /// Record the type of an expression
    pub fn set_expr_type(&mut self, expr_id: ExprId, ty: MonoType) {
        self.expr_types.insert(expr_id, ty);
    }

    /// Get the type of an expression
    /// Returns None if the expression has not been type-checked yet
    pub fn get_expr_type(&self, expr_id: ExprId) -> Option<&MonoType> {
        self.expr_types.get(&expr_id)
    }

    /// Record a method call resolution
    /// The expr_id should be the ID of the Call or FieldAccess expression
    pub fn set_method_call(&mut self, expr_id: ExprId, func_id: FuncId) {
        self.method_calls.insert(expr_id, func_id);
    }

    /// Get the FuncId for a method call expression
    /// Returns None if this is not a method call or hasn't been resolved
    pub fn get_method_call(&self, expr_id: ExprId) -> Option<FuncId> {
        self.method_calls.get(&expr_id).copied()
    }
}
