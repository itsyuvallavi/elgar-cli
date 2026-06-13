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
    session::{runtime_session_id, Session},
};
use elgar_tui::terminal::TerminalCommand;

use crate::{load_runtime_provider, RuntimeProviderConfigError};

mod commands;
mod render;

pub use commands::{
    is_tui_approval_command, is_tui_cancel_command, is_tui_clear_command, is_tui_copy_command,
    is_tui_copy_raw_command, is_tui_details_command, is_tui_exit_command, is_tui_help_command,
    is_tui_rejection_command, render_tui_help, render_tui_unknown_command, tui_unknown_command,
};

use commands::parse_scripted_command;
use render::render_tui_turn;

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
            session.reset_conversation();
            shell.clear_conversation();
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_cancel_command(input) {
            shell.push_local_message("No active provider turn to cancel.");
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_approval_command(input) {
            approve_scripted(&mut shell, &mut session);
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_rejection_command(input) {
            deny_scripted(&mut shell, &mut session);
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
            rendered_turns.push(render_tui_turn(&shell, &session));
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
pub fn run_tui_loop_with_runtime<R, W, P>(
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
            session.reset_conversation();
            shell.clear_conversation();
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_cancel_command(&input) {
            shell.push_local_message("No active provider turn to cancel.");
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_approval_command(&input) {
            approve_scripted(&mut shell, &mut session);
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_rejection_command(&input) {
            deny_scripted(&mut shell, &mut session);
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
            writeln!(writer, "{}", render_tui_turn(&shell, &session))?;
        }
    }

    Ok(())
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
    match parse_scripted_command(input) {
        TerminalCommand::Cancel => {
            shell.push_local_message("No active provider turn to cancel.");
        }
        TerminalCommand::Approve => approve_scripted(shell, session),
        TerminalCommand::Deny => deny_scripted(shell, session),
        TerminalCommand::DetailsLast => {
            shell.push_latest_raw_details();
        }
        TerminalCommand::Unknown(command) => {
            shell.push_local_message(render_tui_unknown_command(command));
        }
        TerminalCommand::Text(text) => {
            shell.submit_harness_input(provider, session, text);
        }
        TerminalCommand::Clear => {
            session.reset_conversation();
            shell.clear_conversation();
        }
        TerminalCommand::Empty
        | TerminalCommand::Help
        | TerminalCommand::Copy
        | TerminalCommand::CopyRaw
        | TerminalCommand::Exit => {}
    }
}

fn approve_scripted(shell: &mut elgar_tui::TuiShell, session: &mut Session) {
    match approve_pending_approval(session) {
        Ok(result) => shell.push_local_message(result.message),
        Err(error) => shell.push_local_message(error.to_string()),
    }
}

fn deny_scripted(shell: &mut elgar_tui::TuiShell, session: &mut Session) {
    match deny_pending_approval(session) {
        Ok(result) => shell.push_local_message(result.message),
        Err(error) => shell.push_local_message(error.to_string()),
    }
}
