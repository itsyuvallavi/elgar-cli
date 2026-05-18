use std::fmt;

use serde::{Deserialize, Serialize};

use crate::event::ProviderOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
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
        }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
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
