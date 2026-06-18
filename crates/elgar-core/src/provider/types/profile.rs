//! Provider backend and request-mode profile types.
//!
//! Profiles let config select an OpenAI-compatible request mode and optional
//! controls like reasoning level, context length, stats, and streaming.

use serde::{Deserialize, Serialize};

/// Provider backend selected for a specific request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderBackendKind {
    #[serde(rename = "openai_chat_completions")]
    OpenAiChatCompletions,
    #[serde(rename = "openai_responses_probe")]
    OpenAiResponsesProbe,
}

impl Default for ProviderBackendKind {
    fn default() -> Self {
        Self::OpenAiChatCompletions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderReasoningLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    On,
}

/// Optional per-request controls loaded from provider config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestProfile {
    #[serde(default)]
    pub backend: ProviderBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ProviderReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stateful: Option<bool>,
}

impl Default for ProviderRequestProfile {
    fn default() -> Self {
        Self {
            backend: ProviderBackendKind::OpenAiChatCompletions,
            stream: None,
            reasoning: None,
            context_length: None,
            stats: None,
            stateful: None,
        }
    }
}

impl ProviderRequestProfile {
    /// Merges a mode-specific profile over the default profile.
    pub fn overlay(mut self, override_profile: &ProviderRequestProfile) -> Self {
        self.backend = override_profile.backend;
        self.stream = override_profile.stream.or(self.stream);
        self.reasoning = override_profile.reasoning.or(self.reasoning);
        self.context_length = override_profile.context_length.or(self.context_length);
        self.stats = override_profile.stats.or(self.stats);
        self.stateful = override_profile.stateful.or(self.stateful);
        self
    }
}
