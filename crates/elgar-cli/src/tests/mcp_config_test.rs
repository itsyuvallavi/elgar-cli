//! Tests for loading `elgar-mcp.json` into runtime MCP config.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use elgar_core::{
    mcp::config::McpServerConfig,
    runtime_home::{global_config_file, CONFIG_DIR, ELGAR_HOME_DIR, ELGAR_HOME_ENV},
};

use crate::{load_runtime_mcp_config, RuntimeMcpConfigError, MCP_CONFIG_ENV, MCP_CONFIG_FILE};

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

fn with_mcp_env<T>(home: &Path, run: impl FnOnce() -> T) -> T {
    let _guard = env_lock();
    let previous_home = std::env::var_os(ELGAR_HOME_ENV);
    let previous_mcp = std::env::var_os(MCP_CONFIG_ENV);

    std::env::set_var(ELGAR_HOME_ENV, home);
    std::env::remove_var(MCP_CONFIG_ENV);

    let result = run();

    restore_env(ELGAR_HOME_ENV, previous_home);
    restore_env(MCP_CONFIG_ENV, previous_mcp);

    result
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
    let user_home = temp_root("runtime-mcp-absent-home");
    let elgar_home = user_home.join(ELGAR_HOME_DIR);

    with_mcp_env(&elgar_home, || {
        assert_eq!(load_runtime_mcp_config(&root).unwrap(), None);
    });

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(user_home);
}

#[test]
fn runtime_mcp_config_loads_global_user_config() {
    let root = temp_root("runtime-mcp-global-root");
    let user_home = temp_root("runtime-mcp-global-home");
    let elgar_home = user_home.join(ELGAR_HOME_DIR);

    with_mcp_env(&elgar_home, || {
        fs::create_dir_all(elgar_home.join(CONFIG_DIR)).unwrap();
        let path = global_config_file(MCP_CONFIG_FILE);
        fs::write(
            &path,
            r#"{
              "servers": {
                "project-index": {
                  "transport": "internal",
                  "kind": "project_index"
                }
              }
            }"#,
        )
        .unwrap();

        let runtime = load_runtime_mcp_config(&root).unwrap().unwrap();

        assert_eq!(runtime.source_path, path);
        assert!(matches!(
            runtime.config.servers.get("project-index"),
            Some(McpServerConfig::Internal(_))
        ));
    });

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(user_home);
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
