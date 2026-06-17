//! System/session log events for no-tool synthesis calls.

use std::time::Instant;

use serde_json::json;

use crate::{
    event::{ProviderOutput, ProviderStreamTimings},
    harness::{provider_route::HARNESS_SYNTHESIS_REQUEST_MODE, EvidenceDepth},
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(super) fn log_synthesis_started(
    session: &Session,
    request_id: &str,
    stop_reason: &str,
    evidence_depth: EvidenceDepth,
    evidence_bytes: usize,
) {
    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "stop_reason": stop_reason,
        "evidence_depth": evidence_depth.as_str(),
        "evidence_mode": "full_verified",
        "evidence_bytes": evidence_bytes
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_loop_synthesis",
            "harness_loop_synthesis_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_synthesis_started", metadata);
}

pub(super) fn log_synthesis_finished(
    session: &Session,
    started: Instant,
    request_id: &str,
    response_chars: usize,
    output: &ProviderOutput,
    stream_timings: &ProviderStreamTimings,
) {
    let usage = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.usage.as_ref());
    let backend = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.backend)
        .map(|backend| format!("{backend:?}"));

    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "response_chars": response_chars,
        "backend": backend,
        "provider_response_has_thinking": output.has_thinking(),
        "provider_response_thinking_chars": output.thinking_chars(),
        "first_reasoning_ms": stream_timings.first_reasoning_ms,
        "first_text_ms": stream_timings.first_text_ms,
        "last_reasoning_ms": stream_timings.last_reasoning_ms,
        "last_text_ms": stream_timings.last_text_ms,
        "last_chunk_ms": stream_timings.last_chunk_ms,
        "reasoning_to_text_ms": stream_timings.reasoning_to_text_ms,
        "last_chunk_to_finish_ms": stream_timings.last_chunk_to_finish_ms,
        "stream_done_ms": output
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.stream_done_millis),
        "last_chunk_to_done_ms": output
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.last_chunk_to_done_millis),
        "done_to_finish_ms": output
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.done_to_finish_millis),
        "total_stream_ms": stream_timings.total_ms,
        "prompt_tokens": usage.and_then(|usage| usage.prompt_tokens),
        "completion_tokens": usage.and_then(|usage| usage.completion_tokens),
        "total_tokens": usage.and_then(|usage| usage.total_tokens)
    });
    append_synthesis_log(
        session,
        "harness_loop_synthesis_finished",
        started,
        metadata,
    );
}

pub(super) fn log_synthesis_failed(
    session: &Session,
    started: Instant,
    request_id: &str,
    error: &str,
) {
    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "error": error
    });
    append_synthesis_log(session, "harness_loop_synthesis_failed", started, metadata);
}

pub(super) fn log_synthesis_canceled(session: &Session, started: Instant, request_id: &str) {
    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id
    });
    append_synthesis_log(
        session,
        "harness_loop_synthesis_canceled",
        started,
        metadata,
    );
}

fn append_synthesis_log(
    session: &Session,
    event_name: &'static str,
    started: Instant,
    metadata: serde_json::Value,
) {
    let duration_ms = started.elapsed().as_millis() as u64;
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_loop_synthesis",
            event_name,
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    let mut session_metadata = metadata;
    if let Some(object) = session_metadata.as_object_mut() {
        object.insert("duration_ms".to_string(), json!(duration_ms));
    }
    session.log_harness_event(
        event_name.replace("harness_loop_", "harness_"),
        session_metadata,
    );
}
