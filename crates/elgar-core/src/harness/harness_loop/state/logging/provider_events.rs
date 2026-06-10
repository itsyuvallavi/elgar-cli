//! Provider-call system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    harness::harness_loop::evidence::state::EvidencePromptStats,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_provider_call_started(
    session: &Session,
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_provider_call_started",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "request_id": request_id,
            "request_mode": request_mode,
            "loop_phase": phase
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_provider_call_finished(
    session: &Session,
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
    metrics: &Option<crate::event::ProviderMetrics>,
) {
    let usage = metrics.as_ref().and_then(|metrics| metrics.usage.as_ref());
    let backend = metrics
        .as_ref()
        .and_then(|metrics| metrics.backend)
        .map(|backend| format!("{backend:?}"));

    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_provider_call_finished",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "request_id": request_id,
            "request_mode": request_mode,
            "loop_phase": phase,
            "backend": backend,
            "prompt_tokens": usage.and_then(|usage| usage.prompt_tokens),
            "completion_tokens": usage.and_then(|usage| usage.completion_tokens),
            "total_tokens": usage.and_then(|usage| usage.total_tokens)
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_provider_call_failed(
    session: &Session,
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
    error: &str,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_provider_call_failed",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "request_id": request_id,
            "request_mode": request_mode,
            "loop_phase": phase,
            "error": error
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_turn_prompt_context(
    session: &Session,
    initial_message_count: usize,
    history_turns: usize,
    verified_fact_count: usize,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_turn_prompt_context_built",
        )
        .with_metadata(json!({
            "initial_message_count": initial_message_count,
            "history_turns": history_turns,
            "verified_fact_count": verified_fact_count
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_decision_context(
    session: &Session,
    round_index: usize,
    evidence_mode: &str,
    stats: &EvidencePromptStats,
    phase: &str,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_decision_context_built",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "loop_phase": phase,
            "evidence_mode": evidence_mode,
            "evidence_items": stats.item_count,
            "full_evidence_bytes": stats.full_bytes,
            "prompt_evidence_bytes": stats.compact_bytes
        })),
    );
}
