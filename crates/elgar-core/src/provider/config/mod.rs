//! Provider configuration loaded by Elgar.
//!
//! This file owns static provider settings: local endpoint, model, timeouts,
//! compatibility hints, and per-mode request profiles.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::types::{ProviderBackendKind, ProviderRequestProfile};

pub const LM_STUDIO_PROVIDER_NAME: &str = "lm-studio";
pub const LM_STUDIO_DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234/v1";
pub const LM_STUDIO_DEFAULT_TIMEOUT_MILLIS: u64 = 30_000;

/// Data-only configuration for an LM Studio/OpenAI-compatible local provider.
///
/// This type is intentionally inert: it does not open sockets, perform health
/// checks, route requests, apply actions, or mutate project state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_provider_name")]
    pub provider: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_timeout_millis")]
    pub timeout_millis: u64,
    #[serde(default)]
    pub connect_timeout_millis: Option<u64>,
    #[serde(default)]
    pub read_timeout_millis: Option<u64>,
    #[serde(default)]
    pub write_timeout_millis: Option<u64>,
    #[serde(default)]
    pub request_timeout_millis: Option<u64>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub compatibility: ProviderCompatibility,
    #[serde(default)]
    pub request_modes: BTreeMap<String, ProviderRequestProfile>,
}

impl ProviderConfig {
    /// Convenience constructor for the default local LM Studio provider.
    pub fn lm_studio(model: impl Into<String>) -> Self {
        Self {
            model: Some(model.into()),
            ..Self::default()
        }
    }

    /// Builds the OpenAI-compatible chat-completions URL from `base_url`.
    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Connect timeout, falling back to the overall provider timeout.
    pub fn connect_timeout_millis(&self) -> u64 {
        self.connect_timeout_millis.unwrap_or(self.timeout_millis)
    }

    /// Read timeout, falling back to the overall provider timeout.
    pub fn read_timeout_millis(&self) -> u64 {
        self.read_timeout_millis.unwrap_or(self.timeout_millis)
    }

    /// Write timeout, falling back to the overall provider timeout.
    pub fn write_timeout_millis(&self) -> u64 {
        self.write_timeout_millis.unwrap_or(self.timeout_millis)
    }

    /// Whole-request timeout, falling back to the overall provider timeout.
    pub fn request_timeout_millis(&self) -> u64 {
        self.request_timeout_millis.unwrap_or(self.timeout_millis)
    }

    /// Returns the known model context window if config provided one.
    pub fn configured_context_window_tokens(&self) -> Option<u64> {
        self.compatibility
            .context_window_tokens
            .or(self.context_window_tokens)
    }

    /// Returns whether this provider accepts the OpenAI `developer` role.
    pub fn supports_developer_role(&self) -> bool {
        self.compatibility.supports_developer_role.unwrap_or(false)
    }

    /// Returns the configured backend/options for a named request mode.
    pub fn request_profile_for_mode(&self, request_mode: &str) -> ProviderRequestProfile {
        let default_profile = ProviderRequestProfile {
            backend: ProviderBackendKind::OpenAiChatCompletions,
            stream: None,
            reasoning: None,
            context_length: None,
            stats: None,
            stateful: None,
        };
        self.request_modes
            .get(request_mode)
            .map(|profile| default_profile.clone().overlay(profile))
            .unwrap_or(default_profile)
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_name(),
            base_url: default_base_url(),
            model: None,
            timeout_millis: default_timeout_millis(),
            connect_timeout_millis: None,
            read_timeout_millis: None,
            write_timeout_millis: None,
            request_timeout_millis: None,
            stream: false,
            context_window_tokens: None,
            compatibility: ProviderCompatibility::default(),
            request_modes: BTreeMap::new(),
        }
    }
}

/// Optional model/provider behavior metadata.
///
/// Values here are assertions from local configuration, not registry defaults.
/// Elgar only consumes fields that are explicitly present.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompatibility {
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: ReasoningCompatibility,
    #[serde(default)]
    pub supports_streaming_usage: Option<bool>,
    #[serde(default)]
    pub supports_developer_role: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningCompatibility {
    #[serde(default)]
    pub response_fields: Vec<String>,
    #[serde(default)]
    pub stream_fields: Vec<String>,
}

fn default_provider_name() -> String {
    LM_STUDIO_PROVIDER_NAME.to_string()
}

fn default_base_url() -> String {
    LM_STUDIO_DEFAULT_BASE_URL.to_string()
}

fn default_timeout_millis() -> u64 {
    LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
}

#[cfg(test)]
mod tests;
