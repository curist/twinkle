// Type representation, unification, inference - Stage 2+

pub mod check;
pub mod env;
pub mod error;
pub mod patterns;
pub mod resolve;
pub mod ty;
pub mod type_map;

// Re-export commonly used types
pub use check::TypeChecker;
pub use env::{LocalEnv, TypeEnv, ValueEnv};
pub use error::TypeError;
pub use patterns::PatternChecker;
pub use resolve::Resolver;
pub use ty::{FunctionSignature, MonoType, RecordField, TypeDef, TypeId, Variant};
pub use type_map::TypeMap;
