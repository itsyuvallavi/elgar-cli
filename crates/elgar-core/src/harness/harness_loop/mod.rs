//! Bounded harness loops.
//!
//! Loop modules coordinate repeated model-choice and primitive evidence
//! collection. Individual primitive implementations live outside the loop.

mod control;
mod evidence;
mod provider;
mod state;

pub use control::{render_primitive_harness_loop_result, run_primitive_harness_loop};
pub use state::types::{PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound};
