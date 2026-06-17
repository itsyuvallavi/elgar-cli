//! System-log helpers for interactive provider turns.
//!
//! These events make the live TUI path auditable without logging full model
//! response text into the system log.

use std::time::Instant;

use elgar_core::{
    event::{Event, ProviderStreamChunkReceived},
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ProviderStreamChunk,
    session::Session,
};

use crate::{terminal::ui::prompt::LiveProviderOutput, turn_metrics::duration_millis};

pub(super) fn log_tui_provider_turn_started(session: &Session, turn_id: u64, input_chars: usize) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Tui,
            file!(),
            "log_tui_provider_turn_started",
            "tui_provider_turn_started",
        )
        .with_metadata(serde_json::json!({
            "input_chars": input_chars,
            "turn_kind": "harness"
        })),
    );
}

pub(super) fn log_live_preview_render(
    session: &Session,
    turn_id: u64,
    turn_started: Instant,
    render_reason: &'static str,
    live_output: &LiveProviderOutput,
    stream_chunk: Option<&ProviderStreamChunkReceived>,
    render_duration_ms: Option<u64>,
    chunk_to_render_ms: Option<u64>,
) {
    let preview = live_output.response_preview_stats();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Render,
            file!(),
            "log_live_preview_render",
            "tui_live_preview_rendered",
        )
        .with_metadata(serde_json::json!({
            "render_reason": render_reason,
            "turn_elapsed_ms": duration_millis(turn_started.elapsed()),
            "render_duration_ms": render_duration_ms,
            "chunk_to_render_ms": chunk_to_render_ms,
            "request_id": stream_chunk.map(|chunk| chunk.request_id.as_str()),
            "chunk_sequence": stream_chunk.map(|chunk| chunk.sequence),
            "chunk_kind": stream_chunk.map(|chunk| stream_chunk_kind(&chunk.chunk)),
            "chunk_chars": stream_chunk.map(|chunk| stream_chunk_chars(&chunk.chunk)),
            "reasoning_chars": live_output.reasoning_chars(),
            "raw_response_chars": preview.raw_response_chars,
            "has_response_preview": preview.has_preview,
            "rendered_preview_chars": preview.rendered_preview_chars,
            "rendered_preview_lines": preview.rendered_preview_lines,
        })),
    );
}

pub(super) fn log_ui_render_finished(
    session: &Session,
    turn_id: u64,
    turn_duration_millis: u64,
    events: &[Event],
    completion_to_render_ms: u64,
    conversation_lines_before: usize,
    conversation_lines_after: usize,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Render,
            file!(),
            "log_ui_render_finished",
            "ui_render_finished",
        )
        .with_duration_ms(turn_duration_millis)
        .with_metadata(serde_json::json!({
            "events_applied": events.len(),
            "provider_started_count": count_events(events, is_provider_started),
            "provider_finished_count": count_events(events, is_provider_finished),
            "assistant_message_count": count_events(events, is_assistant_message),
            "latest_provider_request_id": latest_provider_request_id(events),
            "completion_to_render_ms": completion_to_render_ms,
            "conversation_lines_before": conversation_lines_before,
            "conversation_lines_after": conversation_lines_after
        })),
    );
}

pub(super) fn log_live_preview_finalized(
    session: &Session,
    turn_id: u64,
    preserve_candidate: bool,
    preserved_preview: bool,
    live_preview_chars: usize,
    final_chars: usize,
    finalize_render_ms: u64,
) {
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Render,
            file!(),
            "log_live_preview_finalized",
            "tui_live_preview_finalized",
        )
        .with_metadata(serde_json::json!({
            "preview_matched_final": preserve_candidate,
            "preserved_preview": preserved_preview,
            "final_content_changed": !preserve_candidate,
            "live_preview_chars": live_preview_chars,
            "final_chars": final_chars,
            "finalize_render_ms": finalize_render_ms,
        })),
    );
}

fn count_events(events: &[Event], predicate: fn(&Event) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}

fn is_provider_started(event: &Event) -> bool {
    matches!(event, Event::ProviderStarted(_))
}

fn is_provider_finished(event: &Event) -> bool {
    matches!(event, Event::ProviderFinished(_))
}

fn is_assistant_message(event: &Event) -> bool {
    matches!(event, Event::AssistantMessage(_))
}

fn latest_provider_request_id(events: &[Event]) -> Option<&str> {
    events.iter().rev().find_map(|event| match event {
        Event::ProviderFinished(finished) => Some(finished.request_id.as_str()),
        Event::ProviderStarted(started) => Some(started.request_id.as_str()),
        _ => None,
    })
}

fn stream_chunk_kind(chunk: &ProviderStreamChunk) -> &'static str {
    match chunk {
        ProviderStreamChunk::Reasoning(_) => "reasoning",
        ProviderStreamChunk::Text(_) => "text",
        ProviderStreamChunk::ToolCallDelta(_) => "tool_call_delta",
    }
}

fn stream_chunk_chars(chunk: &ProviderStreamChunk) -> usize {
    match chunk {
        ProviderStreamChunk::Reasoning(value) | ProviderStreamChunk::Text(value) => {
            value.chars().count()
        }
        ProviderStreamChunk::ToolCallDelta(_) => 0,
    }
}
