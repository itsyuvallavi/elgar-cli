//! Verified-evidence system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    harness::harness_loop::state::types::Evidence,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_loop_evidence(
    session: &Session,
    round_index: usize,
    evidence: &Evidence,
) {
    let metadata = json!({
        "round_index": round_index,
        "evidence_label": evidence.label,
        "evidence_bytes": evidence.bytes,
        "truncated": evidence.truncated
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_evidence_collected",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_tool_result_verified", metadata);
}
