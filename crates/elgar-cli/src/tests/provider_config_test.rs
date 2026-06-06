//! Tests for loading `elgar-provider.json` into runtime provider config.

use std::{fs, path::PathBuf};

use elgar_core::provider::{ProviderBackendKind, ProviderReasoningLevel};

use crate::{
    load_runtime_provider, render_cli_turn_from_runtime_config, RuntimeProviderConfigError,
    PROVIDER_CONFIG_FILE,
};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-lib-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn runtime_provider_config_loads_live_lm_studio_file() {
    let root = temp_root("runtime-provider-live");
    fs::write(
        root.join(PROVIDER_CONFIG_FILE),
        r#"{
          "provider": "lm-studio",
          "base_url": "http://127.0.0.1:1234/v1",
          "default_model": "openai/gpt-oss-20b",
          "mode": "live",
          "connect_timeout_millis": 1000,
          "read_timeout_millis": 120000,
          "write_timeout_millis": 2000,
          "request_timeout_millis": 180000,
          "context_window_tokens": 128000,
          "stream": true
        }"#,
    )
    .unwrap();

    let runtime = load_runtime_provider(&root).unwrap().unwrap();

    assert_eq!(runtime.config.provider, "lm-studio");
    assert_eq!(runtime.config.base_url, "http://127.0.0.1:1234/v1");
    assert_eq!(runtime.config.model.as_deref(), Some("openai/gpt-oss-20b"));
    assert_eq!(runtime.config.connect_timeout_millis(), 1000);
    assert_eq!(runtime.config.read_timeout_millis(), 120000);
    assert_eq!(runtime.config.write_timeout_millis(), 2000);
    assert_eq!(runtime.config.request_timeout_millis(), 180000);
    assert_eq!(runtime.config.context_window_tokens, Some(128_000));
    assert_eq!(
        runtime.config.configured_context_window_tokens(),
        Some(128_000)
    );
    assert!(runtime.config.stream);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_provider_config_loads_compatibility_metadata() {
    let root = temp_root("runtime-provider-compatibility");
    fs::write(
        root.join(PROVIDER_CONFIG_FILE),
        r#"{
          "provider": "lm-studio",
          "base_url": "http://127.0.0.1:1234/v1",
          "default_model": "openai/gpt-oss-20b",
          "mode": "live",
          "context_window_tokens": 32000,
          "compatibility": {
            "context_window_tokens": 128000,
            "reasoning": {
              "response_fields": ["reasoning_content"],
              "stream_fields": ["reasoning_content", "thinking"]
            },
            "supports_streaming_usage": false,
            "supports_developer_role": true
          },
          "request_modes": {
            "plain_chat": {
              "backend": "lm_studio_native_chat",
              "reasoning": "off",
              "context_length": 8000,
              "stats": true
            },
            "chat_response": {
              "backend": "lm_studio_native_chat",
              "stats": true
            },
            "tool_enabled": {
              "backend": "openai_chat_completions"
            }
          }
        }"#,
    )
    .unwrap();

    let runtime = load_runtime_provider(&root).unwrap().unwrap();

    assert_eq!(runtime.config.context_window_tokens, Some(32_000));
    assert_eq!(
        runtime.config.configured_context_window_tokens(),
        Some(128_000)
    );
    assert!(runtime.config.supports_developer_role());
    assert_eq!(
        runtime.config.compatibility.supports_streaming_usage,
        Some(false)
    );
    assert_eq!(
        runtime.config.compatibility.reasoning.stream_fields,
        vec!["reasoning_content", "thinking"]
    );
    let plain = runtime.config.request_profile_for_mode("plain_chat");
    assert_eq!(plain.backend, ProviderBackendKind::LmStudioNativeChat);
    assert_eq!(plain.reasoning, Some(ProviderReasoningLevel::Off));
    assert_eq!(plain.context_length, Some(8000));
    assert_eq!(plain.stats, Some(true));
    let chat_response = runtime.config.request_profile_for_mode("chat_response");
    assert_eq!(
        chat_response.backend,
        ProviderBackendKind::LmStudioNativeChat
    );
    assert_eq!(chat_response.stats, Some(true));
    let tool = runtime.config.request_profile_for_mode("tool_enabled");
    assert_eq!(tool.backend, ProviderBackendKind::OpenAiChatCompletions);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_provider_config_absent_keeps_stub_fallback() {
    let root = temp_root("runtime-provider-absent");

    assert_eq!(load_runtime_provider(&root).unwrap(), None);

    let rendered =
        render_cli_turn_from_runtime_config("what does the harness do?", &root, &root).unwrap();
    assert!(rendered.contains("provider started: stub-provider"));
    assert!(!rendered.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_provider_config_live_requires_model() {
    let root = temp_root("runtime-provider-missing-model");
    let path = root.join(PROVIDER_CONFIG_FILE);
    fs::write(
        &path,
        r#"{
          "provider": "lm-studio",
          "mode": "live"
        }"#,
    )
    .unwrap();

    let error = load_runtime_provider(&root).unwrap_err();

    assert_eq!(error, RuntimeProviderConfigError::MissingModel { path });

    let _ = fs::remove_dir_all(root);
}
