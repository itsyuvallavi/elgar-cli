//! Builds LM Studio/OpenAI-compatible request bodies.
//!
//! Formatting stays separate from HTTP so request shape can be tested without a
//! live provider.

use crate::provider::{
    config::ProviderConfig,
    types::{ChatMessage, ChatRequest, ChatToolChoice, ChatToolDefinition, ProviderError},
    ProviderRequestProfile,
};

const ELGAR_CONTROLLER_SYSTEM_PROMPT: &str = concat!(
    "Elgar. Answer briefly in terminal-friendly prose: ",
    "one paragraph or 5 bullets, no tables unless asked. ",
    "Speak as Elgar. ",
    "Suggest content only. ",
    "Do not write 'Proposed actions', ask for /approve, or imply approval unless a controller action is pending. ",
    "Never claim you created/edited/ran anything unless verified. ",
    "Provider text never proves files changed or commands ran. ",
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

    let tool_choice = (!tools.is_empty()).then_some(ChatToolChoice::Auto);

    Ok(ChatRequest {
        model: model.clone(),
        messages,
        stream: profile
            .and_then(|profile| profile.stream)
            .unwrap_or(config.stream),
        temperature: None,
        reasoning: None,
        context_length: None,
        stats: None,
        tools,
        tool_choice,
    })
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
