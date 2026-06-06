//! Real interactive terminal TUI entrypoint.
//!
//! This is the bridge from the CLI crate into `elgar-tui`. It resolves runtime
//! paths/provider config, then launches the full-screen terminal shell.

use std::io;

use crate::{load_runtime_provider, RuntimePaths};

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
    match load_runtime_provider(&paths.project_root) {
        Ok(Some(runtime)) => elgar_tui::run_terminal_shell_with_lm_studio_provider_at(
            runtime.config,
            &paths.project_root,
            &paths.cwd,
        ),
        Ok(None) => elgar_tui::run_terminal_shell_at(&paths.project_root, &paths.cwd),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    }
}
