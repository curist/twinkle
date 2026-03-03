use super::ty::{MonoType, zonk_ty};
use crate::ir::FuncId;
use crate::syntax::ast::ExprId;
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

    /// For generic call sites, maps the call expression's ExprId to the
    /// concrete type arguments used at that instantiation. Populated during
    /// type checking; consumed by the monomorphization pass (Stage 9.5).
    generic_instantiations: HashMap<ExprId, Vec<MonoType>>,
}

impl TypeMap {
    /// Create a new empty TypeMap
    pub fn new() -> Self {
        Self {
            expr_types: HashMap::new(),
            method_calls: HashMap::new(),
            generic_instantiations: HashMap::new(),
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

    /// Record a generic instantiation at a call site.
    pub fn set_generic_instantiation(&mut self, expr_id: ExprId, type_args: Vec<MonoType>) {
        self.generic_instantiations.insert(expr_id, type_args);
    }

    /// Get the concrete type args for a generic call site.
    pub fn get_generic_instantiation(&self, expr_id: ExprId) -> Option<&Vec<MonoType>> {
        self.generic_instantiations.get(&expr_id)
    }

    /// Apply meta-variable substitution to all stored expression types.
    /// Called after each function is checked and at the end of module checking.
    pub fn zonk(&mut self, meta_subst: &HashMap<u32, MonoType>) {
        for ty in self.expr_types.values_mut() {
            *ty = zonk_ty(ty, meta_subst);
        }
        for args in self.generic_instantiations.values_mut() {
            for ty in args.iter_mut() {
                *ty = zonk_ty(ty, meta_subst);
            }
        }
    }
}
