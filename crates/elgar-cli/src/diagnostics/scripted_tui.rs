//! Line-based scripted TUI mode.
//!
//! This is not the real interactive terminal UI. It reads one line at a time
//! from stdin, applies local slash commands, and prints a transcript. It is
//! useful for tests and dogfood scripts.

use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use elgar_core::{
    harness::{approve_pending_approval, deny_pending_approval},
    provider::{ControllerProvider, LmStudioProvider, ProviderStub},
    session::Session,
};

use crate::{load_runtime_provider, runtime_session_id, RuntimeProviderConfigError};

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
    matches!(input.trim(), "/deny" | "/reject")
}

pub fn is_tui_copy_command(input: &str) -> bool {
    input.trim() == "/copy"
}

pub fn is_tui_copy_raw_command(input: &str) -> bool {
    matches!(input.trim(), "/copy raw" | "/copy details")
}

pub fn is_tui_details_command(input: &str) -> bool {
    matches!(input.trim(), "/details" | "/details last")
}

pub fn is_tui_memory_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_state_snapshot_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_status_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_pending_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_created_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_plan_preview_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_reasoning_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn is_tui_tokens_command(input: &str) -> bool {
    let _ = input;
    false
}

pub fn tui_tool_command_argument(input: &str) -> Option<&str> {
    let _ = input;
    None
}

pub fn tui_permission_command_argument(input: &str) -> Option<Option<&str>> {
    let _ = input;
    None
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
        || is_tui_copy_command(trimmed)
        || is_tui_copy_raw_command(trimmed)
        || is_tui_details_command(trimmed)
        || is_tui_approval_command(trimmed)
        || is_tui_rejection_command(trimmed)
        || is_tui_clear_command(trimmed)
        || is_tui_cancel_command(trimmed)
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
    "Unknown command: /tool\nPlain text now sends one harness-controlled model turn."
}

/// Applies one submitted scripted input to the shell/session.
fn submit_tui_input<P>(
    shell: &mut elgar_tui::TuiShell,
    provider: &P,
    session: &mut Session,
    input: &str,
) where
    P: ControllerProvider,
{
    if is_tui_cancel_command(input) {
        shell.push_local_message("No active provider turn to cancel.");
    } else if is_tui_approval_command(input) {
        match approve_pending_approval(session) {
            Ok(result) => shell.push_local_message(result.message),
            Err(error) => shell.push_local_message(error.to_string()),
        }
    } else if is_tui_rejection_command(input) {
        match deny_pending_approval(session) {
            Ok(result) => shell.push_local_message(result.message),
            Err(error) => shell.push_local_message(error.to_string()),
        }
    } else if is_tui_details_command(input) {
        shell.push_latest_raw_details();
    } else if let Some(command) = tui_unknown_command(input) {
        shell.push_local_message(render_tui_unknown_command(command));
    } else {
        shell.submit_harness_input(provider, session, input);
    }
}

/// Renders the local help text for scripted TUI commands.
pub fn render_tui_help() -> &'static str {
    "Commands\nChat\n  plain text           Send one harness-controlled model turn\n  /cancel              Cancel the active provider turn\nApproval\n  /approve             Approve and execute the pending risky primitive\n  /deny                Deny the pending risky primitive\n  /reject              Deny the pending risky primitive\nView\n  /clear               Clear the visible conversation\n  /new                 Clear the visible conversation\n  /details last        Show latest hidden details\n  /copy                Copy the conversation\n  /copy raw            Copy hidden details\n  /help                Show commands\n  /commands            Show commands\nExit\n  /exit                Quit\n  /quit                Quit\n  /q                   Quit"
}

/// Runs scripted inputs against the stub provider and returns the transcript.
pub fn render_tui_script<I, S>(
    inputs: I,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let provider = ProviderStub::default();
    let session_id = runtime_session_id("cli-tui-script");
    let mut session = Session::new(&session_id, project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::new();
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
        } else if is_tui_approval_command(input) {
            match approve_pending_approval(&mut session) {
                Ok(result) => shell.push_local_message(result.message),
                Err(error) => shell.push_local_message(error.to_string()),
            }
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_rejection_command(input) {
            match deny_pending_approval(&mut session) {
                Ok(result) => shell.push_local_message(result.message),
                Err(error) => shell.push_local_message(error.to_string()),
            }
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_copy_command(input) {
            rendered_turns.push(shell.conversation_copy_text());
        } else if is_tui_copy_raw_command(input) {
            rendered_turns.push(
                shell
                    .raw_details_copy_text()
                    .unwrap_or_else(|| "No raw details are available.".to_string()),
            );
        } else if is_tui_details_command(input) {
            shell.push_latest_raw_details();
            rendered_turns.push(shell.render_scripted_transcript());
        } else {
            submit_tui_input(&mut shell, &provider, &mut session, input);
            rendered_turns.push(render_tui_turn(&shell));
        }
    }

    rendered_turns.join("\n")
}

/// Runs the line-based TUI with the stub provider.
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
    run_tui_loop_with_runtime(reader, writer, project_root, cwd, ProviderStub::default())
}

/// Runs the line-based TUI using runtime provider config when available.
pub fn run_tui_loop_from_runtime_config<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();
    match load_runtime_provider(project_root_ref).map_err(runtime_provider_config_io_error)? {
        Some(runtime_provider) => run_tui_loop_with_runtime(
            reader,
            writer,
            project_root_ref,
            cwd_ref,
            LmStudioProvider::new(runtime_provider.config),
        ),
        None => run_tui_loop_with_runtime(
            reader,
            writer,
            project_root_ref,
            cwd_ref,
            ProviderStub::default(),
        ),
    }
}

fn runtime_provider_config_io_error(error: RuntimeProviderConfigError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

/// Shared line-loop implementation used by tests and runtime config dispatch.
pub(crate) fn run_tui_loop_with_runtime<R, W, P>(
    reader: R,
    mut writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    provider: P,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    P: ControllerProvider + Clone,
{
    let session_id = runtime_session_id("cli-tui");
    let mut session = Session::new(&session_id, project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::new();

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
        } else if is_tui_approval_command(&input) {
            match approve_pending_approval(&mut session) {
                Ok(result) => shell.push_local_message(result.message),
                Err(error) => shell.push_local_message(error.to_string()),
            }
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_rejection_command(&input) {
            match deny_pending_approval(&mut session) {
                Ok(result) => shell.push_local_message(result.message),
                Err(error) => shell.push_local_message(error.to_string()),
            }
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_copy_command(&input) {
            writeln!(writer, "{}", shell.conversation_copy_text())?;
        } else if is_tui_copy_raw_command(&input) {
            writeln!(
                writer,
                "{}",
                shell
                    .raw_details_copy_text()
                    .unwrap_or_else(|| "No raw details are available.".to_string())
            )?;
        } else if is_tui_details_command(&input) {
            shell.push_latest_raw_details();
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else {
            submit_tui_input(&mut shell, &provider, &mut session, &input);
            writeln!(writer, "{}", render_tui_turn(&shell))?;
        }
    }

    Ok(())
}

fn render_tui_turn(shell: &elgar_tui::TuiShell) -> String {
    shell.render_scripted_transcript()
}
