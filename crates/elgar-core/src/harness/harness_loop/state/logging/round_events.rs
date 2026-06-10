//! Loop round and finish system-log events for the primitive harness loop.

use std::time::Instant;

use serde_json::json;

use crate::{
    harness::harness_loop::state::types::PrimitiveHarnessLoopResult,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_loop_round_started(
    session: &Session,
    round_index: usize,
    evidence_count: usize,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_round_started",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "evidence_count": evidence_count
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_loop_round_finished(
    session: &Session,
    round_index: usize,
    started: Instant,
    result: &str,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_round_finished",
        )
        .with_duration_ms(started.elapsed().as_millis() as u64)
        .with_metadata(json!({
            "round_index": round_index,
            "result": result
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_loop_finished(
    session: &Session,
    turn_id: u64,
    result: &PrimitiveHarnessLoopResult,
    started: Instant,
) {
    let metadata = json!({
        "rounds": result.rounds.len(),
        "stopped_reason": result.stopped_reason,
        "has_final_text": result.final_text.is_some()
    });
    let duration_ms = started.elapsed().as_millis() as u64;
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    let mut session_metadata = metadata;
    if let Some(object) = session_metadata.as_object_mut() {
        object.insert("duration_ms".to_string(), json!(duration_ms));
    }
    session.log_harness_event("harness_turn_finished", session_metadata);
}
