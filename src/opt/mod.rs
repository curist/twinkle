pub mod use_count;
pub mod passes;
pub mod liveness;
pub mod pipeline;

pub use pipeline::optimize_module;
