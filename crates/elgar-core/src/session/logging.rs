//! Session event metadata helpers.

use serde_json::json;

use crate::event::Event;

pub(super) fn session_event_metadata(event: &Event) -> serde_json::Value {
    let mut metadata = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
    match event {
        Event::ProviderFinished(finished) => {
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "provider_response_has_thinking".to_string(),
                    json!(finished.output.has_thinking()),
                );
                object.insert(
                    "provider_response_thinking_chars".to_string(),
                    json!(finished.output.thinking_chars()),
                );
                object.insert(
                    "reasoning_output_chars".to_string(),
                    json!(finished.output.thinking_chars()),
                );
                if let Some(timings) = finished.stream_timings.as_ref() {
                    object.insert(
                        "first_reasoning_ms".to_string(),
                        json!(timings.first_reasoning_ms),
                    );
                    object.insert("first_text_ms".to_string(), json!(timings.first_text_ms));
                    object.insert(
                        "last_reasoning_ms".to_string(),
                        json!(timings.last_reasoning_ms),
                    );
                    object.insert("last_text_ms".to_string(), json!(timings.last_text_ms));
                    object.insert("last_chunk_ms".to_string(), json!(timings.last_chunk_ms));
                    object.insert(
                        "reasoning_to_text_ms".to_string(),
                        json!(timings.reasoning_to_text_ms),
                    );
                    object.insert(
                        "last_chunk_to_finish_ms".to_string(),
                        json!(timings.last_chunk_to_finish_ms),
                    );
                    object.insert("total_stream_ms".to_string(), json!(timings.total_ms));
                }
                if let Some(metrics) = finished.output.metrics.as_ref() {
                    object.insert(
                        "reasoning_level_requested".to_string(),
                        json!(metrics.reasoning),
                    );
                    object.insert(
                        "reasoning_output_tokens".to_string(),
                        json!(metrics.reasoning_output_tokens),
                    );
                    object.insert(
                        "reasoning_request_format".to_string(),
                        json!(metrics.reasoning_request_format),
                    );
                    object.insert(
                        "provider_supports_reasoning_control".to_string(),
                        json!(metrics.provider_supports_reasoning_control),
                    );
                    object.insert(
                        "stream_done_ms".to_string(),
                        json!(metrics.stream_done_millis),
                    );
                    object.insert(
                        "last_chunk_to_done_ms".to_string(),
                        json!(metrics.last_chunk_to_done_millis),
                    );
                    object.insert(
                        "done_to_finish_ms".to_string(),
                        json!(metrics.done_to_finish_millis),
                    );
                }
            }
        }
        Event::ProviderStreamChunk(chunk) => {
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "provider_stream_chunk_kind".to_string(),
                    json!(provider_stream_chunk_kind(&chunk.chunk)),
                );
                object.insert(
                    "provider_stream_chunk_chars".to_string(),
                    json!(provider_stream_chunk_chars(&chunk.chunk)),
                );
            }
        }
        _ => {}
    }
    metadata
}

pub(super) fn event_log_kind(event: &Event) -> &'static str {
    match event {
        Event::UserMessage(_) => "user_message",
        Event::AssistantMessage(_) => "assistant_message",
        Event::ProviderStarted(_) => "provider_started",
        Event::ProviderFinished(_) => "provider_finished",
        Event::ProviderStreamChunk(_) => "provider_stream_chunk",
        Event::Error(_) => "error",
    }
}

fn provider_stream_chunk_kind(chunk: &crate::provider::ProviderStreamChunk) -> &'static str {
    match chunk {
        crate::provider::ProviderStreamChunk::Reasoning(_) => "reasoning",
        crate::provider::ProviderStreamChunk::Text(_) => "text",
        crate::provider::ProviderStreamChunk::ToolCallDelta(_) => "tool_call_delta",
    }
}

fn provider_stream_chunk_chars(chunk: &crate::provider::ProviderStreamChunk) -> usize {
    match chunk {
        crate::provider::ProviderStreamChunk::Reasoning(value)
        | crate::provider::ProviderStreamChunk::Text(value) => value.chars().count(),
        crate::provider::ProviderStreamChunk::ToolCallDelta(delta) => delta
            .function_arguments
            .as_deref()
            .unwrap_or_default()
            .chars()
            .count(),
    }
}
