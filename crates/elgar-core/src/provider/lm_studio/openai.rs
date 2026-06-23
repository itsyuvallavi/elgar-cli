//! OpenAI-compatible LM Studio chat backend.
//!
//! This path sends requests to `/v1/chat/completions`. It supports normal
//! non-streaming chat, streaming chat, and the old tool-capable request shape.

mod chat_streaming;
mod metrics;
mod streaming;
mod tool_streaming;

use std::time::Instant;

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        http::{post_json_cancelable, HttpEndpoint},
        types::{
            ChatMessage, ChatToolDefinition, ProviderBackendKind, ProviderError,
            ProviderRequestProfile,
        },
        ProviderCancelToken,
    },
};

use super::{
    format::format_chat_request_body_with_tools_and_profile,
    parse::{
        parse_chat_response_json_with_metrics, parse_chat_stream_response,
        parse_provider_error_json,
    },
};

pub(super) use chat_streaming::{
    chat_lm_studio_streaming_with_request_id, chat_lm_studio_streaming_with_request_id_cancelable,
};
#[cfg(not(test))]
use metrics::metrics_for_request;
#[cfg(test)]
pub(super) use metrics::metrics_for_request;
use metrics::{duration_millis, http_timeouts};
pub(super) use tool_streaming::chat_lm_studio_with_tools_streaming_with_request_id_cancelable;

pub(super) fn chat_lm_studio_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: Option<&ProviderRequestProfile>,
) -> Result<ProviderOutput, ProviderError> {
    chat_lm_studio_with_request_id_cancelable(
        config,
        messages,
        request_id,
        profile,
        &ProviderCancelToken::new(),
    )
}

pub(super) fn chat_lm_studio_with_request_id_cancelable(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: Option<&ProviderRequestProfile>,
    cancel: &ProviderCancelToken,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    cancel.error_if_canceled()?;
    let (request, body) =
        format_chat_request_body_with_tools_and_profile(config, messages, Vec::new(), profile)?;
    let mut metrics = metrics_for_request(
        request_id,
        &request,
        body.len(),
        profile,
        config.compatibility.reasoning.request_format,
    );
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
    let response = post_json_cancelable(&endpoint, &body, http_timeouts(config), cancel)?;

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
pub(super) fn chat_lm_studio_with_tools_with_request_id_cancelable(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    tools: Vec<ChatToolDefinition>,
    profile: Option<&ProviderRequestProfile>,
    cancel: &ProviderCancelToken,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    cancel.error_if_canceled()?;
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
    let mut metrics = metrics_for_request(
        request_id,
        &request,
        body.len(),
        profile,
        config.compatibility.reasoning.request_format,
    );
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
    let response = post_json_cancelable(&endpoint, &body, http_timeouts(&config), cancel)?;

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

/// Keeps only controls represented by OpenAI-compatible chat requests.
pub(super) fn openai_chat_profile(
    profile: Option<&ProviderRequestProfile>,
) -> Option<ProviderRequestProfile> {
    profile.map(|profile| ProviderRequestProfile {
        backend: ProviderBackendKind::OpenAiChatCompletions,
        stream: profile.stream,
        reasoning: None,
        context_length: None,
        stats: profile.stats,
        stateful: None,
    })
}
