use crate::provider::{
    config::ProviderConfig,
    types::{ChatMessage, ChatRequest, ChatToolChoice, ChatToolDefinition, ProviderError},
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

pub fn format_chat_request_with_tools(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatToolDefinition>,
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
        stream: config.stream,
        temperature: None,
        tools,
        tool_choice,
    })
}

#[cfg(test)]
pub(crate) fn elgar_controller_messages(prompt: &str) -> Vec<ChatMessage> {
    elgar_controller_messages_for_config(&ProviderConfig::default(), prompt)
}

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
    let request = format_chat_request_with_tools(config, messages, tools)?;
    let body = serde_json::to_string(&request)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    Ok((request, body))
}
