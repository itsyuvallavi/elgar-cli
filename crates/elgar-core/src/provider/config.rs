use serde::{Deserialize, Serialize};

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
}

impl ProviderConfig {
    pub fn lm_studio(model: impl Into<String>) -> Self {
        Self {
            model: Some(model.into()),
            ..Self::default()
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    pub fn connect_timeout_millis(&self) -> u64 {
        self.connect_timeout_millis.unwrap_or(self.timeout_millis)
    }

    pub fn read_timeout_millis(&self) -> u64 {
        self.read_timeout_millis.unwrap_or(self.timeout_millis)
    }

    pub fn write_timeout_millis(&self) -> u64 {
        self.write_timeout_millis.unwrap_or(self.timeout_millis)
    }

    pub fn request_timeout_millis(&self) -> u64 {
        self.request_timeout_millis.unwrap_or(self.timeout_millis)
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
        }
    }
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
mod tests {
    use serde_json::json;

    use super::{
        ProviderConfig, LM_STUDIO_DEFAULT_BASE_URL, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS,
        LM_STUDIO_PROVIDER_NAME,
    };

    #[test]
    fn provider_config_defaults_to_lm_studio_local_endpoint() {
        let config = ProviderConfig::default();

        assert_eq!(config.provider, LM_STUDIO_PROVIDER_NAME);
        assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(config.model, None);
        assert_eq!(config.timeout_millis, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS);
        assert_eq!(
            config.connect_timeout_millis(),
            LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
        );
        assert_eq!(
            config.read_timeout_millis(),
            LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
        );
        assert_eq!(
            config.write_timeout_millis(),
            LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
        );
        assert_eq!(
            config.request_timeout_millis(),
            LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
        );
        assert!(!config.stream);
        assert_eq!(
            config.chat_completions_url(),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

    #[test]
    fn provider_config_deserializes_with_defaults() {
        let config: ProviderConfig = serde_json::from_value(json!({
            "model": "local-model"
        }))
        .unwrap();

        assert_eq!(config.provider, LM_STUDIO_PROVIDER_NAME);
        assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(config.model.as_deref(), Some("local-model"));
        assert_eq!(config.timeout_millis, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS);
        assert_eq!(config.connect_timeout_millis, None);
        assert_eq!(config.read_timeout_millis, None);
        assert_eq!(config.write_timeout_millis, None);
        assert_eq!(config.request_timeout_millis, None);
        assert!(!config.stream);
    }

    #[test]
    fn provider_config_deserializes_phase_timeouts() {
        let config: ProviderConfig = serde_json::from_value(json!({
            "model": "local-model",
            "timeout_millis": 30_000,
            "connect_timeout_millis": 1_000,
            "read_timeout_millis": 120_000,
            "write_timeout_millis": 2_000,
            "request_timeout_millis": 180_000
        }))
        .unwrap();

        assert_eq!(config.connect_timeout_millis(), 1_000);
        assert_eq!(config.read_timeout_millis(), 120_000);
        assert_eq!(config.write_timeout_millis(), 2_000);
        assert_eq!(config.request_timeout_millis(), 180_000);
    }

    #[test]
    fn provider_config_deserializes_opt_in_streaming() {
        let config: ProviderConfig = serde_json::from_value(json!({
            "model": "local-model",
            "stream": true
        }))
        .unwrap();

        assert!(config.stream);
    }

    #[test]
    fn provider_config_trims_chat_url_slash() {
        let config = ProviderConfig {
            base_url: "http://127.0.0.1:1234/v1/".to_string(),
            ..ProviderConfig::lm_studio("loaded-model")
        };

        assert_eq!(
            config.chat_completions_url(),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }
}
