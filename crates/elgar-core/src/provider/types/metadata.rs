//! Provider request metadata recorded around model calls.
//!
//! Metadata gives Elgar a stable request id plus provider/model labels before
//! the provider call starts.

use serde::{Deserialize, Serialize};

use super::profile::ProviderRequestProfile;

/// Request metadata the controller can record before a provider call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestMetadata {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProviderRequestProfile>,
}

impl ProviderRequestMetadata {
    /// Creates the metadata Elgar records before sending a provider request.
    pub fn new(
        provider: impl Into<String>,
        model: Option<String>,
        request_id: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model,
            request_id: request_id.into(),
            profile: None,
        }
    }

    /// Attaches the selected backend/profile for this request.
    pub fn with_profile(mut self, profile: ProviderRequestProfile) -> Self {
        self.profile = Some(profile);
        self
    }
}
