use std::fmt;

// Note on module dependencies:
// ty.rs defines core types (MonoType, TypeDef, etc.)
// env.rs uses ty.rs types for its environments
// Some MonoType methods (is_sum, format_with_names) require TypeEnv for lookups
//
// This creates a controlled dependency: ty → env for helper methods only.
// The core type definitions remain independent. If this becomes problematic,
// we could move these helpers to free functions in env.rs or a separate module.

/// Unique identifier for a user-defined type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

/// Monomorphic type representation
/// Stage 2: No generics - all type parameters must be monomorphic
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MonoType {
    /// Integer type (i64)
    Int,
    /// Floating point type (f64)
    Float,
    /// Boolean type
    Bool,
    /// String type (immutable, GC-managed)
    String,
    /// Void/unit type
    Void,

    /// User-defined nominal type (record or sum type)
    /// args is empty in Stage 2 but prepared for Stage 5 generics
    Named {
        type_id: TypeId,
        args: Vec<MonoType>,
    },

    /// Array type (GC-managed, immutable)
    Array(Box<MonoType>),

    /// Dict type (GC-managed, persistent hash map)
    Dict(Box<MonoType>, Box<MonoType>),

    /// Function type
    Function {
        params: Vec<MonoType>,
        ret: Box<MonoType>,
    },
}

impl MonoType {
    /// Create a named type with no type arguments
    pub fn named(type_id: TypeId) -> Self {
        MonoType::Named {
            type_id,
            args: vec![],
        }
    }

    /// Check if this is a sum type by looking up the TypeDef
    /// This follows type aliases to their targets (e.g., type MySum = Result)
    /// This is needed for case expression validation to distinguish sum types from records
    ///
    /// Note: The resolver should prevent circular aliases, but we rely on that here.
    /// If a circular alias exists, this will recurse until stack overflow.
    pub fn is_sum(&self, type_env: &crate::types::env::TypeEnv) -> bool {
        match self {
            MonoType::Named { type_id, .. } => {
                if let Some(def) = type_env.get_def(*type_id) {
                    match def {
                        crate::types::ty::TypeDef::Sum { .. } => true,
                        crate::types::ty::TypeDef::Record { .. } => false,
                        // Follow aliases to their target recursively
                        // The resolver must ensure no circular aliases exist
                        crate::types::ty::TypeDef::Alias { target, .. } => {
                            target.is_sum(type_env)
                        }
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Format the type with readable names using a TypeEnv
    /// This is used for error messages to show "Point" instead of "Type#0"
    pub fn format_with_names(&self, type_env: &crate::types::env::TypeEnv) -> String {
        match self {
            MonoType::Int => "Int".to_string(),
            MonoType::Float => "Float".to_string(),
            MonoType::Bool => "Bool".to_string(),
            MonoType::String => "String".to_string(),
            MonoType::Void => "Void".to_string(),
            MonoType::Named { type_id, args } => {
                if let Some(def) = type_env.get_def(*type_id) {
                    let name = def.name();
                    if args.is_empty() {
                        name.to_string()
                    } else {
                        let args_str = args
                            .iter()
                            .map(|arg| arg.format_with_names(type_env))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}<{}>", name, args_str)
                    }
                } else {
                    // Fallback if TypeId not found
                    format!("Type#{}", type_id.0)
                }
            }
            MonoType::Array(elem_ty) => {
                format!("Array<{}>", elem_ty.format_with_names(type_env))
            }
            MonoType::Dict(k, v) => {
                format!("Dict<{}, {}>", k.format_with_names(type_env), v.format_with_names(type_env))
            }
            MonoType::Function { params, ret } => {
                let params_str = params
                    .iter()
                    .map(|p| p.format_with_names(type_env))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}) {}", params_str, ret.format_with_names(type_env))
            }
        }
    }
}

/// Display implementation for MonoType - shows Type#<id> for named types.
/// For user-facing error messages, use format_with_names() instead to show readable names.
/// This Display is primarily for debugging and internal use.
impl fmt::Display for MonoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MonoType::Int => write!(f, "Int"),
            MonoType::Float => write!(f, "Float"),
            MonoType::Bool => write!(f, "Bool"),
            MonoType::String => write!(f, "String"),
            MonoType::Void => write!(f, "Void"),
            MonoType::Named { type_id, args } => {
                if args.is_empty() {
                    write!(f, "Type#{}", type_id.0)
                } else {
                    write!(f, "Type#{}<", type_id.0)?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")
                }
            }
            MonoType::Array(elem_ty) => write!(f, "Array<{}>", elem_ty),
            MonoType::Dict(k, v) => write!(f, "Dict<{}, {}>", k, v),
            MonoType::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") {}", ret)
            }
        }
    }
}

/// Type definition (resolved from AST type declarations)
#[derive(Debug, Clone)]
pub enum TypeDef {
    /// Record type: nominal struct with named fields
    Record {
        name: String,
        fields: Vec<RecordField>,
    },
    /// Sum type: nominal enum with named variants
    Sum {
        name: String,
        variants: Vec<Variant>,
    },
    /// Type alias: transparent alias to another type
    Alias {
        name: String,
        target: MonoType,
    },
}

impl TypeDef {
    /// Get the name of this type definition
    pub fn name(&self) -> &str {
        match self {
            TypeDef::Record { name, .. } => name,
            TypeDef::Sum { name, .. } => name,
            TypeDef::Alias { name, .. } => name,
        }
    }

    /// Check if this is a sum type
    pub fn is_sum(&self) -> bool {
        matches!(self, TypeDef::Sum { .. })
    }

    /// Check if this is a record type
    pub fn is_record(&self) -> bool {
        matches!(self, TypeDef::Record { .. })
    }
}

/// Record field with name and type
#[derive(Debug, Clone)]
pub struct RecordField {
    pub name: String,
    pub ty: MonoType,
}

/// Sum type variant with name and field types
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<MonoType>,
}

/// Function signature for value environment
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<MonoType>,
    pub ret: Option<MonoType>, // None means infer from body
}
