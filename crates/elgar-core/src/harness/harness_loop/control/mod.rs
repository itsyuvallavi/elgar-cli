//! Loop-control modules for the primitive harness.
//!
//! Control code decides loop order and finish paths. It does not know how to
//! talk to providers directly and does not implement primitive tools.

mod choice_from_output;
mod coordinator;
mod finish;
mod request_handling;
mod start;

pub use coordinator::run_primitive_harness_loop;
pub use finish::render_primitive_harness_loop_result;
