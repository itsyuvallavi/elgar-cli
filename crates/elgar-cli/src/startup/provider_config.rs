//! Runtime provider config loading for the CLI.
//!
//! This reads `elgar-provider.json` or `ELGAR_PROVIDER_CONFIG` and turns it into
//! the provider config used by single-turn CLI and TUI startup.

use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    provider::{ProviderCompatibility, ProviderConfig, ProviderRequestProfile},
    runtime_home::global_config_file,
};
use serde::Deserialize;

use super::paths::{find_provider_config_file, PROVIDER_CONFIG_ENV, PROVIDER_CONFIG_FILE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProvider {
    pub config: ProviderConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeProviderConfigError {
    InvalidEnvironment { name: &'static str },
    ReadFailed { path: PathBuf, message: String },
    ParseFailed { path: PathBuf, message: String },
    UnsupportedProvider { provider: String },
    MissingModel { path: PathBuf },
}

impl std::fmt::Display for RuntimeProviderConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEnvironment { name } => {
                write!(
                    formatter,
                    "provider config failed: environment variable {name} is not valid Unicode"
                )
            }
            Self::ReadFailed { path, message } => {
                write!(
                    formatter,
                    "provider config failed: could not read {}: {message}",
                    path.display()
                )
            }
            Self::ParseFailed { path, message } => {
                write!(
                    formatter,
                    "provider config failed: could not parse {}: {message}",
                    path.display()
                )
            }
            Self::UnsupportedProvider { provider } => {
                write!(
                    formatter,
                    "provider config failed: unsupported provider {provider}"
                )
            }
            Self::MissingModel { path } => {
                write!(
                    formatter,
                    "provider config failed: {} is live but has no default_model",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for RuntimeProviderConfigError {}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct RuntimeProviderConfigFile {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    timeout_millis: Option<u64>,
    #[serde(default)]
    connect_timeout_millis: Option<u64>,
    #[serde(default)]
    read_timeout_millis: Option<u64>,
    #[serde(default)]
    write_timeout_millis: Option<u64>,
    #[serde(default)]
    request_timeout_millis: Option<u64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    context_window_tokens: Option<u64>,
    #[serde(default)]
    compatibility: ProviderCompatibility,
    #[serde(default)]
    request_modes: std::collections::BTreeMap<String, ProviderRequestProfile>,
}

/// Loads live provider config from the nearest runtime config file.
///
/// Returns `Ok(None)` when config is missing or explicitly disabled.
pub fn load_runtime_provider(
    start: impl AsRef<Path>,
) -> Result<Option<RuntimeProvider>, RuntimeProviderConfigError> {
    let Some(path) = runtime_provider_config_path(start)? else {
        return Ok(None);
    };

    let contents =
        fs::read_to_string(&path).map_err(|error| RuntimeProviderConfigError::ReadFailed {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let file: RuntimeProviderConfigFile = serde_json::from_str(&contents).map_err(|error| {
        RuntimeProviderConfigError::ParseFailed {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;

    runtime_provider_from_file(path, file)
}

/// Chooses an explicit env config path or searches upward from `start`.
fn runtime_provider_config_path(
    start: impl AsRef<Path>,
) -> Result<Option<PathBuf>, RuntimeProviderConfigError> {
    match std::env::var(PROVIDER_CONFIG_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if matches!(trimmed, "" | "off" | "none" | "disabled") {
                return Ok(None);
            }
            return Ok(Some(PathBuf::from(trimmed)));
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(RuntimeProviderConfigError::InvalidEnvironment {
                name: PROVIDER_CONFIG_ENV,
            });
        }
    }

    if let Some(path) = find_provider_config_file(start) {
        return Ok(Some(path));
    }

    let global = global_config_file(PROVIDER_CONFIG_FILE);
    Ok(global.exists().then_some(global))
}

/// Converts the decoded JSON file into the active provider config.
fn runtime_provider_from_file(
    path: PathBuf,
    file: RuntimeProviderConfigFile,
) -> Result<Option<RuntimeProvider>, RuntimeProviderConfigError> {
    let mode = file.mode.trim();
    if !mode.eq_ignore_ascii_case("live") {
        return Ok(None);
    }

    let provider = if file.provider.trim().is_empty() {
        "lm-studio"
    } else {
        file.provider.trim()
    };
    if provider != "lm-studio" {
        return Err(RuntimeProviderConfigError::UnsupportedProvider {
            provider: provider.to_string(),
        });
    }

    let model = file
        .default_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| RuntimeProviderConfigError::MissingModel { path: path.clone() })?;

    let mut config = ProviderConfig::lm_studio(model);
    if let Some(base_url) = file
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        config.base_url = base_url.to_string();
    }
    if let Some(timeout_millis) = file.timeout_millis {
        config.timeout_millis = timeout_millis;
    }
    config.connect_timeout_millis = file.connect_timeout_millis;
    config.read_timeout_millis = file.read_timeout_millis;
    config.write_timeout_millis = file.write_timeout_millis;
    config.request_timeout_millis = file.request_timeout_millis;
    config.stream = file.stream;
    config.context_window_tokens = file.context_window_tokens;
    config.compatibility = file.compatibility;
    config.request_modes = file.request_modes;

    Ok(Some(RuntimeProvider {
        config,
        source_path: path,
    }))
}
