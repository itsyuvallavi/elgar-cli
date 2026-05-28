use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::event::ProviderOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    Developer,
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub content: String,
    #[serde(
        default,
        alias = "reasoning_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning: Option<String>,
    #[serde(
        default,
        alias = "thinking_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub thinking: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatRole::Assistant, content)
    }

    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning: None,
            thinking: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            reasoning: None,
            thinking: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ChatToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn explicit_thinking(&self) -> Option<String> {
        let thinking = [self.reasoning.as_deref(), self.thinking.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");

        (!thinking.is_empty()).then_some(thinking)
    }
}

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_nullable_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ChatToolCallFunction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ChatToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatToolChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: ChatToolType,
    pub function: ChatToolFunctionDefinition,
}

impl ChatToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: ChatToolType::Function,
            function: ChatToolFunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatToolType {
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatToolChoice {
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: Option<u32>,
    pub message: Option<ChatMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorResponse {
    pub error: ProviderErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub code: Option<String>,
}

/// Request metadata the controller can record before a provider call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestMetadata {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
}

impl ProviderRequestMetadata {
    pub fn new(
        provider: impl Into<String>,
        model: Option<String>,
        request_id: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model,
            request_id: request_id.into(),
        }
    }
}

/// Minimal provider surface consumed by the controller.
///
/// Implementations may call a live provider, a deterministic stub, or a test
/// double. The returned text remains provider suggestion only.
pub trait ControllerProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata;

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError>;

    fn chat_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat(prompt)
    }

    fn chat_with_tools_with_metadata(
        &self,
        _prompt: &str,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        Err(ProviderError::configuration(
            "provider does not support tool-enabled chat",
        ))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        let prompt = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, ChatRole::User))
            .map(|message| message.content.as_str())
            .unwrap_or_default();
        self.chat_with_tools_with_metadata(prompt, metadata, tools)
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        let prompt = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, ChatRole::User))
            .map(|message| message.content.as_str())
            .unwrap_or_default();
        self.chat_with_metadata(prompt, metadata)
    }

    fn chat_messages_without_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat_messages_with_metadata(messages, metadata)
    }

    fn chat_stream(
        &self,
        prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        let output = self.chat(prompt)?;
        if let Some(thinking) = output.thinking.as_ref() {
            on_chunk(ProviderStreamChunk::Reasoning(thinking.clone()));
        }
        on_chunk(ProviderStreamChunk::Text(output.text.clone()));
        Ok(output)
    }

    fn chat_stream_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat_stream(prompt, on_chunk)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamChunk {
    Reasoning(String),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderErrorKind {
    Configuration,
    ResponseParse,
    Provider,
    EmptyResponse,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub status_code: Option<u16>,
    pub code: Option<String>,
}

impl ProviderError {
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Configuration, message)
    }

    pub fn response_parse(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::ResponseParse, message)
    }

    pub fn provider(
        message: impl Into<String>,
        status_code: Option<u16>,
        code: Option<String>,
    ) -> Self {
        Self::new(ProviderErrorKind::Provider, message)
            .with_status(status_code)
            .with_code(code)
    }

    pub fn empty_response(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::EmptyResponse, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Network, message)
    }

    fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status_code: None,
            code: None,
        }
    }

    pub(crate) fn with_status(mut self, status_code: Option<u16>) -> Self {
        self.status_code = status_code;
        self
    }

    fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status_code, self.code.as_deref()) {
            (Some(status), Some(code)) => write!(
                formatter,
                "{:?} provider error ({status}, {code}): {}",
                self.kind, self.message
            ),
            (Some(status), None) => {
                write!(
                    formatter,
                    "{:?} provider error ({status}): {}",
                    self.kind, self.message
                )
            }
            (None, Some(code)) => write!(
                formatter,
                "{:?} provider error ({code}): {}",
                self.kind, self.message
            ),
            (None, None) => write!(
                formatter,
                "{:?} provider error: {}",
                self.kind, self.message
            ),
        }
    }
}

impl std::error::Error for ProviderError {}
