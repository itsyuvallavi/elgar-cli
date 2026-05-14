use serde::{Deserialize, Serialize};

use crate::event::ProviderOutput;

/// Deterministic provider stub for no-model controller tests.
///
/// This stub never performs network calls, filesystem writes, shell commands,
/// action transitions, or any other side effect.
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
            output: ProviderOutput::new(format!("stub provider response to: {}", prompt.trim())),
        }
    }
}

impl Default for ProviderStub {
    fn default() -> Self {
        Self::new("stub-provider")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStubResponse {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
    pub output: ProviderOutput,
}
