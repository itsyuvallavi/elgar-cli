//! LM Studio native chat backend.
//!
//! This path uses LM Studio's `/api/v1/chat` endpoint for simple no-tool
//! requests. It can expose native stats such as tokens per second and
//! time-to-first-token.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{
    event::{ProviderMetrics, ProviderOutput},
    provider::{
        config::ProviderConfig,
        http::{post_json, HttpEndpoint, HttpTimeouts},
        types::{
            ChatMessage, ChatRole, ProviderBackendKind, ProviderError, ProviderRequestProfile,
        },
    },
};

use super::parse::{parse_native_chat_response_json_with_metrics, parse_provider_error_json};

pub(super) fn profile_allows_native_no_tool(profile: Option<&ProviderRequestProfile>) -> bool {
    matches!(
        profile.map(|profile| profile.backend),
        Some(ProviderBackendKind::LmStudioNativeChat)
    )
}

/// Returns whether messages can be represented by LM Studio native chat.
pub(super) fn messages_are_native_no_tool_safe(messages: &[ChatMessage]) -> bool {
    messages.iter().all(|message| {
        matches!(
            message.role,
            ChatRole::Developer | ChatRole::System | ChatRole::User
        )
    })
}

/// Sends one no-tool request through LM Studio's native chat endpoint.
pub(super) fn chat_lm_studio_native_no_tool_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    profile: &ProviderRequestProfile,
) -> Result<ProviderOutput, ProviderError> {
    if profile.stream.unwrap_or(false) {
        return Err(ProviderError::configuration(
            "native LM Studio streaming is not enabled for no-tool requests yet",
        ));
    }

    let started = Instant::now();
    let request = native_request_from_messages(config, messages, profile)?;
    let body = serde_json::to_string(&request)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    let mut metrics = ProviderMetrics::new(
        request_id,
        Some(request.model.clone()),
        request.stream,
        native_message_count(&request),
        body.len(),
    );
    metrics.backend = Some(ProviderBackendKind::LmStudioNativeChat);
    metrics.reasoning = profile.reasoning;
    metrics.context_length = profile.context_length;
    metrics.stats = profile.stats;

    let endpoint = HttpEndpoint::parse(&native_chat_url(config))?;
    let native_url = native_chat_url(config);
    log::debug!(
        "lm_studio_native_request_start request_id={} endpoint={} model={} messages={} stream={} backend={:?} bytes={}",
        request_id,
        native_url,
        request.model,
        metrics.message_count,
        request.stream,
        ProviderBackendKind::LmStudioNativeChat,
        body.len()
    );
    let response = post_json(&endpoint, &body, http_timeouts(config))?;

    if response.status_code.is_success() {
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        log::debug!(
            "lm_studio_native_request_finish request_id={} status={} duration_ms={} response_bytes={}",
            request_id,
            response.status_code.as_u16(),
            metrics.total_duration_millis.unwrap_or(0),
            response.body.len()
        );
        parse_native_chat_response_json_with_metrics(&response.body, Some(metrics))
    } else {
        log::warn!(
            "lm_studio_native_request_error request_id={} status={} duration_ms={} response_bytes={}",
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

fn native_chat_url(config: &ProviderConfig) -> String {
    let trimmed = config.base_url.trim_end_matches('/');
    let root = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    format!("{}/api/v1/chat", root.trim_end_matches('/'))
}

/// Converts normal chat messages into LM Studio native input/system fields.
fn native_request_from_messages(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    profile: &ProviderRequestProfile,
) -> Result<NativeChatRequest, ProviderError> {
    let model = config
        .model
        .as_ref()
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| ProviderError::configuration("provider model is required"))?;

    let mut system_prompt = String::new();
    let mut input = String::new();
    for message in messages {
        match message.role {
            ChatRole::Developer | ChatRole::System => {
                if !system_prompt.is_empty() {
                    system_prompt.push_str("\n\n");
                }
                system_prompt.push_str(message.content.trim());
            }
            ChatRole::User => {
                if !input.is_empty() {
                    input.push_str("\n\n");
                }
                input.push_str(message.content.trim());
            }
            ChatRole::Assistant | ChatRole::Tool => {
                return Err(ProviderError::configuration(
                    "native LM Studio chat cannot represent assistant/tool history",
                ));
            }
        }
    }

    if input.trim().is_empty() {
        return Err(ProviderError::configuration(
            "native LM Studio chat requires user input",
        ));
    }

    Ok(NativeChatRequest {
        model: model.clone(),
        input,
        system_prompt: (!system_prompt.trim().is_empty()).then_some(system_prompt),
        stream: profile.stream.unwrap_or(false),
        temperature: None,
        reasoning: profile.reasoning,
        context_length: profile.context_length,
    })
}

fn native_message_count(request: &NativeChatRequest) -> usize {
    usize::from(request.system_prompt.is_some()) + 1
}

fn http_timeouts(config: &ProviderConfig) -> HttpTimeouts {
    HttpTimeouts::from_millis(
        config.connect_timeout_millis(),
        config.read_timeout_millis(),
        config.write_timeout_millis(),
        config.request_timeout_millis(),
    )
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeChatRequest {
    model: String,
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<crate::provider::ProviderReasoningLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_length: Option<u64>,
}
