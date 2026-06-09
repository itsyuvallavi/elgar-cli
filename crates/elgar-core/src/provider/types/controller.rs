//! Provider trait consumed by Elgar runtime code.
//!
//! Concrete providers implement this trait. The default methods keep simple
//! providers small while still supporting message-based and streaming calls.

use crate::event::ProviderOutput;

use super::{
    chat::{ChatMessage, ChatRole},
    error::ProviderError,
    metadata::ProviderRequestMetadata,
    stream::ProviderStreamChunk,
    tools::ChatToolDefinition,
};

/// Minimal provider surface consumed by the controller/runtime.
pub trait ControllerProvider {
    /// Creates a request id and provider/model labels for the next call.
    fn request_metadata(&self) -> ProviderRequestMetadata;

    /// Creates metadata for a named request mode such as harness decision or synthesis.
    fn request_metadata_for_mode(&self, _request_mode: &str) -> ProviderRequestMetadata {
        self.request_metadata()
    }

    /// Sends messages and streams chunks if the provider supports live chunks.
    ///
    /// The default implementation falls back to a non-streaming request and
    /// emits the finished reasoning/text as synthetic chunks.
    fn chat_messages_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        let output = self.chat_messages_without_streaming_with_metadata(messages, metadata)?;

        if let Some(thinking) = output.thinking.as_ref() {
            on_chunk(ProviderStreamChunk::Reasoning(thinking.clone()));
        }

        on_chunk(ProviderStreamChunk::Text(output.text.clone()));

        Ok(output)
    }

    /// Sends a simple prompt using the provider's default message handling.
    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError>;

    fn chat_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat(prompt)
    }

    /// Sends a prompt with tool definitions.
    ///
    /// Providers that do not implement tool-enabled chat reject this method by
    /// default.
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

    /// Sends a full message list with request metadata.
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

    /// Sends messages while explicitly avoiding provider streaming.
    fn chat_messages_without_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat_messages_with_metadata(messages, metadata)
    }

    /// Streams a simple prompt using the provider default behavior.
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

    /// Streams a prompt with request metadata.
    fn chat_stream_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        self.chat_stream(prompt, on_chunk)
    }
}
