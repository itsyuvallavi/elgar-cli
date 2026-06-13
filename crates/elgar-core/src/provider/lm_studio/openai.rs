//! OpenAI-compatible LM Studio chat backend.
//!
//! This path sends requests to `/v1/chat/completions`. It supports normal
//! non-streaming chat, streaming chat, and the old tool-capable request shape.

mod metrics;
mod streaming;

use std::time::Instant;

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        http::{post_json, post_json_streaming, HttpEndpoint},
        types::{
            ChatMessage, ChatToolDefinition, ProviderBackendKind, ProviderError,
            ProviderRequestProfile, ProviderStreamChunk,
        },
    },
};

use super::{
    format::{format_chat_request_body, format_chat_request_body_with_tools_and_profile},
    parse::{
        parse_chat_response_json_with_metrics, parse_chat_stream_response,
        parse_provider_error_json, provider_output_from_stream_parts,
    },
};

#[cfg(not(test))]
use metrics::metrics_for_request;
#[cfg(test)]
pub(super) use metrics::metrics_for_request;
use metrics::{duration_millis, http_timeouts};
use streaming::{emit_output_chunks, StreamingOutputParts};

pub(super) fn chat_lm_studio_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: Option<&ProviderRequestProfile>,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    let (request, body) =
        format_chat_request_body_with_tools_and_profile(config, messages, Vec::new(), profile)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len(), profile);
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    log::debug!(
        "lm_studio_request_start request_id={} endpoint={} model={} messages={} tools=0 stream={} backend={} bytes={}",
        request_id,
        config.chat_completions_url(),
        request.model,
        request.messages.len(),
        request.stream,
        metrics
            .backend
            .as_ref()
            .map(|backend| format!("{backend:?}"))
            .unwrap_or_else(|| "n/a".to_string()),
        body.len()
    );
    let response = post_json(&endpoint, &body, http_timeouts(config))?;

    if response.status_code.is_success() {
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        log::debug!(
            "lm_studio_request_finish request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            metrics.total_duration_millis.unwrap_or(0),
            response.body.len()
        );
        if request.stream {
            let output = parse_chat_stream_response(&response.body)?;
            Ok(output.with_metrics(metrics))
        } else {
            parse_chat_response_json_with_metrics(&response.body, Some(metrics))
        }
    } else {
        log::warn!(
            "lm_studio_request_error request_id={} status={} duration_ms={} response_bytes={}",
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

/// Sends an OpenAI-compatible request with tool definitions.
///
/// Harness tool decisions use this OpenAI-compatible boundary.
pub(super) fn chat_lm_studio_with_tools_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    tools: Vec<ChatToolDefinition>,
    profile: Option<&ProviderRequestProfile>,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    let mut config = config.clone();
    config.stream = false;
    if matches!(
        profile.map(|profile| profile.backend),
        Some(ProviderBackendKind::OpenAiResponsesProbe)
    ) {
        return Err(ProviderError::configuration(
            "tool-enabled requests require openai_chat_completions backend",
        ));
    }
    let (request, body) =
        format_chat_request_body_with_tools_and_profile(&config, messages, tools, profile)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len(), profile);
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    log::debug!(
        "lm_studio_request_start request_id={} endpoint={} model={} messages={} tools={} stream={} backend={} bytes={}",
        request_id,
        config.chat_completions_url(),
        request.model,
        request.messages.len(),
        request.tools.len(),
        request.stream,
        metrics
            .backend
            .as_ref()
            .map(|backend| format!("{backend:?}"))
            .unwrap_or_else(|| "n/a".to_string()),
        body.len()
    );
    let response = post_json(&endpoint, &body, http_timeouts(&config))?;

    if response.status_code.is_success() {
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        log::debug!(
            "lm_studio_request_finish request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            metrics.total_duration_millis.unwrap_or(0),
            response.body.len()
        );
        parse_chat_response_json_with_metrics(&response.body, Some(metrics))
    } else {
        log::warn!(
            "lm_studio_request_error request_id={} status={} duration_ms={} response_bytes={}",
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

/// Sends an OpenAI-compatible streaming request and emits chunks as they arrive.
pub(super) fn chat_lm_studio_streaming_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> Result<ProviderOutput, ProviderError> {
    if !config.stream {
        let output = chat_lm_studio_with_request_id(config, messages, request_id, None)?;
        emit_output_chunks(&output, on_chunk);
        return Ok(output);
    }

    let started = Instant::now();
    let (request, body) = format_chat_request_body(config, messages)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len(), None);
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
    let response =
        post_json_streaming(&endpoint, &body, http_timeouts(config), &mut |body_chunk| {
            parts.push_body_chunk(body_chunk, &mut |chunk| {
                if metrics.first_chunk_latency_millis.is_none() {
                    metrics.first_chunk_latency_millis = Some(duration_millis(started.elapsed()));
                }
                on_chunk(chunk);
            })
        })?;

    if response.status_code.is_success() {
        parts.finish(&mut |chunk| {
            if metrics.first_chunk_latency_millis.is_none() {
                metrics.first_chunk_latency_millis = Some(duration_millis(started.elapsed()));
            }
            on_chunk(chunk);
        })?;
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        log::debug!(
            "lm_studio_stream_request_finish request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            metrics.total_duration_millis.unwrap_or(0),
            response.body.len()
        );
        let output = provider_output_from_stream_parts(parts.text, parts.thinking)?;
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

/// Keeps only controls represented by OpenAI-compatible chat requests.
pub(super) fn openai_chat_profile(
    profile: Option<&ProviderRequestProfile>,
) -> Option<ProviderRequestProfile> {
    profile.map(|profile| ProviderRequestProfile {
        backend: ProviderBackendKind::OpenAiChatCompletions,
        stream: profile.stream,
        reasoning: None,
        context_length: None,
        stats: None,
        stateful: None,
    })
}
