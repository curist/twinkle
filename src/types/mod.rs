// Type representation, unification, inference - Stage 2+

pub mod ty;
pub mod env;
pub mod error;
pub mod resolve;
pub mod check;
pub mod patterns;
pub mod type_map;

// Re-export commonly used types
pub use ty::{MonoType, TypeDef, TypeId, RecordField, Variant, FunctionSignature};
pub use env::{TypeEnv, ValueEnv, LocalEnv};
pub use error::TypeError;
pub use resolve::Resolver;
pub use check::TypeChecker;
pub use patterns::PatternChecker;
pub use type_map::TypeMap;
