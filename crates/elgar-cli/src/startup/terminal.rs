//! Real interactive terminal TUI entrypoint.
//!
//! This is the bridge from the CLI crate into `elgar-tui`. It resolves runtime
//! paths/provider config, then launches the full-screen terminal shell.

use std::{io, path::Path};

use crate::{load_runtime_mcp_config, load_runtime_provider, RuntimePaths};

pub const TUI_COMMAND: &str = "tui";
pub const TUI_TERMINAL_COMMAND: &str = "tui-terminal";

pub fn should_launch_terminal_tui_by_default(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

/// Launches the interactive terminal shell with live provider config if present.
pub fn run_tui_terminal() -> io::Result<()> {
    let paths = RuntimePaths::from_current_dir();
    let mcp_status = startup_mcp_status(&paths.project_root);
    match load_runtime_provider(&paths.project_root) {
        Ok(Some(runtime)) => elgar_tui::run_terminal_shell_with_lm_studio_provider_and_mcp_at(
            runtime.config,
            &paths.project_root,
            &paths.cwd,
            mcp_status,
        ),
        Ok(None) => elgar_tui::run_terminal_shell_at_with_mcp_status(
            &paths.project_root,
            &paths.cwd,
            mcp_status,
        ),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    }
}

fn startup_mcp_status(project_root: &Path) -> elgar_tui::StartupMcpStatus {
    match load_runtime_mcp_config(project_root) {
        Ok(Some(runtime)) => {
            let server_ids = runtime.config.servers.keys().cloned().collect::<Vec<_>>();
            elgar_tui::StartupMcpStatus::active(server_ids, compact_home_path(&runtime.source_path))
        }
        Ok(None) => elgar_tui::StartupMcpStatus::Inactive,
        Err(error) => elgar_tui::StartupMcpStatus::error(error.to_string()),
    }
}

fn compact_home_path(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    let home = Path::new(&home);
    match path.strip_prefix(home) {
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}
