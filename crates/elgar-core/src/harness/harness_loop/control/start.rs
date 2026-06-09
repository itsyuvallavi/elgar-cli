//! Loop-start logging for the primitive harness coordinator.
//!
//! Startup logging is kept outside the coordinator so the coordinator only
//! describes runtime order.

use serde_json::json;

use crate::{
    harness::harness_loop::state::budget::PrimitiveLoopBudget,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

/// Record the initial loop guard settings and input size.
pub(super) fn log_loop_started(
    session: &Session,
    turn_id: u64,
    input: &str,
    budget: &PrimitiveLoopBudget,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_started",
        )
        .with_metadata(json!({
            "content_limits_enabled": false,
            "max_repair_attempts": budget.max_repair_attempts,
            "input_chars": input.chars().count(),
            "mode": "read_only"
        })),
    );
}
