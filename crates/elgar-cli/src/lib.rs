//! Public CLI library surface used by the `elgar` binary and CLI tests.
//!
//! This module re-exports startup and diagnostic helpers and owns the simple
//! single-turn CLI rendering path.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use elgar_core::{
    harness::run_harness_turn,
    provider::{LmStudioProvider, ProviderStub},
    renderer::render_session,
    session::Session,
};
use elgar_tui::terminal::{
    parse_terminal_command, render_terminal_help, render_unknown_command, TerminalCommand,
};

mod diagnostics;
mod startup;

static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub use diagnostics::*;
pub use startup::*;

pub fn init_terminal_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "off"))
        .format_timestamp_millis()
        .try_init();
}

/// Runs one CLI prompt against the no-network stub provider.
///
/// This is used by smoke tests and by fallback paths when no live provider
/// config is available.
pub fn render_cli_turn(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    if let Some(local_output) = render_cli_local_command(input) {
        return local_output;
    }
    if is_direct_logs_latest_prompt(input) {
        return render_latest_turn_summary(project_root.as_ref())
            .unwrap_or_else(|error| error.to_string());
    }

    log::debug!(
        "render_cli_turn stub_provider input_chars={} project_root={} cwd={}",
        input.chars().count(),
        project_root.as_ref().display(),
        cwd.as_ref().display()
    );
    let provider = ProviderStub::default();
    let session_id = runtime_session_id("cli-smoke");
    let mut session = Session::new(&session_id, project_root.as_ref(), cwd.as_ref());

    run_harness_turn(&provider, &mut session, input);
    render_session(&session)
}

/// Render a direct CLI slash command locally instead of sending it to a model.
pub fn render_cli_local_command(input: &str) -> Option<String> {
    match parse_terminal_command(input) {
        TerminalCommand::Empty | TerminalCommand::Text(_) => None,
        TerminalCommand::Help => Some(render_terminal_help().to_string()),
        TerminalCommand::Approve | TerminalCommand::Deny => {
            Some("No pending approval.".to_string())
        }
        TerminalCommand::Cancel => Some("No active provider turn to cancel.".to_string()),
        TerminalCommand::DetailsLast | TerminalCommand::CopyRaw => {
            Some("No raw details are available.".to_string())
        }
        TerminalCommand::Copy => Some("No conversation is available to copy.".to_string()),
        TerminalCommand::Clear => Some("(empty conversation)".to_string()),
        TerminalCommand::Exit => Some(
            "Nothing to exit. Run `elgar` with no arguments for the interactive TUI.".to_string(),
        ),
        TerminalCommand::Unknown(command) => Some(render_unknown_command(command)),
    }
}

/// Runs one CLI prompt using `elgar-provider.json` when live config exists.
///
/// If runtime config is missing or disabled, this falls back to
/// `render_cli_turn` and uses the stub provider.
pub fn render_cli_turn_from_runtime_config(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, RuntimeProviderConfigError> {
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();

    if let Some(local_output) = render_cli_local_command(input) {
        return Ok(local_output);
    }
    if is_direct_logs_latest_prompt(input) {
        return Ok(
            render_latest_turn_summary(project_root_ref).unwrap_or_else(|error| error.to_string())
        );
    }

    let Some(runtime_provider) = load_runtime_provider(project_root_ref)? else {
        log::debug!(
            "runtime provider config missing; using stub provider project_root={}",
            project_root_ref.display()
        );
        return Ok(render_cli_turn(input, project_root_ref, cwd_ref));
    };

    log::info!(
        "render_cli_turn raw_provider provider={} model={:?} input_chars={} cwd={}",
        runtime_provider.config.provider,
        runtime_provider.config.model,
        input.chars().count(),
        cwd_ref.display()
    );
    let provider = LmStudioProvider::new(runtime_provider.config);
    let session_id = runtime_session_id("cli-runtime");
    let mut session = Session::new(&session_id, project_root_ref, cwd_ref);

    run_harness_turn(&provider, &mut session, input);
    Ok(render_session(&session))
}

fn is_direct_logs_latest_prompt(input: &str) -> bool {
    input.split_whitespace().eq(["logs", "latest"])
}

/// Builds a unique session id for CLI-created sessions.
pub(crate) fn runtime_session_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let counter = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{millis}-{counter}", std::process::id())
}

#[cfg(test)]
mod tests;
