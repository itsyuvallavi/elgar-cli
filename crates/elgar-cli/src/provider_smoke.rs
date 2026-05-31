use elgar_core::provider::{
    chat_lm_studio, ChatMessage, ProviderConfig, ProviderError, LM_STUDIO_DEFAULT_BASE_URL,
};

pub const PROVIDER_SMOKE_COMMAND: &str = "provider-smoke";
pub const PROVIDER_SMOKE_DEFAULT_PROMPT: &str = "Say hello in one sentence.";
pub const LM_STUDIO_MODEL_ENV: &str = "ELGAR_LM_STUDIO_MODEL";
pub const LM_STUDIO_BASE_URL_ENV: &str = "ELGAR_LM_STUDIO_BASE_URL";

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
    let provider_config = provider_config_from_smoke_config(&config);

    chat_lm_studio(&provider_config, vec![ChatMessage::user(config.prompt)])
        .map(|output| output.text)
        .map_err(ProviderSmokeError::Provider)
}

fn provider_config_from_smoke_config(config: &ProviderSmokeConfig) -> ProviderConfig {
    ProviderConfig {
        base_url: config.base_url.clone(),
        model: Some(config.model.clone()),
        ..ProviderConfig::default()
    }
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
