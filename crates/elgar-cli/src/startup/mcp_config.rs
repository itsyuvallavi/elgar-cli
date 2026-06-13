//! Runtime MCP config loading for the CLI.
//!
//! This reads `elgar-mcp.json` or `ELGAR_MCP_CONFIG` and validates configured
//! servers without connecting to them.

use std::path::{Path, PathBuf};

use elgar_core::mcp::config::{
    load_runtime_mcp_config as load_core_runtime_mcp_config, McpConfig, McpConfigError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMcpConfig {
    pub config: McpConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMcpConfigError {
    InvalidEnvironment { name: &'static str },
    ReadFailed { path: PathBuf, message: String },
    ParseFailed { path: PathBuf, message: String },
}

impl std::fmt::Display for RuntimeMcpConfigError {
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
            Self::ParseFailed { path, message } => {
                write!(
                    formatter,
                    "MCP config failed: could not parse {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for RuntimeMcpConfigError {}

pub fn load_runtime_mcp_config(
    start: impl AsRef<Path>,
) -> Result<Option<RuntimeMcpConfig>, RuntimeMcpConfigError> {
    load_core_runtime_mcp_config(start)
        .map(|runtime| {
            runtime.map(|runtime| RuntimeMcpConfig {
                config: runtime.config,
                source_path: runtime.source_path,
            })
        })
        .map_err(RuntimeMcpConfigError::from)
}

impl From<McpConfigError> for RuntimeMcpConfigError {
    fn from(error: McpConfigError) -> Self {
        match error {
            McpConfigError::InvalidEnvironment { name } => Self::InvalidEnvironment { name },
            McpConfigError::ReadFailed { path, message } => Self::ReadFailed { path, message },
            error => Self::ParseFailed {
                path: PathBuf::from("elgar-mcp.json"),
                message: error.to_string(),
            },
        }
    }
}
