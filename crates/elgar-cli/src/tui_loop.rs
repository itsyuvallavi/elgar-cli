use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use elgar_core::{
    action_gate::ActionGate, agent_runtime::AgentRuntime, policy::PermissionPolicyMode,
    provider::ControllerProvider, session::Session,
};

use crate::{
    default_permission_policy_mode, load_runtime_provider, runtime_permission_policy_mode,
    RuntimePaths, RuntimeProviderConfigError,
};

pub const TUI_COMMAND: &str = "tui";
pub const TUI_TERMINAL_COMMAND: &str = "tui-terminal";

pub fn should_launch_terminal_tui_by_default(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

pub fn is_tui_exit_command(input: &str) -> bool {
    matches!(input.trim(), "/exit" | "/quit" | "/q")
}

pub fn is_tui_help_command(input: &str) -> bool {
    matches!(input.trim(), "/help" | "/commands")
}

pub fn is_tui_approval_command(input: &str) -> bool {
    input.trim() == "/approve"
}

pub fn is_tui_rejection_command(input: &str) -> bool {
    input.trim() == "/reject"
}

pub fn is_tui_copy_command(input: &str) -> bool {
    input.trim() == "/copy"
}

pub fn is_tui_memory_command(input: &str) -> bool {
    input.trim() == "/memory"
}

pub fn is_tui_state_snapshot_command(input: &str) -> bool {
    input.trim() == "/state"
}

pub fn is_tui_status_command(input: &str) -> bool {
    input.trim() == "/status"
}

pub fn is_tui_pending_command(input: &str) -> bool {
    input.trim() == "/pending"
}

pub fn is_tui_created_command(input: &str) -> bool {
    input.trim() == "/created"
}

pub fn is_tui_plan_preview_command(input: &str) -> bool {
    matches!(input.trim(), "/plan" | "/plan preview")
}

pub fn is_tui_reasoning_command(input: &str) -> bool {
    matches!(input.trim(), "/reasoning" | "/trace")
}

pub fn is_tui_tokens_command(input: &str) -> bool {
    input.trim() == "/tokens"
}

pub fn tui_tool_command_argument(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if trimmed == "/tool" {
        return Some("");
    }
    trimmed.strip_prefix("/tool ").map(str::trim)
}

pub fn tui_permission_command_argument(input: &str) -> Option<Option<&str>> {
    let trimmed = input.trim();
    if matches!(trimmed, "/permissions" | "/policy") {
        return Some(None);
    }
    trimmed
        .strip_prefix("/permissions ")
        .or_else(|| trimmed.strip_prefix("/policy "))
        .map(str::trim)
        .map(|argument| {
            if argument.is_empty() {
                None
            } else {
                Some(argument)
            }
        })
}

pub fn is_tui_clear_command(input: &str) -> bool {
    matches!(input.trim(), "/clear" | "/new")
}

pub fn is_tui_cancel_command(input: &str) -> bool {
    input.trim() == "/cancel"
}

pub fn tui_unknown_command(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    if is_tui_exit_command(trimmed)
        || is_tui_help_command(trimmed)
        || is_tui_approval_command(trimmed)
        || is_tui_rejection_command(trimmed)
        || is_tui_copy_command(trimmed)
        || is_tui_memory_command(trimmed)
        || is_tui_state_snapshot_command(trimmed)
        || is_tui_status_command(trimmed)
        || is_tui_pending_command(trimmed)
        || is_tui_created_command(trimmed)
        || is_tui_plan_preview_command(trimmed)
        || is_tui_reasoning_command(trimmed)
        || is_tui_tokens_command(trimmed)
        || is_tui_clear_command(trimmed)
        || is_tui_cancel_command(trimmed)
        || tui_tool_command_argument(trimmed).is_some()
        || tui_permission_command_argument(trimmed).is_some()
    {
        None
    } else {
        Some(trimmed)
    }
}

pub fn render_tui_unknown_command(command: &str) -> String {
    format!(
        "Unknown command: {command}\nUse /commands to see local commands. Plain text without / is sent to the model."
    )
}

pub fn render_tui_tool_usage() -> &'static str {
    "Usage: /tool <request>\nExample: /tool create file notes.txt"
}

fn submit_tui_input<P>(
    shell: &mut elgar_tui::TuiShell,
    runtime: &AgentRuntime<P>,
    action_gate: &ActionGate<P>,
    session: &mut Session,
    input: &str,
) where
    P: ControllerProvider,
{
    if is_tui_approval_command(input) {
        shell.submit_approval(action_gate, session);
    } else if is_tui_rejection_command(input) {
        shell.submit_rejection(action_gate, session);
    } else if is_tui_cancel_command(input) {
        shell.push_local_message("No active provider turn to cancel.");
    } else if let Some(argument) = tui_permission_command_argument(input) {
        let message = shell.apply_permission_command(argument);
        shell.push_local_message(message);
    } else if let Some(tool_request) = tui_tool_command_argument(input) {
        if tool_request.is_empty() {
            shell.push_local_message(render_tui_tool_usage());
        } else {
            shell.submit_agent_tool_input(runtime, session, tool_request);
        }
    } else if let Some(command) = tui_unknown_command(input) {
        shell.push_local_message(render_tui_unknown_command(command));
    } else {
        shell.submit_agent_input(runtime, session, input);
    }
}

pub fn render_tui_help() -> &'static str {
    "Commands\nSession\n  /status              Show session status\n  /tokens              Show token and context usage\n  /memory              Show verified memory\n  /state               Show verified state snapshot\n  /plan                Preview latest structured plan\n  /plan preview        Preview latest structured plan\n  /created             Show verified creations\n  /pending             Show pending action\n  /reasoning           Show latest reasoning trace\n  /trace               Show latest reasoning trace\nActions\n  /tool <request>      Run an explicit tool-enabled turn\n  /approve             Apply the pending action\n  /reject              Reject the pending action\n  /cancel              Cancel the active provider turn\nPolicy\n  /permissions         Show permission mode\n  /permissions next    Cycle permission mode\n  /permissions <mode>  Set permission mode\nView\n  /clear               Clear the visible conversation\n  /new                 Clear the visible conversation\n  /copy                Copy the conversation\n  /help                Show commands\n  /commands            Show commands\nExit\n  /exit                Quit\n  /quit                Quit\n  /q                   Quit"
}

pub fn render_tui_script<I, S>(
    inputs: I,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    render_tui_script_with_policy(inputs, project_root, cwd, default_permission_policy_mode())
}

pub fn render_tui_script_with_policy<I, S>(
    inputs: I,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let action_gate = ActionGate::default();
    let runtime = AgentRuntime::default();
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::with_policy_mode(policy_mode);
    let mut rendered_turns = Vec::new();

    for input in inputs {
        let input = input.as_ref();
        if is_tui_exit_command(input) {
            break;
        }

        if is_tui_help_command(input) {
            rendered_turns.push(render_tui_help().to_string());
        } else if is_tui_clear_command(input) {
            shell.clear_conversation();
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_cancel_command(input) {
            shell.push_local_message("No active provider turn to cancel.");
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_copy_command(input) {
            rendered_turns.push(shell.conversation_copy_text());
        } else if is_tui_state_snapshot_command(input) {
            rendered_turns.push(elgar_tui::render_session_state_snapshot(&session));
        } else if is_tui_status_command(input) {
            rendered_turns.push(elgar_tui::render_session_status(&session));
        } else if is_tui_tokens_command(input) {
            rendered_turns.push(elgar_tui::render_session_tokens(&session));
        } else if is_tui_pending_command(input) {
            rendered_turns.push(elgar_tui::render_session_pending_action(&session));
        } else if is_tui_created_command(input) {
            rendered_turns.push(elgar_tui::render_session_created_actions(&session));
        } else if is_tui_memory_command(input) {
            rendered_turns.push(elgar_tui::render_session_memory(&session));
        } else if is_tui_plan_preview_command(input) {
            rendered_turns.push(elgar_tui::render_session_plan_preview(&session));
        } else if is_tui_reasoning_command(input) {
            rendered_turns.push(elgar_tui::render_session_reasoning(&session));
        } else {
            submit_tui_input(&mut shell, &runtime, &action_gate, &mut session, input);
            rendered_turns.push(render_tui_turn_with_observability(&shell, &session));
        }
    }

    rendered_turns.join("\n")
}

pub fn run_tui_loop<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    run_tui_loop_with_policy(
        reader,
        writer,
        project_root,
        cwd,
        default_permission_policy_mode(),
    )
}

pub fn run_tui_loop_with_policy<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    run_tui_loop_with_runtime(
        reader,
        writer,
        project_root,
        cwd,
        AgentRuntime::default(),
        None,
        policy_mode,
    )
}

pub fn run_tui_loop_from_runtime_config_with_policy<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();
    match load_runtime_provider(project_root_ref).map_err(runtime_provider_config_io_error)? {
        Some(runtime_provider) => {
            let context_window_tokens = runtime_provider.config.configured_context_window_tokens();
            run_tui_loop_with_runtime(
                reader,
                writer,
                project_root_ref,
                cwd_ref,
                AgentRuntime::with_lm_studio_provider(runtime_provider.config),
                context_window_tokens,
                policy_mode,
            )
        }
        None => run_tui_loop_with_runtime(
            reader,
            writer,
            project_root_ref,
            cwd_ref,
            AgentRuntime::default(),
            None,
            policy_mode,
        ),
    }
}

fn runtime_provider_config_io_error(error: RuntimeProviderConfigError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

pub(crate) fn run_tui_loop_with_runtime<R, W, P>(
    reader: R,
    mut writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    runtime: AgentRuntime<P>,
    context_window_tokens: Option<u64>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    P: ControllerProvider + Clone,
{
    let action_gate = ActionGate::new(runtime.provider.clone());
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    runtime.refresh_context_accounting(&mut session, context_window_tokens);
    let mut shell = elgar_tui::TuiShell::with_policy_mode(policy_mode);

    writeln!(writer, "Elgar TUI. Type /exit, /quit, or /q to leave.")?;
    for line in reader.lines() {
        let input = line?;
        if is_tui_exit_command(&input) {
            writeln!(writer, "Exiting Elgar TUI.")?;
            break;
        }

        if is_tui_help_command(&input) {
            writeln!(writer, "{}", render_tui_help())?;
        } else if is_tui_clear_command(&input) {
            shell.clear_conversation();
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_cancel_command(&input) {
            shell.push_local_message("No active provider turn to cancel.");
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_copy_command(&input) {
            writeln!(writer, "{}", shell.conversation_copy_text())?;
        } else if is_tui_state_snapshot_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_state_snapshot(&session)
            )?;
        } else if is_tui_status_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_status(&session))?;
        } else if is_tui_tokens_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_tokens(&session))?;
        } else if is_tui_pending_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_pending_action(&session)
            )?;
        } else if is_tui_created_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_created_actions(&session)
            )?;
        } else if is_tui_memory_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_memory(&session))?;
        } else if is_tui_plan_preview_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_plan_preview(&session)
            )?;
        } else if is_tui_reasoning_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_reasoning(&session))?;
        } else {
            runtime.refresh_context_accounting(&mut session, context_window_tokens);
            submit_tui_input(&mut shell, &runtime, &action_gate, &mut session, &input);
            writeln!(
                writer,
                "{}",
                render_tui_turn_with_observability(&shell, &session)
            )?;
        }
    }

    Ok(())
}

fn render_tui_turn_with_observability(shell: &elgar_tui::TuiShell, session: &Session) -> String {
    format!(
        "{}\n{}",
        shell.render_scripted_transcript(),
        elgar_tui::render_session_observability(session)
    )
}

pub fn run_tui_terminal() -> io::Result<()> {
    let paths = RuntimePaths::from_current_dir();
    let policy_mode = runtime_permission_policy_mode(&paths.project_root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    match load_runtime_provider(&paths.project_root) {
        Ok(Some(runtime)) => elgar_tui::run_terminal_shell_with_lm_studio_provider_at_with_policy(
            runtime.config,
            &paths.project_root,
            &paths.cwd,
            policy_mode,
        ),
        Ok(None) => elgar_tui::run_terminal_shell_at_with_policy(
            &paths.project_root,
            &paths.cwd,
            policy_mode,
        ),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    }
}
