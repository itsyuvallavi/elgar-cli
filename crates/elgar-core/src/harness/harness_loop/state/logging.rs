//! System-log helpers for the primitive harness loop.
//!
//! These helpers keep logging shape consistent without cluttering the loop
//! coordinator.

use std::time::Instant;

use serde_json::json;

use crate::{
    harness::{
        harness_loop::{evidence::state::EvidencePromptStats, state::memory::HarnessWorkingMemory},
        ModelChoice, PendingApproval, PermissionDecision, ValidatedStructuredRequest,
    },
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

use super::types::{Evidence, PrimitiveHarnessLoopResult};

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

pub(in crate::harness::harness_loop) fn log_permission_decision(
    session: &Session,
    round_index: usize,
    request: &ValidatedStructuredRequest,
    decision: &PermissionDecision,
) {
    let metadata = json!({
        "round_index": round_index,
        "tool": request.kind.as_str(),
        "decision": decision.kind.as_str(),
        "reason": decision.reason.as_str(),
        "execution_allowed": decision.allows_execution()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_permission_decision",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_permission_decision", metadata);
}

pub(in crate::harness::harness_loop) fn log_harness_approval_requested(
    session: &Session,
    round_index: usize,
    approval: &PendingApproval,
) {
    let metadata = json!({
        "round_index": round_index,
        "approval_id": approval.id.as_str(),
        "tool": approval.tool.as_str(),
        "status": approval.status.as_str(),
        "reason": approval.reason.as_str(),
        "arguments_preview_chars": approval.arguments_preview.chars().count(),
        "execution_allowed": false
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_approval_requested",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_approval_requested", metadata);
}

pub(in crate::harness::harness_loop) fn log_harness_duplicate_rejected(
    session: &Session,
    round_index: usize,
    label: &str,
    memory: &HarnessWorkingMemory,
) {
    let metadata = json!({
        "round_index": round_index,
        "duplicate_label": label,
        "duplicate_requests": memory.duplicate_requests()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_duplicate_rejected",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_duplicate_rejected", metadata);
}

pub(in crate::harness::harness_loop) fn log_harness_memory_snapshot(
    session: &Session,
    round_index: usize,
    reason: &str,
    memory: &HarnessWorkingMemory,
) {
    let metadata = json!({
        "round_index": round_index,
        "reason": reason,
        "listed_paths": memory.listed_paths(),
        "directory_listings": memory.directory_listings().into_iter().map(|listing| {
            json!({
                "path": &listing.path,
                "dirs": &listing.dirs,
                "files": &listing.files,
                "omitted_dirs": listing.omitted_dirs,
                "omitted_files": listing.omitted_files,
                "truncated": listing.truncated
            })
        }).collect::<Vec<_>>(),
        "read_paths": memory.read_paths(),
        "find_patterns": memory.find_patterns(),
        "grep_queries": memory.grep_queries(),
        "duplicate_requests": memory.duplicate_requests()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_memory_snapshot",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_memory_snapshot", metadata);
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
