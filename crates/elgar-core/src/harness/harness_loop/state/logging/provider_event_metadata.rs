//! Metadata builders for provider-call system log events.

use serde_json::{json, Value};

use crate::{
    event::{ProviderOutput, ProviderStreamTimings},
    provider::ProviderStreamChunk,
};

pub(super) fn provider_call_finished_metadata(
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
    backend: Option<String>,
    output: &ProviderOutput,
    stream_timings: &ProviderStreamTimings,
) -> Value {
    let usage = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.usage.as_ref());

    json!({
        "round_index": round_index,
        "request_id": request_id,
        "request_mode": request_mode,
        "loop_phase": phase,
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
    })
}

pub(super) fn provider_stream_chunk_metadata(
    round_index: usize,
    request_id: &str,
    request_mode: &str,
    phase: &str,
    sequence: u64,
    chunk: &ProviderStreamChunk,
) -> Value {
    json!({
        "round_index": round_index,
        "request_id": request_id,
        "request_mode": request_mode,
        "loop_phase": phase,
        "sequence": sequence,
        "chunk_kind": stream_chunk_kind(chunk),
        "chunk_chars": stream_chunk_chars(chunk),
        "preview": stream_chunk_preview(chunk, 240)
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
        ProviderStreamChunk::ToolCallDelta(delta) => delta
            .function_arguments
            .as_deref()
            .unwrap_or_default()
            .chars()
            .count(),
    }
}

fn stream_chunk_preview(chunk: &ProviderStreamChunk, max_chars: usize) -> String {
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

#[cfg(test)]
mod tests {
    use crate::event::{ProviderOutput, ProviderStreamTimings};

    use super::provider_call_finished_metadata;

    #[test]
    fn provider_call_finished_metadata_includes_thinking_diagnostics() {
        let output = ProviderOutput::new("ok").with_thinking("one two");

        let metadata = provider_call_finished_metadata(
            1,
            "request-1",
            "harness_tool_decision",
            "native_tool_loop",
            None,
            &output,
            &ProviderStreamTimings::from_stream_marks(
                Some(100),
                Some(250),
                Some(175),
                Some(400),
                Some(475),
                500,
            ),
        );

        assert_eq!(metadata["provider_response_has_thinking"], true);
        assert_eq!(metadata["provider_response_thinking_chars"], 7);
        assert_eq!(metadata["first_reasoning_ms"], 100);
        assert_eq!(metadata["first_text_ms"], 250);
        assert_eq!(metadata["last_reasoning_ms"], 175);
        assert_eq!(metadata["last_text_ms"], 400);
        assert_eq!(metadata["last_chunk_ms"], 475);
        assert_eq!(metadata["reasoning_to_text_ms"], 150);
        assert_eq!(metadata["last_chunk_to_finish_ms"], 25);
        assert_eq!(metadata["total_stream_ms"], 500);
    }
}
