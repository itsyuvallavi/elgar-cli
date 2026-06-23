//! Tests for direct LM Studio smoke-test config helpers.

use elgar_core::provider::LM_STUDIO_DEFAULT_BASE_URL;

use crate::{
    provider_smoke_config, provider_smoke_prompt, ProviderSmokeError, PROVIDER_SMOKE_DEFAULT_PROMPT,
};

#[test]
fn provider_smoke_prompt_defaults_when_no_prompt_is_passed() {
    assert_eq!(provider_smoke_prompt(&[]), PROVIDER_SMOKE_DEFAULT_PROMPT);
    assert_eq!(
        provider_smoke_prompt(&["   ".to_string()]),
        PROVIDER_SMOKE_DEFAULT_PROMPT
    );
}

#[test]
fn provider_smoke_prompt_joins_terminal_args() {
    assert_eq!(
        provider_smoke_prompt(&["Say".to_string(), "hello.".to_string()]),
        "Say hello."
    );
}

#[test]
fn provider_smoke_config_requires_model_env_value() {
    let error = provider_smoke_config(None, None, "hello").unwrap_err();

    assert_eq!(error, ProviderSmokeError::MissingModel);
    assert!(error.to_string().contains("ELGAR_LM_STUDIO_MODEL"));

    let blank = provider_smoke_config(Some("   ".to_string()), None, "hello").unwrap_err();
    assert_eq!(blank, ProviderSmokeError::MissingModel);
}

#[test]
fn provider_smoke_config_uses_default_base_url_and_prompt() {
    let config = provider_smoke_config(Some("local-model".to_string()), None, "  ").unwrap();

    assert_eq!(config.model, "local-model");
    assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
    assert_eq!(config.prompt, PROVIDER_SMOKE_DEFAULT_PROMPT);
}

#[test]
fn provider_smoke_config_accepts_custom_base_url() {
    let config = provider_smoke_config(
        Some("local-model".to_string()),
        Some(" http://localhost:4321/v1 ".to_string()),
        "hello",
    )
    .unwrap();

    assert_eq!(config.base_url, "http://localhost:4321/v1");
    assert_eq!(config.prompt, "hello");
}
