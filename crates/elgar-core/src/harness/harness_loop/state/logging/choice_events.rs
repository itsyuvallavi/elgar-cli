//! Model-choice and repair system-log events for the primitive harness loop.

use std::time::Instant;

use serde_json::json;

use crate::{
    harness::ModelChoice,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_loop_model_choice(
    session: &Session,
    round_index: usize,
    duration_ms: u64,
    choice: &ModelChoice,
    metrics: &Option<crate::event::ProviderMetrics>,
    fallback_request_id: &str,
) {
    let usage = metrics.as_ref().and_then(|metrics| metrics.usage.as_ref());
    let backend = metrics
        .as_ref()
        .and_then(|metrics| metrics.backend)
        .map(|backend| format!("{backend:?}"));
    let tool = match choice {
        ModelChoice::StructuredRequest(request) => Some(request.kind.as_str().to_string()),
        _ => None,
    };
    let tools = match choice {
        ModelChoice::StructuredRequests(requests) => requests
            .iter()
            .map(|request| request.kind.as_str())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let batch_size = match choice {
        ModelChoice::StructuredRequests(requests) => Some(requests.len()),
        _ => None,
    };
    let (answer_reason_chars, evidence_depth) = match choice {
        ModelChoice::AnswerNow {
            reason,
            evidence_depth,
        } => (Some(reason.chars().count()), Some(evidence_depth.as_str())),
        _ => (None, None),
    };

    let metadata = json!({
        "round_index": round_index,
        "choice_type": choice_type(choice),
        "tool": tool,
        "tools": tools,
        "batch_size": batch_size,
        "answer_reason_chars": answer_reason_chars,
        "evidence_depth": evidence_depth,
        "request_id": metrics.as_ref().map(|metrics| metrics.request_id.as_str()).unwrap_or(fallback_request_id),
        "backend": backend,
        "prompt_tokens": usage.and_then(|usage| usage.prompt_tokens),
        "completion_tokens": usage.and_then(|usage| usage.completion_tokens),
        "total_tokens": usage.and_then(|usage| usage.total_tokens)
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_model_choice",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_model_decision", metadata);
}

pub(in crate::harness::harness_loop) fn log_loop_repair_started(
    session: &Session,
    round_index: usize,
    error: &str,
    raw: &str,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_repair_started",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "error": error,
            "raw_preview": bounded_preview(raw)
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_loop_repair_finished(
    session: &Session,
    round_index: usize,
    started: Instant,
    choice: &ModelChoice,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_repair_finished",
        )
        .with_duration_ms(started.elapsed().as_millis() as u64)
        .with_metadata(json!({
            "round_index": round_index,
            "repaired_choice_type": choice_type(choice)
        })),
    );
}

fn choice_type(choice: &ModelChoice) -> &'static str {
    match choice {
        ModelChoice::Message { .. } => "message",
        ModelChoice::AnswerNow { .. } => "answer_now",
        ModelChoice::StructuredRequest(_) => "structured_request",
        ModelChoice::StructuredRequests(_) => "structured_requests",
        ModelChoice::InvalidStructuredRequest { .. } => "invalid_structured_request",
    }
}

fn bounded_preview(value: &str) -> String {
    const MAX_CHARS: usize = 1_000;
    let mut preview = value.chars().take(MAX_CHARS).collect::<String>();
    if value.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    preview
}
