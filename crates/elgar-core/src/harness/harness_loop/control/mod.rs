//! Loop-control modules for the primitive harness.
//!
//! Control code decides loop order and finish paths. It does not know how to
//! talk to providers directly and does not implement primitive tools.

mod choice_from_output;
mod choice_repair;
mod coordinator;
mod direct_display;
mod entrypoint;
mod finish;
mod loop_setup;
mod model_text_round;
mod native_execution;
mod native_tool_round;
mod prose_claim_guard;
mod provider_claim_retry;
mod provider_error;
mod request_handling;
mod start;
mod structured_choice_round;
mod synthetic_tool_calls;
mod tool_target_fidelity;

pub use coordinator::run_primitive_harness_loop_with_cancel_and_stream;
pub use entrypoint::{run_primitive_harness_loop, run_primitive_harness_loop_with_cancel};
pub use finish::render_primitive_harness_loop_result;
