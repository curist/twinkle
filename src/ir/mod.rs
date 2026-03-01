// Core IR and ANF IR - Stage 3+

pub mod anf;
pub mod core;
pub mod error;
pub mod local_allocator;
pub mod lower;
pub mod lower_anf;

// Re-export commonly used types
pub use core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, FieldId, FuncId, FunctionDef, LocalId,
    MatchArm, VariantId,
};
pub use error::LowerError;
pub use local_allocator::LocalAllocator;
pub use lower::Lowerer;
pub use anf::AnfModule;
