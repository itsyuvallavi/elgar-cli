//! Provider-call system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    event::{ProviderOutput, ProviderStreamTimings},
    harness::harness_loop::{
        evidence::state::EvidencePromptStats, provider::session_context::TurnPromptContextStats,
    },
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ProviderStreamChunk,
    session::Session,
};

use super::provider_event_metadata::{
    provider_call_finished_metadata, provider_stream_chunk_metadata,
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
    output: &ProviderOutput,
    stream_timings: &ProviderStreamTimings,
) {
    let backend = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.backend)
        .map(|backend| format!("{backend:?}"));
    let metadata = provider_call_finished_metadata(
        round_index,
        request_id,
        request_mode,
        phase,
        backend,
        output,
        stream_timings,
    );

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
        .with_metadata(metadata),
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

pub(in crate::harness::harness_loop) fn log_provider_call_canceled(
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
            "provider_request_canceled",
        )
        .with_metadata(json!({
            "round_index": round_index,
            "request_id": request_id,
            "request_mode": request_mode,
            "loop_phase": phase
        })),
    );
}

pub(in crate::harness::harness_loop) fn log_provider_stream_chunk(
    session: &Session,
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
    sequence: u64,
    chunk: &ProviderStreamChunk,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_provider_stream_chunk",
        )
        .with_metadata(provider_stream_chunk_metadata(
            round_index,
            request_id,
            request_mode,
            phase,
            sequence,
            chunk,
        )),
    );
}

pub(in crate::harness::harness_loop) fn log_turn_prompt_context(
    session: &Session,
    stats: &TurnPromptContextStats,
) {
    let memory = &stats.memory;
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
            "initial_message_count": stats.initial_message_count,
            "history_turns": stats.history_turns,
            "system_prompt_chars": stats.system_prompt_chars,
            "history_prompt_chars": stats.history_prompt_chars,
            "memory_prompt_chars": stats.memory_prompt_chars,
            "mcp_catalog_chars": stats.mcp_catalog_chars,
            "total_initial_prompt_chars": stats.total_initial_prompt_chars,
            "history_token_budget": stats.history_token_budget,
            "history_budget_hit": stats.history_budget_hit,
            "assistant_replay_chars": stats.assistant_replay_chars,
            "memory_selection_strategy": memory.selection_strategy,
            "verified_fact_count": memory.indexed_fact_count,
            "indexed_fact_count": memory.indexed_fact_count,
            "rendered_fact_count": memory.rendered_fact_count,
            "rendered_memory_chars": memory.rendered_memory_chars,
            "omitted_fact_count": memory.omitted_fact_count,
            "memory_budget_hit": memory.memory_budget_hit,
            "rendered_by_kind": {
                "read": memory.rendered_read_file_facts,
                "listed": memory.rendered_listed_directory_facts,
                "find": memory.rendered_find_facts,
                "grep": memory.rendered_grep_facts,
                "executed": memory.rendered_approved_execution_facts
            },
            "omitted_by_kind": {
                "read": memory.omitted_read_file_facts,
                "listed": memory.omitted_listed_directory_facts,
                "find": memory.omitted_find_facts,
                "grep": memory.omitted_grep_facts,
                "executed": memory.omitted_approved_execution_facts
            }
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
