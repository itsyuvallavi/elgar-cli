//! Loop-control modules for the primitive harness.
//!
//! Control code decides loop order and finish paths. It does not know how to
//! talk to providers directly and does not implement primitive tools.

mod approval_claim_guard;
mod choice_from_output;
mod choice_repair;
mod coordinator;
mod finish;
mod native_execution;
mod native_tool_round;
mod prose_claim_guard;
mod provider_claim_retry;
mod request_handling;
mod start;
mod structured_choice_round;
mod synthetic_tool_calls;
mod tool_target_fidelity;

pub use coordinator::run_primitive_harness_loop;
pub use finish::render_primitive_harness_loop_result;
