//! Builds LM Studio/OpenAI-compatible request bodies.
//!
//! Formatting stays separate from HTTP so request shape can be tested without a
//! live provider.

use crate::provider::{
    config::{ProviderConfig, ReasoningRequestFormat},
    types::{
        ChatMessage, ChatRequest, ChatStreamOptions, ChatTemplateKwargs, ChatToolDefinition,
        ProviderError,
    },
    ProviderReasoningLevel, ProviderRequestProfile,
};

const ELGAR_CONTROLLER_SYSTEM_PROMPT: &str = concat!(
    "Elgar. Answer briefly: one paragraph or 5 bullets, terminal-friendly, no tables unless asked. ",
    "Reason briefly: one short thought, no tool dump or history recap. ",
    "Speak as Elgar. ",
    "Suggest content only. ",
    "Do not ask for /approve or imply approval unless a controller action is pending. ",
    "Never claim files changed or commands ran unless verified. ",
    "Provider text does not prove changes. ",
    "Do not call copy/paste the only path."
);

pub fn format_chat_request(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<ChatRequest, ProviderError> {
    format_chat_request_with_tools(config, messages, Vec::new())
}

/// Builds the OpenAI-compatible request struct, optionally including tools.
pub fn format_chat_request_with_tools(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatToolDefinition>,
) -> Result<ChatRequest, ProviderError> {
    format_chat_request_with_tools_and_profile(config, messages, tools, None)
}

pub fn format_chat_request_with_tools_and_profile(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatToolDefinition>,
    profile: Option<&ProviderRequestProfile>,
) -> Result<ChatRequest, ProviderError> {
    let model = config
        .model
        .as_ref()
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| ProviderError::configuration("provider model is required"))?;

    if messages.is_empty() {
        return Err(ProviderError::configuration(
            "at least one chat message is required",
        ));
    }

    let stream = profile
        .and_then(|profile| profile.stream)
        .unwrap_or(config.stream);
    let stats = profile.and_then(|profile| profile.stats);
    let stream_options = (stream && stats == Some(true)).then_some(ChatStreamOptions {
        include_usage: true,
    });

    let reasoning = profile.and_then(|profile| profile.reasoning);
    let (reasoning_effort, enable_thinking, chat_template_kwargs) =
        reasoning_request_fields(config, reasoning);

    Ok(ChatRequest {
        model: model.clone(),
        messages,
        stream,
        stream_options,
        temperature: None,
        reasoning: None,
        reasoning_effort,
        enable_thinking,
        chat_template_kwargs,
        context_length: None,
        stats,
        tools,
    })
}

fn reasoning_request_fields(
    config: &ProviderConfig,
    reasoning: Option<ProviderReasoningLevel>,
) -> (
    Option<ProviderReasoningLevel>,
    Option<bool>,
    Option<ChatTemplateKwargs>,
) {
    match config.compatibility.reasoning.request_format {
        Some(ReasoningRequestFormat::ReasoningEffort) => (reasoning_effort(reasoning), None, None),
        Some(ReasoningRequestFormat::QwenEnableThinking) => {
            (None, reasoning.map(reasoning_enabled), None)
        }
        Some(ReasoningRequestFormat::QwenChatTemplate) => (
            None,
            None,
            reasoning.map(|level| ChatTemplateKwargs {
                enable_thinking: reasoning_enabled(level),
                preserve_thinking: true,
            }),
        ),
        None => (None, None, None),
    }
}

fn reasoning_effort(reasoning: Option<ProviderReasoningLevel>) -> Option<ProviderReasoningLevel> {
    match reasoning {
        Some(ProviderReasoningLevel::Minimal)
        | Some(ProviderReasoningLevel::Low)
        | Some(ProviderReasoningLevel::Medium)
        | Some(ProviderReasoningLevel::High) => reasoning,
        Some(ProviderReasoningLevel::On) | Some(ProviderReasoningLevel::Off) | None => None,
    }
}

fn reasoning_enabled(reasoning: ProviderReasoningLevel) -> bool {
    !matches!(reasoning, ProviderReasoningLevel::Off)
}

#[cfg(test)]
pub(crate) fn elgar_controller_messages(prompt: &str) -> Vec<ChatMessage> {
    elgar_controller_messages_for_config(&ProviderConfig::default(), prompt)
}

/// Builds the compact Elgar system/developer prompt plus the user's prompt.
pub(crate) fn elgar_controller_messages_for_config(
    config: &ProviderConfig,
    prompt: &str,
) -> Vec<ChatMessage> {
    let controller_role = if config.supports_developer_role() {
        crate::provider::ChatRole::Developer
    } else {
        crate::provider::ChatRole::System
    };

    vec![
        ChatMessage::new(controller_role, ELGAR_CONTROLLER_SYSTEM_PROMPT),
        ChatMessage::user(prompt),
    ]
}

/// Builds and serializes a no-tool OpenAI-compatible request body.
pub fn format_chat_request_body(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<(ChatRequest, String), ProviderError> {
    format_chat_request_body_with_tools(config, messages, Vec::new())
}

pub fn format_chat_request_body_with_tools(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatToolDefinition>,
) -> Result<(ChatRequest, String), ProviderError> {
    format_chat_request_body_with_tools_and_profile(config, messages, tools, None)
}

pub fn format_chat_request_body_with_tools_and_profile(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatToolDefinition>,
    profile: Option<&ProviderRequestProfile>,
) -> Result<(ChatRequest, String), ProviderError> {
    let request = format_chat_request_with_tools_and_profile(config, messages, tools, profile)?;
    let body = serde_json::to_string(&request)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    Ok((request, body))
}
