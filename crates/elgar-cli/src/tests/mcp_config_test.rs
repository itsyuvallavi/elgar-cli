//! Tests for loading `elgar-mcp.json` into runtime MCP config.

use std::{fs, path::PathBuf};

use elgar_core::mcp::config::McpServerConfig;

use crate::{load_runtime_mcp_config, RuntimeMcpConfigError, MCP_CONFIG_FILE};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-lib-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn runtime_mcp_config_loads_http_stdio_and_internal_servers() {
    let root = temp_root("runtime-mcp-config");
    fs::write(
        root.join(MCP_CONFIG_FILE),
        r#"{
          "servers": {
            "context7": {
              "transport": "http",
              "url": "https://mcp.context7.com/mcp",
              "headers": {
                "CONTEXT7_API_KEY": { "env": "CONTEXT7_API_KEY" }
              }
            },
            "obsidian": {
              "transport": "stdio",
              "command": "obsidian-mcp-server",
              "args": []
            },
            "project-index": {
              "transport": "internal",
              "kind": "project_index"
            }
          }
        }"#,
    )
    .unwrap();

    let runtime = load_runtime_mcp_config(&root).unwrap().unwrap();

    assert_eq!(runtime.source_path, root.join(MCP_CONFIG_FILE));
    assert!(matches!(
        runtime.config.servers.get("context7"),
        Some(McpServerConfig::Http(_))
    ));
    assert!(matches!(
        runtime.config.servers.get("obsidian"),
        Some(McpServerConfig::Stdio(_))
    ));
    assert!(matches!(
        runtime.config.servers.get("project-index"),
        Some(McpServerConfig::Internal(_))
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_mcp_config_absent_is_optional() {
    let root = temp_root("runtime-mcp-absent");

    assert_eq!(load_runtime_mcp_config(&root).unwrap(), None);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_mcp_config_reports_validation_errors() {
    let root = temp_root("runtime-mcp-invalid");
    fs::write(
        root.join(MCP_CONFIG_FILE),
        r#"{
          "servers": {
            "context7": {
              "transport": "http",
              "url": "ftp://mcp.context7.com/mcp"
            }
          }
        }"#,
    )
    .unwrap();

    let error = load_runtime_mcp_config(&root).unwrap_err();

    assert!(matches!(error, RuntimeMcpConfigError::ParseFailed { .. }));

    let _ = fs::remove_dir_all(root);
}
