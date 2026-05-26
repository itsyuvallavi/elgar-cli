use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    provider::types::{
        ChatMessage, ChatRole, ChatToolDefinition, ControllerProvider, ProviderError,
        ProviderRequestMetadata,
    },
};

/// No-network provider stub for controller and UI tests.
///
/// It deliberately does not infer tool calls from natural-language text.
/// Tests that need tools should use scripted providers so the provider
/// boundary is explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStub {
    pub provider: String,
    pub model: Option<String>,
}

impl ProviderStub {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn ask(&self, prompt: &str) -> ProviderStubResponse {
        ProviderStubResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_id: "stub-request-1".to_string(),
            output: ProviderOutput::new(stub_response_text(visible_user_prompt(prompt))),
        }
    }

    pub fn ask_with_tools(&self, prompt: &str) -> ProviderStubResponse {
        ProviderStubResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_id: "stub-request-1".to_string(),
            output: ProviderOutput::new(stub_response_text(visible_user_prompt(prompt))),
        }
    }
}

impl ControllerProvider for ProviderStub {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(self.provider.clone(), self.model.clone(), "stub-request-1")
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(self.ask(prompt).output)
    }

    fn chat_with_tools_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        Ok(self.ask_with_tools(prompt).output)
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        if messages
            .iter()
            .any(|message| matches!(message.role, ChatRole::Tool))
        {
            return Ok(ProviderOutput::new("Done."));
        }

        let prompt = latest_user_message(&messages);
        Ok(ProviderOutput::new(stub_response_text(prompt)))
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        let prompt = latest_user_message(&messages);
        Ok(ProviderOutput::new(stub_response_text(prompt)))
    }
}

impl Default for ProviderStub {
    fn default() -> Self {
        Self::new("stub-provider")
    }
}

fn latest_user_message(messages: &[ChatMessage]) -> &str {
    messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, ChatRole::User))
        .map(|message| message.content.as_str())
        .unwrap_or_default()
}

fn visible_user_prompt(prompt: &str) -> &str {
    prompt
        .rsplit_once("User request:\n")
        .map(|(_context, request)| request.trim())
        .unwrap_or_else(|| prompt.trim())
}

fn stub_response_text(prompt: &str) -> String {
    format!(
        "stub provider response (no-network) to: {}. No live provider call was made.",
        prompt.trim()
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStubResponse {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
    pub output: ProviderOutput,
}

#[cfg(test)]
mod tests {
    use super::ProviderStub;
    use crate::provider::{ControllerProvider, ProviderRequestMetadata};

    #[test]
    fn stub_tool_request_does_not_infer_tool_calls_from_text() {
        let output = ProviderStub::default()
            .chat_with_tools_with_metadata(
                "create a folder called demo",
                &ProviderRequestMetadata::new("stub-provider", None, "request-1"),
                Vec::new(),
            )
            .unwrap();

        assert!(output.tool_calls.is_empty());
        assert!(output.text.contains("No live provider call was made."));
    }

    #[test]
    fn stub_plain_request_returns_no_network_text() {
        let output = ProviderStub::default().ask("hello").output;

        assert_eq!(output.tool_calls.len(), 0);
        assert!(output.text.contains("hello"));
    }
}
