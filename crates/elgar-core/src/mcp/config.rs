//! MCP server config types.
//!
//! These types describe configured MCP servers without opening network
//! connections or launching subprocesses.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const MCP_CONFIG_ENV: &str = "ELGAR_MCP_CONFIG";
pub const MCP_CONFIG_FILE: &str = "elgar-mcp.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMcpConfig {
    pub config: McpConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpServerConfig {
    Http(McpHttpServerConfig),
    Stdio(McpStdioServerConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpHttpServerConfig {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, McpSecretSource>,
    #[serde(default)]
    pub timeout_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStdioServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, McpSecretSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSecretSource {
    pub env: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpConfigError {
    InvalidEnvironment { name: &'static str },
    ReadFailed { path: PathBuf, message: String },
    ParseFailed(String),
    EmptyServerId,
    EmptyHttpUrl { server_id: String },
    UnsupportedHttpUrl { server_id: String, url: String },
    EmptyHeaderName { server_id: String },
    EmptySecretEnv { server_id: String, name: String },
    EmptyStdioCommand { server_id: String },
    EmptyStdioEnvName { server_id: String },
}

impl std::fmt::Display for McpConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEnvironment { name } => {
                write!(
                    formatter,
                    "MCP config failed: environment variable {name} is not valid Unicode"
                )
            }
            Self::ReadFailed { path, message } => {
                write!(
                    formatter,
                    "MCP config failed: could not read {}: {message}",
                    path.display()
                )
            }
            Self::ParseFailed(message) => write!(formatter, "MCP config parse failed: {message}"),
            Self::EmptyServerId => write!(formatter, "MCP config has an empty server id"),
            Self::EmptyHttpUrl { server_id } => {
                write!(formatter, "MCP server {server_id} has an empty HTTP URL")
            }
            Self::UnsupportedHttpUrl { server_id, url } => write!(
                formatter,
                "MCP server {server_id} URL must start with http:// or https://: {url}"
            ),
            Self::EmptyHeaderName { server_id } => {
                write!(formatter, "MCP server {server_id} has an empty header name")
            }
            Self::EmptySecretEnv { server_id, name } => write!(
                formatter,
                "MCP server {server_id} secret source {name} has an empty env name"
            ),
            Self::EmptyStdioCommand { server_id } => {
                write!(
                    formatter,
                    "MCP stdio server {server_id} has an empty command"
                )
            }
            Self::EmptyStdioEnvName { server_id } => {
                write!(
                    formatter,
                    "MCP stdio server {server_id} has an empty env key"
                )
            }
        }
    }
}

impl std::error::Error for McpConfigError {}

pub fn load_runtime_mcp_config(
    project_root: impl AsRef<Path>,
) -> Result<Option<RuntimeMcpConfig>, McpConfigError> {
    let Some(path) = runtime_mcp_config_path(project_root.as_ref())? else {
        return Ok(None);
    };
    let contents = fs::read_to_string(&path).map_err(|error| McpConfigError::ReadFailed {
        path: path.clone(),
        message: error.to_string(),
    })?;
    let config = parse_mcp_config_json(&contents)?;
    Ok(Some(RuntimeMcpConfig {
        config,
        source_path: path,
    }))
}

fn runtime_mcp_config_path(project_root: &Path) -> Result<Option<PathBuf>, McpConfigError> {
    match std::env::var(MCP_CONFIG_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if matches!(trimmed, "" | "off" | "none" | "disabled") {
                return Ok(None);
            }
            return Ok(Some(PathBuf::from(trimmed)));
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(McpConfigError::InvalidEnvironment {
                name: MCP_CONFIG_ENV,
            });
        }
    }

    let candidate = project_root.join(MCP_CONFIG_FILE);
    Ok(candidate.exists().then_some(candidate))
}

pub fn parse_mcp_config_json(contents: &str) -> Result<McpConfig, McpConfigError> {
    let config: McpConfig = serde_json::from_str(contents)
        .map_err(|error| McpConfigError::ParseFailed(error.to_string()))?;
    validate_mcp_config(&config)?;
    Ok(config)
}

pub fn validate_mcp_config(config: &McpConfig) -> Result<(), McpConfigError> {
    for (server_id, server) in &config.servers {
        let trimmed_id = server_id.trim();
        if trimmed_id.is_empty() {
            return Err(McpConfigError::EmptyServerId);
        }

        match server {
            McpServerConfig::Http(http) => validate_http_server(trimmed_id, http)?,
            McpServerConfig::Stdio(stdio) => validate_stdio_server(trimmed_id, stdio)?,
        }
    }

    Ok(())
}

pub fn resolve_secret_sources(
    values: &BTreeMap<String, McpSecretSource>,
) -> Result<BTreeMap<String, String>, McpConfigError> {
    let mut resolved = BTreeMap::new();
    for (name, source) in values {
        match std::env::var(&source.env) {
            Ok(value) => {
                resolved.insert(name.clone(), value);
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(McpConfigError::InvalidEnvironment {
                    name: MCP_CONFIG_ENV,
                });
            }
        }
    }
    Ok(resolved)
}

fn validate_http_server(
    server_id: &str,
    config: &McpHttpServerConfig,
) -> Result<(), McpConfigError> {
    let url = config.url.trim();
    if url.is_empty() {
        return Err(McpConfigError::EmptyHttpUrl {
            server_id: server_id.to_string(),
        });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(McpConfigError::UnsupportedHttpUrl {
            server_id: server_id.to_string(),
            url: config.url.clone(),
        });
    }
    validate_secret_map(server_id, &config.headers, true)
}

fn validate_stdio_server(
    server_id: &str,
    config: &McpStdioServerConfig,
) -> Result<(), McpConfigError> {
    if config.command.trim().is_empty() {
        return Err(McpConfigError::EmptyStdioCommand {
            server_id: server_id.to_string(),
        });
    }
    validate_secret_map(server_id, &config.env, false)
}

fn validate_secret_map(
    server_id: &str,
    values: &BTreeMap<String, McpSecretSource>,
    is_header_map: bool,
) -> Result<(), McpConfigError> {
    for (name, source) in values {
        if name.trim().is_empty() {
            if is_header_map {
                return Err(McpConfigError::EmptyHeaderName {
                    server_id: server_id.to_string(),
                });
            }
            return Err(McpConfigError::EmptyStdioEnvName {
                server_id: server_id.to_string(),
            });
        }
        if source.env.trim().is_empty() {
            return Err(McpConfigError::EmptySecretEnv {
                server_id: server_id.to_string(),
                name: name.clone(),
            });
        }
    }
    Ok(())
}
