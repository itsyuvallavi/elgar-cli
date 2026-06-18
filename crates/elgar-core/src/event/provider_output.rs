//! Provider output text, tool calls, thinking, and request metrics.

use serde::{Deserialize, Serialize};

use crate::{
    provider::{ChatToolCall, ProviderBackendKind, ProviderReasoningLevel},
    token_accounting::ProviderTokenUsage,
};

/// Text returned by a provider.
///
/// This may contain suggestions or claims, but it is not proof that anything
/// happened outside the provider response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOutput {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ProviderMetrics>,
}

impl ProviderOutput {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: Vec::new(),
            thinking: None,
            metrics: None,
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ChatToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn with_thinking(mut self, thinking: impl Into<String>) -> Self {
        self.thinking = Some(thinking.into());
        self
    }

    pub fn with_metrics(mut self, metrics: ProviderMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn has_thinking(&self) -> bool {
        self.thinking
            .as_ref()
            .is_some_and(|thinking| !thinking.is_empty())
    }

    pub fn thinking_chars(&self) -> usize {
        self.thinking
            .as_ref()
            .map(|thinking| thinking.chars().count())
            .unwrap_or(0)
    }
}

/// Provider-owned facts about one provider request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetrics {
    pub request_id: String,
    pub model: Option<String>,
    pub stream: bool,
    pub message_count: usize,
    pub serialized_request_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<ProviderBackendKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ProviderReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_request_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_supports_reasoning_control: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_time_to_first_token_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tokens_per_second_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ProviderTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_chunk_latency_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_done_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chunk_to_done_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_to_finish_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_millis: Option<u64>,
}

impl ProviderMetrics {
    pub fn new(
        request_id: impl Into<String>,
        model: Option<String>,
        stream: bool,
        message_count: usize,
        serialized_request_bytes: usize,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            model,
            stream,
            message_count,
            serialized_request_bytes,
            backend: None,
            reasoning: None,
            context_length: None,
            stats: None,
            reasoning_request_format: None,
            provider_supports_reasoning_control: None,
            provider_time_to_first_token_millis: None,
            provider_tokens_per_second_milli: None,
            reasoning_output_tokens: None,
            usage: None,
            first_chunk_latency_millis: None,
            stream_done_millis: None,
            last_chunk_to_done_millis: None,
            done_to_finish_millis: None,
            total_duration_millis: None,
        }
    }
}
