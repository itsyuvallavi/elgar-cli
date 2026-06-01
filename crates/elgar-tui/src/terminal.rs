use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::TuiShell;
use elgar_core::{
    action_gate::ActionGate,
    agent_runtime::AgentRuntime,
    policy::PermissionPolicyMode,
    provider::{ControllerProvider, ProviderConfig},
    session::Session,
};

mod commands;
mod context;
mod footer;
mod inline;
mod keymap;
mod prompt;
mod provider_task;
mod render;
mod text;

#[cfg(test)]
use commands::{
    copy_conversation_to_terminal_clipboard, copy_conversation_with_clipboards,
    copy_text_with_command_and_args, encode_base64, osc52_clipboard_sequence,
    parse_terminal_command, render_terminal_help, TerminalCommand,
};
#[cfg(test)]
use context::{context_window_pressure, ContextWindowPressure};
#[cfg(test)]
use inline::{
    handle_active_provider_input_event, handle_active_provider_key, handle_terminal_input_event,
    live_render_due, ActiveProviderKeyAction, LIVE_RENDER_INTERVAL,
};
#[cfg(test)]
use keymap::{
    handle_scroll_key, handle_submitted_terminal_input_for_loop, handle_terminal_key,
    handle_terminal_key_with_copy_writer, should_exit,
};
#[cfg(test)]
use prompt::LiveProviderOutput;
#[cfg(test)]
use prompt::{
    active_working_frame_lines, active_working_frame_lines_with_cursor, inline_prompt_frame_lines,
    inline_prompt_frame_lines_with_cursor,
};
#[cfg(test)]
use provider_task::ProviderTurnUpdate;
#[cfg(test)]
use provider_task::{start_provider_turn, ProviderTurnTask};
use render::transcript_output_ansi;
#[cfg(test)]
use render::{status_style, style_terminal_conversation};
#[cfg(test)]
use text::{conversation_print_blocks, plain_block_lines};

use context::terminal_context;
pub use context::TerminalShellContext;
use inline::{handle_inline_submission, print_inline_startup, read_inline_prompt};
pub use render::{default_shell_text, render_default_terminal_shell, render_tui_shell};

const IDLE_RENDER_INTERVAL: Duration = Duration::from_millis(140);

const ANSI_RESET: &str = "[0m";
const ANSI_BOLD: &str = "[1m";
const ANSI_CYAN: &str = "[38;2;143;207;198m";
const ANSI_MUTED: &str = "[38;2;118;126;126m";
const ANSI_TEXT: &str = "[38;2;214;219;224m";
const ANSI_TOOL_BLOCK: &str = "[38;2;186;214;194m[48;2;29;45;34m";
const ANSI_USER_BLOCK: &str = "[1m[38;2;143;207;198m[48;2;8;32;32m";
const ANSI_CURSOR_HIDE: &str = "[?25l";
const ANSI_CURSOR_SHOW: &str = "[?25h";

pub fn run_terminal_shell() -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_at(&cwd, &cwd)
}

pub fn run_terminal_shell_with_lm_studio_provider(config: ProviderConfig) -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let context_window_tokens = config.configured_context_window_tokens();
    run_terminal_shell_with_runtime(
        &cwd,
        &cwd,
        AgentRuntime::with_lm_studio_provider(config),
        context_window_tokens,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_at(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_with_lm_studio_provider_at_with_policy(
        config,
        project_root,
        cwd,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_at_with_policy(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()> {
    let context_window_tokens = config.configured_context_window_tokens();
    run_terminal_shell_with_runtime(
        project_root,
        cwd,
        AgentRuntime::with_lm_studio_provider(config),
        context_window_tokens,
        policy_mode,
    )
}

pub fn run_terminal_shell_at(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_at_with_policy(
        project_root,
        cwd,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_at_with_policy(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()> {
    run_terminal_shell_with_runtime(
        project_root,
        cwd,
        AgentRuntime::default(),
        None,
        policy_mode,
    )
}

fn run_terminal_shell_with_runtime<P>(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    runtime: AgentRuntime<P>,
    context_window_tokens: Option<u64>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let mut session = Session::new("terminal-tui-session", project_root.as_ref(), cwd.as_ref());
    runtime.refresh_context_accounting(&mut session, context_window_tokens);
    let action_gate = ActionGate::new(runtime.provider.clone());
    let mut shell = TuiShell::with_policy_mode(policy_mode);

    let mut context = terminal_context(&session, &runtime, shell.policy_mode);
    print_inline_startup(&context)?;

    let mut next_prompt_input = String::new();
    loop {
        runtime.refresh_context_accounting(&mut session, context_window_tokens);
        context = terminal_context(&session, &runtime, shell.policy_mode);
        let Some(input) = read_inline_prompt(&context, &next_prompt_input)? else {
            break;
        };
        next_prompt_input.clear();

        let (exit, preserved_input) =
            handle_inline_submission(&input, &runtime, &action_gate, &mut session, &mut shell)?;
        if exit {
            break;
        }
        next_prompt_input = preserved_input;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
