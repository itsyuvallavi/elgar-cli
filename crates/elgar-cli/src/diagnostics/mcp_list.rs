//! MCP discovery diagnostic command.
//!
//! This command connects to one configured HTTP MCP server and lists discovered
//! tools/resources. It does not call the model or expose MCP tools to the
//! harness.

use std::{collections::BTreeMap, path::Path};

use elgar_core::mcp::{
    client::{discover_http_server, McpHttpDiscovery},
    config::{McpHttpServerConfig, McpSecretSource, McpServerConfig},
    error::McpError,
    logging::{log_config_loaded, McpLogContext},
};
use elgar_core::session::runtime_session_id;

use crate::{load_runtime_mcp_config, RuntimeMcpConfigError};

pub const MCP_COMMAND: &str = "mcp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpListError {
    Config(RuntimeMcpConfigError),
    MissingSubcommand,
    UnsupportedSubcommand(String),
    MissingServer,
    ServerNotFound(String),
    StdioNotSupported(String),
    InvalidEnvironment { name: String },
    Mcp(McpError),
}

impl std::fmt::Display for McpListError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::MissingSubcommand => write!(formatter, "MCP command failed: expected `list`"),
            Self::UnsupportedSubcommand(subcommand) => {
                write!(formatter, "MCP command failed: unsupported subcommand `{subcommand}`")
            }
            Self::MissingServer => {
                write!(formatter, "MCP command failed: expected `--server <id>`")
            }
            Self::ServerNotFound(server_id) => {
                write!(formatter, "MCP command failed: server `{server_id}` was not found")
            }
            Self::StdioNotSupported(server_id) => write!(
                formatter,
                "MCP command failed: server `{server_id}` uses stdio, which is not connected in this slice"
            ),
            Self::InvalidEnvironment { name } => write!(
                formatter,
                "MCP command failed: environment variable {name} is not valid Unicode"
            ),
            Self::Mcp(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for McpListError {}

pub fn is_mcp_command(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == MCP_COMMAND)
}

pub fn render_mcp_from_args(args: &[String], project_root: &Path) -> Result<String, McpListError> {
    let Some(subcommand) = args.get(1) else {
        return Err(McpListError::MissingSubcommand);
    };
    if subcommand != "list" {
        return Err(McpListError::UnsupportedSubcommand(subcommand.clone()));
    }

    let server_id = parse_server_arg(args)?;
    render_mcp_list(project_root, server_id)
}

pub fn render_mcp_list(project_root: &Path, server_id: &str) -> Result<String, McpListError> {
    let session_id = runtime_session_id("mcp-diagnostic");
    let runtime = load_runtime_mcp_config(project_root).map_err(McpListError::Config)?;
    let Some(runtime) = runtime else {
        return Err(McpListError::Config(RuntimeMcpConfigError::ReadFailed {
            path: project_root.join("elgar-mcp.json"),
            message: "config file not found".to_string(),
        }));
    };

    let server = runtime
        .config
        .servers
        .get(server_id)
        .ok_or_else(|| McpListError::ServerNotFound(server_id.to_string()))?;

    let context = McpLogContext {
        project_root: project_root.to_path_buf(),
        session_id,
        turn_id: 0,
        server_id: server_id.to_string(),
        transport: server_transport(server).to_string(),
    };
    log_config_loaded(
        &context,
        &runtime.source_path.display().to_string(),
        runtime.config.servers.len(),
        server_id,
    );

    match server {
        McpServerConfig::Http(config) => render_http_mcp_list(server_id, config, context),
        McpServerConfig::Stdio(_) => Err(McpListError::StdioNotSupported(server_id.to_string())),
    }
}

fn render_http_mcp_list(
    server_id: &str,
    config: &McpHttpServerConfig,
    context: McpLogContext,
) -> Result<String, McpListError> {
    let headers = resolve_secret_sources(&config.headers)?;
    let discovery = discover_http_server(config, headers, env!("CARGO_PKG_VERSION"), Some(context))
        .map_err(McpListError::Mcp)?;
    Ok(render_discovery(server_id, &discovery))
}

fn parse_server_arg(args: &[String]) -> Result<&str, McpListError> {
    let Some(position) = args.iter().position(|arg| arg == "--server") else {
        return Err(McpListError::MissingServer);
    };
    args.get(position + 1)
        .map(String::as_str)
        .filter(|server_id| !server_id.trim().is_empty())
        .ok_or(McpListError::MissingServer)
}

fn resolve_secret_sources(
    values: &BTreeMap<String, McpSecretSource>,
) -> Result<BTreeMap<String, String>, McpListError> {
    let mut resolved = BTreeMap::new();
    for (name, source) in values {
        match std::env::var(&source.env) {
            Ok(value) => {
                resolved.insert(name.clone(), value);
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(McpListError::InvalidEnvironment {
                    name: source.env.clone(),
                });
            }
        }
    }
    Ok(resolved)
}

fn server_transport(server: &McpServerConfig) -> &'static str {
    match server {
        McpServerConfig::Http(_) => "http",
        McpServerConfig::Stdio(_) => "stdio",
    }
}

fn render_discovery(server_id: &str, discovery: &McpHttpDiscovery) -> String {
    let mut lines = vec![
        format!("MCP server: {server_id}"),
        format!(
            "server: {} {}",
            discovery.initialize.server_info.name, discovery.initialize.server_info.version
        ),
        format!("protocol: {}", discovery.initialize.protocol_version),
    ];

    if let Some(tools) = &discovery.tools {
        lines.push(format!("tools: {}", tools.tools.len()));
        for tool in &tools.tools {
            let description = compact_description(tool.description.as_deref());
            lines.push(format!("- {}: {}", tool.name, description));
        }
    } else {
        lines.push("tools: not declared".to_string());
    }

    if let Some(resources) = &discovery.resources {
        lines.push(format!("resources: {}", resources.resources.len()));
        for resource in &resources.resources {
            lines.push(format!("- {} ({})", resource.name, resource.uri));
        }
    } else {
        lines.push("resources: not declared".to_string());
    }

    lines.join("\n")
}

fn compact_description(description: Option<&str>) -> String {
    let first_line = description
        .and_then(|description| description.lines().find(|line| !line.trim().is_empty()))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("no description");

    const LIMIT: usize = 160;
    if first_line.chars().count() <= LIMIT {
        return first_line.to_string();
    }

    let mut clipped = first_line.chars().take(LIMIT).collect::<String>();
    clipped.push_str("...");
    clipped
}

#[cfg(test)]
mod tests {
    use super::{compact_description, is_mcp_command, parse_server_arg};

    #[test]
    fn detects_mcp_command() {
        assert!(is_mcp_command(&["mcp".to_string(), "list".to_string()]));
        assert!(!is_mcp_command(&["logs".to_string(), "latest".to_string()]));
    }

    #[test]
    fn parses_server_arg() {
        let args = vec![
            "mcp".to_string(),
            "list".to_string(),
            "--server".to_string(),
            "context7".to_string(),
        ];

        assert_eq!(parse_server_arg(&args).unwrap(), "context7");
    }

    #[test]
    fn compacts_tool_description_to_first_line() {
        let compact = compact_description(Some("First line.\nSecond line."));

        assert_eq!(compact, "First line.");
    }
}
