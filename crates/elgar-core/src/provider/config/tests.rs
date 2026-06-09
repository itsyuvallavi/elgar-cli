//! Tests for provider configuration.
//!
//! These tests keep config parsing and defaults out of the main config file.

use serde_json::json;

use super::{
    ProviderBackendKind, ProviderConfig, LM_STUDIO_DEFAULT_BASE_URL,
    LM_STUDIO_DEFAULT_TIMEOUT_MILLIS, LM_STUDIO_PROVIDER_NAME,
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
    assert_eq!(config.context_window_tokens, None);
    assert_eq!(config.compatibility, Default::default());
    assert!(config.request_modes.is_empty());
    assert_eq!(config.configured_context_window_tokens(), None);
    assert!(!config.supports_developer_role());
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
    assert_eq!(config.compatibility, Default::default());
    assert!(config.request_modes.is_empty());
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
        "stream": true,
        "context_window_tokens": 128000
    }))
    .unwrap();

    assert!(config.stream);
    assert_eq!(config.context_window_tokens, Some(128_000));
    assert_eq!(config.configured_context_window_tokens(), Some(128_000));
}

#[test]
fn provider_config_deserializes_optional_compatibility_metadata() {
    let config: ProviderConfig = serde_json::from_value(json!({
        "model": "local-model",
        "context_window_tokens": 32_000,
        "compatibility": {
            "context_window_tokens": 128_000,
            "reasoning": {
                "response_fields": ["reasoning_content"],
                "stream_fields": ["reasoning_content", "thinking"]
            },
            "supports_streaming_usage": true,
            "supports_developer_role": true
        }
    }))
    .unwrap();

    assert_eq!(config.context_window_tokens, Some(32_000));
    assert_eq!(config.configured_context_window_tokens(), Some(128_000));
    assert_eq!(
        config.compatibility.reasoning.response_fields,
        vec!["reasoning_content"]
    );
    assert_eq!(
        config.compatibility.reasoning.stream_fields,
        vec!["reasoning_content", "thinking"]
    );
    assert_eq!(config.compatibility.supports_streaming_usage, Some(true));
    assert!(config.supports_developer_role());
}

#[test]
fn provider_config_deserializes_request_mode_profiles() {
    let config: ProviderConfig = serde_json::from_value(json!({
        "model": "local-model",
        "request_modes": {
            "harness_synthesis": {
                "backend": "openai_chat_completions",
                "stats": true
            },
            "harness_tool_decision": {
                "backend": "openai_chat_completions"
            }
        }
    }))
    .unwrap();

    let synthesis = config.request_profile_for_mode("harness_synthesis");
    assert_eq!(
        synthesis.backend,
        ProviderBackendKind::OpenAiChatCompletions
    );
    assert_eq!(synthesis.stats, Some(true));

    let tool = config.request_profile_for_mode("harness_tool_decision");
    assert_eq!(tool.backend, ProviderBackendKind::OpenAiChatCompletions);

    let missing = config.request_profile_for_mode("missing_mode");
    assert_eq!(missing.backend, ProviderBackendKind::OpenAiChatCompletions);
    assert_eq!(missing.reasoning, None);
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
