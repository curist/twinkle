use crate::syntax::ast::{BinOp, UnOp};
use crate::syntax::span::Span;
use crate::types::env::TypeEnv;
use crate::types::ty::{MonoType, TypeId};
// TODO: Add serde dependency when implementing JSON serialization
// use serde::{Deserialize, Serialize};

/// Unique identifier for a local variable within a function
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Unique identifier for a function in the module
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// Unique identifier for a field in a record type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(pub usize);

/// Unique identifier for a variant in a sum type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariantId(pub usize);

/// Core IR expression - all expressions produce a value
#[derive(Debug, Clone, PartialEq)]
pub struct CoreExpr {
    pub kind: CoreExprKind,
    pub ty: MonoType,
    pub span: Span,
}

/// Core IR expression variants
#[derive(Debug, Clone, PartialEq)]
pub enum CoreExprKind {
    // Literals
    LitInt(i64),
    LitFloat(f64),
    LitBool(bool),
    LitStr(String),
    LitVoid,

    // Variables
    Local(LocalId),
    GlobalFunc(FuncId),

    // Binding (introduces a new variable; purely functional)
    Let {
        local: LocalId,
        value: Box<CoreExpr>,
        body: Box<CoreExpr>,
    },

    // Mutation (updates an existing variable; maps to Wasm local.set)
    // Used for rebinding inside loops and explicit `x = expr` rebinding.
    Assign {
        local: LocalId,
        value: Box<CoreExpr>,
    },

    // Binary operation
    BinOp {
        op: BinOp,
        left: Box<CoreExpr>,
        right: Box<CoreExpr>,
    },

    // Unary operation
    UnOp {
        op: UnOp,
        expr: Box<CoreExpr>,
    },

    // Function call
    Call {
        callee: Box<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    // Lambda/closure — hoisted to a FunctionDef at the top level; this node
    // captures the free variables by value at the point of creation.
    MakeClosure {
        func_id: FuncId,
        free_vars: Vec<LocalId>,
    },

    // Control flow
    // Note: Inherent method calls are NOT represented as a special node.
    // They lower to ordinary Call { callee: GlobalFunc(method_func_id), args: [receiver, ...] }
    // See Stage 3 plan for details.
    If {
        cond: Box<CoreExpr>,
        then_branch: Box<CoreExpr>,
        else_branch: Box<CoreExpr>,
    },

    Match {
        scrutinee: Box<CoreExpr>,
        arms: Vec<MatchArm>,
    },

    Loop {
        body: Box<CoreExpr>,
    },

    Break {
        value: Option<Box<CoreExpr>>,
    },

    Continue,

    Return {
        value: Option<Box<CoreExpr>>,
    },

    // Data structures
    Record {
        type_id: TypeId,
        fields: Vec<(FieldId, CoreExpr)>,
    },

    RecordGet {
        target: Box<CoreExpr>,
        field: FieldId,
    },

    Variant {
        type_id: TypeId,
        variant: VariantId,
        args: Vec<CoreExpr>,
    },

    ArrayLit {
        elements: Vec<CoreExpr>,
    },

    Index {
        base: Box<CoreExpr>,
        index: Box<CoreExpr>,
    },

    /// Functional record update: produces a new record with one field replaced.
    /// Semantics: new_record = { ...base, field: value }
    /// A future optimization pass may lower this to struct.set when provably safe.
    RecordUpdate {
        base: Box<CoreExpr>,
        field: FieldId,
        value: Box<CoreExpr>,
    },
}

/// Match arm in Core IR
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: CorePattern,
    pub body: CoreExpr,
}

/// Pattern in Core IR - fully resolved, no name ambiguity
#[derive(Debug, Clone, PartialEq)]
pub enum CorePattern {
    Wildcard,
    Var(LocalId),
    LitInt(i64),
    LitBool(bool),
    LitStr(String),
    Variant {
        type_id: TypeId,
        variant: VariantId,
        fields: Vec<CorePattern>,
    },
}

/// Function definition in Core IR
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub func_id: FuncId,
    pub name: String,
    pub params: Vec<LocalId>,
    pub body: CoreExpr,
    pub return_ty: MonoType,
}

/// Module in Core IR
#[derive(Debug, Clone)]
pub struct CoreModule {
    pub functions: Vec<FunctionDef>,
    pub type_env: TypeEnv,
    pub init_func_id: Option<FuncId>,
}
