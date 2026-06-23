//! Compact session status system-log events.
//!
//! These events summarize runtime state for diagnostics without logging raw
//! prompts, responses, reasoning, or tool-result bodies.

use serde_json::json;

use crate::logs::system::{append_log_event, LogInput, LogPhase};

use super::Session;

pub(super) fn log_session_context_status(session: &Session) {
    let snapshot = session.latest_context_window_snapshot();
    let totals = session.session_token_totals();
    let latest = session.latest_turn_token_usage();
    let pending = session.pending_approval();
    let provider = session.provider_metadata();

    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "record_provider_metrics",
            "harness_session_context_status",
        )
        .with_metadata(json!({
            "request_id": snapshot.last_request_id,
            "provider": provider.map(|metadata| metadata.provider.as_str()),
            "model": provider.and_then(|metadata| metadata.model.as_deref()),
            "permission_mode": session.permission_mode().as_str(),
            "turn_input_tokens": latest.and_then(|usage| usage.input_tokens),
            "turn_output_tokens": latest.and_then(|usage| usage.output_tokens),
            "turn_total_tokens": latest.and_then(|usage| usage.total_tokens),
            "session_input_tokens": totals.input_tokens,
            "session_output_tokens": totals.output_tokens,
            "session_reasoning_tokens": totals.reasoning_tokens,
            "session_total_tokens": totals.total_tokens,
            "context_window_tokens": snapshot.context_window_tokens,
            "context_used_percent": snapshot.used_percent,
            "context_remaining_percent": snapshot.remaining_percent,
            "context_source": snapshot.source,
            "pending_approval_id": pending.map(|approval| approval.id.as_str()),
            "pending_approval_tool": pending.map(|approval| approval.tool.as_str()),
            "pending_approval_status": pending.map(|approval| approval.status.as_str()),
        })),
    );
}
