use crate::syntax::span::Span;

/// Unique identifier for expressions (used for type annotation)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// Top-level source file
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub items: Vec<Item>,
    pub span: Span,
}

/// Top-level item (declaration or statement)
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Import(ImportDecl),
    TypeDecl(TypeDecl),
    Function(FunctionDecl),
    Stmt(Stmt),
}

/// Import declaration (use foo.bar [as alias])
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub module_path: Vec<String>, // ["foo", "bar"] from `use foo.bar`
    pub is_stdlib: bool,          // true if @ prefix
    pub alias: Option<String>,    // Some("baz") from `use foo.bar as baz`
    pub span: Span,
}

impl ImportDecl {
    /// Returns the effective module alias name
    /// If `as alias` is present, returns that; otherwise returns the last path segment
    pub fn module_name(&self) -> &str {
        if let Some(ref a) = self.alias {
            a.as_str()
        } else {
            self.module_path.last().map(|s| s.as_str()).unwrap_or("")
        }
    }
}

/// Type declaration
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub is_pub: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub definition: TypeDef,
    pub span: Span,
}

/// Type definition variants
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDef {
    Record { fields: Vec<RecordField> },
    Sum { variants: Vec<Variant> },
    Alias { ty: Type },
}

/// Record field
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

/// Sum type variant
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Type>,
    pub span: Span,
}

/// Function declaration
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub is_pub: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
    pub span: Span,
}

/// Function parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<Type>,
    pub span: Span,
}

//
// Expressions
//

/// Expression with span and unique ID
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub id: ExprId,
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(id: ExprId, kind: ExprKind, span: Span) -> Self {
        Self { id, kind, span }
    }
}

/// Expression variants
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Literal value
    Literal(Literal),

    /// Identifier
    Ident(String),

    /// Binary operation
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    /// Unary operation
    Unary { op: UnOp, expr: Box<Expr> },

    /// Function call
    Call { callee: Box<Expr>, args: Vec<Expr> },

    /// Field access: expr.field
    FieldAccess { base: Box<Expr>, field: String },

    /// Index access: expr[index]
    Index { base: Box<Expr>, index: Box<Expr> },

    /// If expression
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },

    /// Case expression (pattern matching)
    Case {
        scrutinee: Box<Expr>,
        arms: Vec<CaseArm>,
    },

    /// Block expression
    Block(Block),

    /// Array literal: [1, 2, 3]
    Array { elements: Vec<Expr> },

    /// Record literal: .{ x: 1, y: 2 } or Point.{ x: 1, y: 2 }
    RecordLit {
        name: Option<String>,
        fields: Vec<(String, Expr)>,
    },

    /// Variant literal: .Some(42) or .None
    VariantLit { name: String, fields: Vec<Expr> },

    /// Function expression: fn(x) { x + 1 }
    Function(FunctionExpr),

    /// Collect expression: collect x in xs { x * 2 }
    Collect {
        pattern: Pattern,
        index_pattern: Option<Pattern>,
        iter: Box<Expr>,
        body: Box<Expr>,
    },

    /// Collect-while expression: collect cond { expr }
    CollectWhile {
        cond: Box<Expr>,
        body: Box<Expr>,
    },

    /// Try expression: try expr
    Try { expr: Box<Expr> },

    /// String with interpolation
    StringInterpolation { parts: Vec<StringPart> },
}

/// String part (literal or interpolation)
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Interpolation(Box<Expr>),
}

/// Literal values
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,

    // Assignment
    Assign,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg, // -
    Not, // !
}

/// Case arm
#[derive(Debug, Clone, PartialEq)]
pub struct CaseArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// Function expression
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionExpr {
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Box<Expr>,
    pub span: Span,
}

//
// Statements
//

/// Statement
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Let binding: x := expr or x: Type = expr
    Let {
        pattern: Pattern,
        ty: Option<Type>,
        value: Expr,
        is_pub: bool,
        span: Span,
    },

    /// For loop: for pattern in iter { body }
    For {
        pattern: Pattern,
        index_pattern: Option<Pattern>,
        iter: Expr,
        body: Block,
        span: Span,
    },

    /// For loop with condition: for cond { body }
    ForCond { cond: Expr, body: Block, span: Span },

    /// Expression statement
    Expr(Expr),

    /// Break statement
    Break { value: Option<Expr>, span: Span },

    /// Continue statement
    Continue { span: Span },

    /// Return statement
    Return { value: Option<Expr>, span: Span },

    /// Defer statement: schedules `expr` to run when the enclosing scope exits.
    /// Any expression type is accepted except `Never` (diverging expressions).
    Defer { expr: Expr, span: Span },
}

/// Block of statements
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

//
// Patterns
//

/// Pattern for destructuring
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Wildcard: _
    Wildcard(Span),

    /// Identifier binding: x
    Ident(String, Span),

    /// Literal pattern: 42, "hello", true
    Literal(Literal, Span),

    /// Variant pattern: .Some(x) or qualified ParseError.InvalidFormat(s)
    Variant {
        /// Present for the qualified `TypeName.Variant` form; None for the anonymous `.Variant` form.
        type_name: Option<String>,
        name: String,
        fields: Vec<Pattern>,
        span: Span,
    },
}

//
// Types
//

/// Type expression
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Named type: Int, Point, Array<Int>
    Named {
        name: String,
        args: Vec<Type>,
        span: Span,
    },

    /// Function type: fn(Int, String) Bool
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        span: Span,
    },
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named { span, .. } => *span,
            Type::Function { span, .. } => *span,
        }
    }
}
