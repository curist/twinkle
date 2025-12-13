// Core IR and ANF IR - Stage 3+

pub mod core;
pub mod error;
pub mod local_allocator;

// Re-export commonly used types
pub use core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, FieldId, FuncId, FunctionDef, LocalId,
    MatchArm, VariantId,
};
pub use error::LowerError;
pub use local_allocator::LocalAllocator;
