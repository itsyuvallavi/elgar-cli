//! Normal streaming chat for OpenAI-compatible LM Studio requests.
//!
//! This module handles text/reasoning chunk emission and SSE `[DONE]`
//! completion for `/v1/chat/completions` requests without tool definitions.

use std::time::Instant;

use crate::provider::{
    config::ProviderConfig,
    http::{post_json_streaming_cancelable, HttpEndpoint, StreamingBodyAction},
    types::{ChatMessage, ProviderError, ProviderRequestProfile, ProviderStreamChunk},
    ProviderCancelToken,
};

use super::{
    chat_lm_studio_with_request_id_cancelable,
    metrics::{duration_millis, http_timeouts, metrics_for_request},
    streaming::{emit_output_chunks, StreamingOutputParts},
};
use crate::provider::lm_studio::{
    format::format_chat_request_body_with_tools_and_profile, parse::parse_provider_error_json,
};

/// Sends an OpenAI-compatible streaming request and emits chunks as they arrive.
pub(in crate::provider::lm_studio) fn chat_lm_studio_streaming_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: Option<&ProviderRequestProfile>,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> Result<crate::event::ProviderOutput, ProviderError> {
    chat_lm_studio_streaming_with_request_id_cancelable(
        config,
        messages,
        request_id,
        profile,
        on_chunk,
        &ProviderCancelToken::new(),
    )
}

pub(in crate::provider::lm_studio) fn chat_lm_studio_streaming_with_request_id_cancelable(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: Option<&ProviderRequestProfile>,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    cancel: &ProviderCancelToken,
) -> Result<crate::event::ProviderOutput, ProviderError> {
    cancel.error_if_canceled()?;
    if !config.stream {
        let output = chat_lm_studio_with_request_id_cancelable(
            config, messages, request_id, profile, cancel,
        )?;
        emit_output_chunks(&output, on_chunk);
        return Ok(output);
    }

    let started = Instant::now();
    let (request, body) =
        format_chat_request_body_with_tools_and_profile(config, messages, Vec::new(), profile)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len(), profile);
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let mut parts = StreamingOutputParts::default();
    log::debug!(
        "lm_studio_stream_request_start request_id={} endpoint={} model={} messages={} tools={} bytes={}",
        request_id,
        config.chat_completions_url(),
        request.model,
        request.messages.len(),
        request.tools.len(),
        body.len()
    );
    let mut last_provider_chunk_millis = None;
    let response = post_json_streaming_cancelable(
        &endpoint,
        &body,
        http_timeouts(config),
        &mut |body_chunk| {
            let done = parts.push_body_chunk(body_chunk, &mut |chunk| {
                let elapsed = duration_millis(started.elapsed());
                if metrics.first_chunk_latency_millis.is_none() {
                    metrics.first_chunk_latency_millis = Some(elapsed);
                }
                last_provider_chunk_millis = Some(elapsed);
                on_chunk(chunk);
            })?;
            if done {
                let done_ms = duration_millis(started.elapsed());
                metrics.stream_done_millis.get_or_insert(done_ms);
                metrics.last_chunk_to_done_millis.get_or_insert(
                    last_provider_chunk_millis
                        .map(|last| done_ms.saturating_sub(last))
                        .unwrap_or(0),
                );
                return Ok(StreamingBodyAction::Stop);
            }
            Ok(StreamingBodyAction::Continue)
        },
        cancel,
    )?;

    if response.status_code.is_success() {
        parts.finish(&mut |chunk| {
            if metrics.first_chunk_latency_millis.is_none() {
                metrics.first_chunk_latency_millis = Some(duration_millis(started.elapsed()));
            }
            on_chunk(chunk);
        })?;
        let total_duration = duration_millis(started.elapsed());
        metrics.total_duration_millis = Some(total_duration);
        metrics.done_to_finish_millis = metrics
            .stream_done_millis
            .map(|done_ms| total_duration.saturating_sub(done_ms));
        if let Some(usage) = parts.usage() {
            metrics.usage = Some(usage.clone());
        }
        log::debug!(
            "lm_studio_stream_request_finish request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            metrics.total_duration_millis.unwrap_or(0),
            response.body.len()
        );
        let output = parts.finish_output()?;
        Ok(output.with_metrics(metrics))
    } else {
        log::warn!(
            "lm_studio_stream_request_error request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            duration_millis(started.elapsed()),
            response.body.len()
        );
        Err(parse_provider_error_json(
            Some(response.status_code.as_u16()),
            &response.body,
        ))
    }
}
