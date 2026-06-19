//! Terminal entry points and shared terminal constants.
//!
//! This module connects the CLI startup path to the interactive inline prompt.

use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{startup::StartupMcpStatus, TuiShell};
use elgar_core::{
    harness::PendingApproval,
    provider::{ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStub},
    session::{runtime_session_id, Session},
};

mod commands;
mod display_context;
mod inline;
mod input;
mod turn;
pub(crate) mod ui;

pub use commands::{
    parse_terminal_command, render_terminal_help, render_unknown_command, TerminalCommand,
};
use display_context::terminal_context;
pub use display_context::TerminalShellContext;
use inline::print_inline_startup;
use input::read::{read_inline_prompt, InlinePromptSubmission};
use turn::submitted::handle_inline_submission;
pub use ui::render::{default_shell_text, render_default_terminal_shell, render_tui_shell};

const IDLE_RENDER_INTERVAL: Duration = Duration::from_millis(140);

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_CYAN: &str = "\x1b[38;2;143;207;198m";
const ANSI_MUTED: &str = "\x1b[38;2;118;126;126m";
const ANSI_TEXT: &str = "\x1b[38;2;214;219;224m";
const ANSI_EVENT: &str = "\x1b[38;2;102;220;150m";
const ANSI_USER_BLOCK: &str = "\x1b[1m\x1b[38;2;143;207;198m\x1b[48;2;8;32;32m";
const ANSI_CODE_BORDER: &str = "\x1b[38;2;83;94;108m\x1b[48;2;18;22;28m";
const ANSI_CODE_HEADER: &str = "\x1b[38;2;117;196;187m\x1b[48;2;18;22;28m";
const ANSI_CODE_BODY: &str = "\x1b[38;2;224;229;235m\x1b[48;2;18;22;28m";
const ANSI_CODE_HINT: &str = "\x1b[38;2;150;159;176m\x1b[48;2;18;22;28m";
const ANSI_CODE_KEY: &str = "\x1b[38;2;117;196;187m\x1b[48;2;18;22;28m";
const ANSI_CODE_STRING: &str = "\x1b[38;2;186;214;194m\x1b[48;2;18;22;28m";
const ANSI_CODE_NUMBER: &str = "\x1b[38;2;214;181;110m\x1b[48;2;18;22;28m";
const ANSI_CODE_LITERAL: &str = "\x1b[38;2;218;154;118m\x1b[48;2;18;22;28m";
const ANSI_CODE_COMMENT: &str = "\x1b[38;2;117;126;138m\x1b[48;2;18;22;28m";
const ANSI_RAW_DETAILS: &str = "\x1b[38;2;180;188;196m";
const ANSI_CURSOR_HIDE: &str = "\x1b[?25l";
const ANSI_CURSOR_SHOW: &str = "\x1b[?25h";

/// Render the current pending approval as plain text for non-interactive views.
pub fn render_pending_approval_text(approval: &PendingApproval) -> String {
    ui::approval::render_pending_approval_text(approval)
}

/// Start the terminal shell in the current working directory.
pub fn run_terminal_shell() -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_at(&cwd, &cwd)
}

/// Start the terminal shell with an LM Studio provider config.
pub fn run_terminal_shell_with_lm_studio_provider(config: ProviderConfig) -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_with_provider_and_context_window(
        &cwd,
        &cwd,
        LmStudioProvider::new(config.clone()),
        config.configured_context_window_tokens(),
        StartupMcpStatus::Inactive,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_at(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_with_lm_studio_provider_and_mcp_at(
        config,
        project_root,
        cwd,
        StartupMcpStatus::Inactive,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_and_mcp_at(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    mcp_status: StartupMcpStatus,
) -> io::Result<()> {
    run_terminal_shell_with_provider_and_context_window(
        project_root,
        cwd,
        LmStudioProvider::new(config.clone()),
        config.configured_context_window_tokens(),
        mcp_status,
    )
}

pub fn run_terminal_shell_at(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_with_provider(project_root, cwd, ProviderStub::default())
}

pub fn run_terminal_shell_at_with_mcp_status(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    mcp_status: StartupMcpStatus,
) -> io::Result<()> {
    run_terminal_shell_with_provider_and_context_window(
        project_root,
        cwd,
        ProviderStub::default(),
        None,
        mcp_status,
    )
}

/// Shared terminal-shell launcher for any provider implementation.
fn run_terminal_shell_with_provider<P>(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    provider: P,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    run_terminal_shell_with_provider_and_context_window(
        project_root,
        cwd,
        provider,
        None,
        StartupMcpStatus::Inactive,
    )
}

fn run_terminal_shell_with_provider_and_context_window<P>(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    provider: P,
    context_window_tokens: Option<u64>,
    mcp_status: StartupMcpStatus,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let session_id = runtime_session_id("terminal-tui");
    let mut session = Session::new(&session_id, project_root.as_ref(), cwd.as_ref());
    session.set_context_window_tokens(context_window_tokens);
    let mut shell = TuiShell::new();

    let mut context = terminal_context(&session, &provider).with_mcp_status(mcp_status);
    print_inline_startup(&context)?;

    let mut next_prompt_input = String::new();
    loop {
        context = terminal_context(&session, &provider);
        let Some(input) = read_inline_prompt(&context, &next_prompt_input)? else {
            break;
        };
        next_prompt_input.clear();

        let input = match input {
            InlinePromptSubmission::Text(input) => input,
            InlinePromptSubmission::Approval(action) => match action {
                ui::approval_action::ApprovalAction::Approve => "/approve".to_string(),
                ui::approval_action::ApprovalAction::Deny => "/deny".to_string(),
            },
        };

        let (exit, preserved_input) =
            handle_inline_submission(&input, &provider, &mut session, &mut shell)?;
        if exit {
            break;
        }
        next_prompt_input = preserved_input;
    }

    Ok(())
}
