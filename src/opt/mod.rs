pub mod defer_elim;
pub mod liveness;
pub mod passes;
pub mod pipeline;
pub mod use_count;

pub use pipeline::optimize_module;
