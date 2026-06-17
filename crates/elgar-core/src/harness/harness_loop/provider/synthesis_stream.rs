//! Stream chunk logging for no-tool synthesis calls.

use serde_json::json;

use crate::{
    harness::provider_route::HARNESS_SYNTHESIS_REQUEST_MODE,
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ProviderStreamChunk,
    session::Session,
};

pub(super) fn log_synthesis_stream_chunk(
    session: &Session,
    request_id: &str,
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
            "run_primitive_loop_synthesis",
            "harness_synthesis_provider_stream_chunk",
        )
        .with_metadata(json!({
            "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
            "request_id": request_id,
            "sequence": sequence,
            "chunk_kind": synthesis_stream_chunk_kind(chunk),
            "chunk_chars": synthesis_stream_chunk_chars(chunk),
            "preview": synthesis_stream_chunk_preview(chunk, 240)
        })),
    );
}

fn synthesis_stream_chunk_kind(chunk: &ProviderStreamChunk) -> &'static str {
    match chunk {
        ProviderStreamChunk::Reasoning(_) => "reasoning",
        ProviderStreamChunk::Text(_) => "text",
        ProviderStreamChunk::ToolCallDelta(_) => "tool_call_delta",
    }
}

fn synthesis_stream_chunk_chars(chunk: &ProviderStreamChunk) -> usize {
    match chunk {
        ProviderStreamChunk::Reasoning(value) | ProviderStreamChunk::Text(value) => {
            value.chars().count()
        }
        ProviderStreamChunk::ToolCallDelta(delta) => delta
            .function_arguments
            .as_deref()
            .unwrap_or_default()
            .chars()
            .count(),
    }
}

fn synthesis_stream_chunk_preview(chunk: &ProviderStreamChunk, max_chars: usize) -> String {
    let value = match chunk {
        ProviderStreamChunk::Reasoning(value) | ProviderStreamChunk::Text(value) => value.as_str(),
        ProviderStreamChunk::ToolCallDelta(delta) => delta
            .function_arguments
            .as_deref()
            .or(delta.function_name.as_deref())
            .unwrap_or_default(),
    };

    value.chars().take(max_chars).collect()
}
