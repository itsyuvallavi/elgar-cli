//! Tests for loading `elgar-provider.json` into runtime provider config.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use elgar_core::{
    provider::ProviderBackendKind,
    runtime_home::{global_config_file, CONFIG_DIR, ELGAR_HOME_DIR, ELGAR_HOME_ENV},
};

use crate::{
    load_runtime_provider, render_cli_turn_from_runtime_config, RuntimeProviderConfigError,
    PROVIDER_CONFIG_ENV, PROVIDER_CONFIG_FILE,
};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-lib-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn restore_env(name: &str, previous: Option<std::ffi::OsString>) {
    match previous {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn with_provider_env<T>(home: &Path, run: impl FnOnce() -> T) -> T {
    let _guard = env_lock();
    let previous_home = std::env::var_os(ELGAR_HOME_ENV);
    let previous_provider = std::env::var_os(PROVIDER_CONFIG_ENV);

    std::env::set_var(ELGAR_HOME_ENV, home);
    std::env::remove_var(PROVIDER_CONFIG_ENV);

    let result = run();

    restore_env(ELGAR_HOME_ENV, previous_home);
    restore_env(PROVIDER_CONFIG_ENV, previous_provider);

    result
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
            "harness_tool_decision": {
              "backend": "openai_chat_completions"
            },
            "harness_synthesis": {
              "backend": "openai_chat_completions",
              "stats": true
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
    let synthesis = runtime.config.request_profile_for_mode("harness_synthesis");
    assert_eq!(
        synthesis.backend,
        ProviderBackendKind::OpenAiChatCompletions
    );
    assert_eq!(synthesis.stats, Some(true));
    let tool = runtime
        .config
        .request_profile_for_mode("harness_tool_decision");
    assert_eq!(tool.backend, ProviderBackendKind::OpenAiChatCompletions);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_provider_config_absent_keeps_stub_fallback() {
    let root = temp_root("runtime-provider-absent");
    let user_home = temp_root("runtime-provider-absent-home");
    let elgar_home = user_home.join(ELGAR_HOME_DIR);

    with_provider_env(&elgar_home, || {
        assert_eq!(load_runtime_provider(&root).unwrap(), None);

        let rendered =
            render_cli_turn_from_runtime_config("what does the harness do?", &root, &root).unwrap();
        assert!(rendered.contains("provider started: stub-provider"));
        assert!(!rendered.contains("lm-studio"));
    });

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(user_home);
}

#[test]
fn runtime_provider_config_loads_global_user_config() {
    let root = temp_root("runtime-provider-global-root");
    let user_home = temp_root("runtime-provider-global-home");
    let elgar_home = user_home.join(ELGAR_HOME_DIR);

    with_provider_env(&elgar_home, || {
        fs::create_dir_all(elgar_home.join(CONFIG_DIR)).unwrap();
        let path = global_config_file(PROVIDER_CONFIG_FILE);
        fs::write(
            &path,
            r#"{
              "provider": "lm-studio",
              "default_model": "qwen-global",
              "mode": "live",
              "context_window_tokens": 128000
            }"#,
        )
        .unwrap();

        let runtime = load_runtime_provider(&root).unwrap().unwrap();

        assert_eq!(runtime.source_path, path);
        assert_eq!(runtime.config.model.as_deref(), Some("qwen-global"));
        assert_eq!(runtime.config.context_window_tokens, Some(128_000));
    });

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(user_home);
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
