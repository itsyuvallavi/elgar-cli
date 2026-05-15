use std::path::Path;

use elgar_core::{
    controller::Controller,
    provider::{
        chat_lm_studio, ChatMessage, ProviderConfig, ProviderError, LM_STUDIO_DEFAULT_BASE_URL,
    },
    renderer::render_session,
    session::Session,
};

pub const PROVIDER_SMOKE_COMMAND: &str = "provider-smoke";
pub const CONTROLLER_SMOKE_COMMAND: &str = "controller-smoke";
pub const PROVIDER_SMOKE_DEFAULT_PROMPT: &str = "Say hello in one sentence.";
pub const LM_STUDIO_MODEL_ENV: &str = "ELGAR_LM_STUDIO_MODEL";
pub const LM_STUDIO_BASE_URL_ENV: &str = "ELGAR_LM_STUDIO_BASE_URL";

pub fn render_cli_turn(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let controller = Controller::default();
    let mut session = Session::new("cli-smoke-session", project_root.as_ref(), cwd.as_ref());

    controller.turn(&mut session, input);
    render_session(&session)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSmokeConfig {
    pub model: String,
    pub base_url: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSmokeError {
    MissingModel,
    InvalidEnvironment { name: &'static str },
    Provider(ProviderError),
}

impl std::fmt::Display for ProviderSmokeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingModel => write!(
                formatter,
                "LM Studio smoke failed: missing required environment variable {LM_STUDIO_MODEL_ENV}; set it to the loaded LM Studio model name"
            ),
            Self::InvalidEnvironment { name } => write!(
                formatter,
                "LM Studio smoke failed: environment variable {name} is not valid Unicode"
            ),
            Self::Provider(error) => write!(formatter, "LM Studio smoke failed: {error}"),
        }
    }
}

impl std::error::Error for ProviderSmokeError {}

pub fn provider_smoke_prompt(args: &[String]) -> String {
    normalize_prompt(args.join(" "))
}

pub fn provider_smoke_config_from_env(
    prompt: impl Into<String>,
) -> Result<ProviderSmokeConfig, ProviderSmokeError> {
    let model = read_env(LM_STUDIO_MODEL_ENV)?;
    let base_url = read_env(LM_STUDIO_BASE_URL_ENV)?;

    provider_smoke_config(model, base_url, prompt)
}

pub fn provider_smoke_config(
    model: Option<String>,
    base_url: Option<String>,
    prompt: impl Into<String>,
) -> Result<ProviderSmokeConfig, ProviderSmokeError> {
    let model = model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .ok_or(ProviderSmokeError::MissingModel)?;
    let base_url = base_url
        .map(|base_url| base_url.trim().to_string())
        .filter(|base_url| !base_url.is_empty())
        .unwrap_or_else(|| LM_STUDIO_DEFAULT_BASE_URL.to_string());

    Ok(ProviderSmokeConfig {
        model,
        base_url,
        prompt: normalize_prompt(prompt.into()),
    })
}

pub fn run_provider_smoke_from_env(prompt: &str) -> Result<String, ProviderSmokeError> {
    let config = provider_smoke_config_from_env(prompt)?;
    run_provider_smoke(config)
}

pub fn run_provider_smoke(config: ProviderSmokeConfig) -> Result<String, ProviderSmokeError> {
    let provider_config = ProviderConfig {
        base_url: config.base_url,
        model: Some(config.model),
        ..ProviderConfig::default()
    };

    chat_lm_studio(&provider_config, vec![ChatMessage::user(config.prompt)])
        .map(|output| output.text)
        .map_err(ProviderSmokeError::Provider)
}

pub fn render_controller_smoke_from_env(
    prompt: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, ProviderSmokeError> {
    let config = provider_smoke_config_from_env(prompt)?;
    Ok(render_controller_smoke(config, project_root, cwd))
}

pub fn render_controller_smoke(
    config: ProviderSmokeConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let provider_config = ProviderConfig {
        base_url: config.base_url,
        model: Some(config.model),
        ..ProviderConfig::default()
    };
    let controller = Controller::with_lm_studio_provider(provider_config);
    let mut session = Session::new(
        "cli-controller-smoke-session",
        project_root.as_ref(),
        cwd.as_ref(),
    );

    controller.turn(&mut session, &config.prompt);
    render_session(&session)
}

fn normalize_prompt(prompt: impl Into<String>) -> String {
    let prompt = prompt.into();
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        PROVIDER_SMOKE_DEFAULT_PROMPT.to_string()
    } else {
        trimmed.to_string()
    }
}

fn read_env(name: &'static str) -> Result<Option<String>, ProviderSmokeError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(ProviderSmokeError::InvalidEnvironment { name })
        }
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::provider::LM_STUDIO_DEFAULT_BASE_URL;

    use super::{
        provider_smoke_config, provider_smoke_prompt, render_controller_smoke, ProviderSmokeConfig,
        ProviderSmokeError, PROVIDER_SMOKE_DEFAULT_PROMPT,
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

    #[test]
    fn controller_smoke_renders_live_provider_error_event_without_network() {
        let rendered = render_controller_smoke(
            ProviderSmokeConfig {
                model: "local-model".to_string(),
                base_url: "https://127.0.0.1:1234/v1".to_string(),
                prompt: "Say hello in one sentence.".to_string(),
            },
            ".",
            ".",
        );

        assert!(rendered.contains("user: Say hello in one sentence."));
        assert!(rendered.contains("provider started: lm-studio request lm-studio-request-1"));
        assert!(rendered.contains("error: lm-studio provider request lm-studio-request-1 failed"));
        assert!(rendered.contains("only http:// provider URLs are supported"));
        assert!(!rendered.contains("action proposed"));
        assert!(!rendered.contains("action applied"));
    }
}
