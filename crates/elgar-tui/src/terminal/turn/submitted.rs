//! Handles submitted terminal input while the provider is idle.
//!
//! This file owns local slash-command execution and forwards plain text into
//! the harness-controlled provider-turn path.

use std::io;

use elgar_core::{
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ControllerProvider,
    session::Session,
};

use crate::{
    terminal::{
        commands::{
            clear_terminal_conversation, clear_visible_terminal,
            copy_conversation_to_terminal_clipboard, copy_raw_details_to_terminal_clipboard,
            parse_terminal_command, render_terminal_help, render_unknown_command, TerminalCommand,
        },
        input::keymap::{
            normalize_terminal_provider_text_input, terminal_text_should_run_inline_provider_text,
        },
        turn::provider::run_inline_provider_text_turn,
        ui::render::{print_and_record_local, print_new_conversation_lines, print_plain_block},
    },
    TuiShell,
};

/// Executes one submitted prompt while the provider is idle.
///
/// Local slash commands are handled here. Plain text is forwarded to the
/// harness-controlled provider turn.
pub(crate) fn handle_inline_submission<P>(
    submitted: &str,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<(bool, String)>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let turn_id = session.next_turn_id();
    let command = parse_terminal_command(submitted);
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Input,
            file!(),
            "handle_inline_submission",
            "input_classified",
        )
        .with_metadata(serde_json::json!({
            "submitted_chars": submitted.chars().count(),
            "classification": terminal_command_name(&command)
        })),
    );
    match command {
        TerminalCommand::Empty => Ok((false, String::new())),
        TerminalCommand::Exit => Ok((true, String::new())),
        TerminalCommand::Help => {
            print_and_record_local(shell, render_terminal_help())?;
            Ok((false, String::new()))
        }
        TerminalCommand::Clear => {
            clear_terminal_conversation(shell);
            clear_visible_terminal()?;
            Ok((false, String::new()))
        }
        TerminalCommand::Copy => {
            let mut sink = io::stdout();
            let _ = copy_conversation_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            Ok((false, String::new()))
        }
        TerminalCommand::CopyRaw => {
            let mut sink = io::stdout();
            let _ = copy_raw_details_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            Ok((false, String::new()))
        }
        TerminalCommand::Cancel => {
            print_and_record_local(shell, "No provider request is running.")?;
            Ok((false, String::new()))
        }
        TerminalCommand::DetailsLast => {
            let before = shell.conversation.render_lines_with_styles().len();
            shell.push_latest_raw_details();
            print_new_conversation_lines(shell, before, false, false)?;
            Ok((false, String::new()))
        }
        TerminalCommand::Unknown(command) => {
            print_and_record_local(shell, render_unknown_command(command))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Text(text) => {
            if terminal_text_should_run_inline_provider_text(text) {
                let provider_input = normalize_terminal_provider_text_input(text);
                let preserved_input =
                    run_inline_provider_text_turn(&provider_input, provider, session, shell)?;
                Ok((false, preserved_input))
            } else {
                Ok((false, String::new()))
            }
        }
    }
}

fn terminal_command_name(command: &TerminalCommand<'_>) -> &'static str {
    match command {
        TerminalCommand::Empty => "empty",
        TerminalCommand::Exit => "exit",
        TerminalCommand::Help => "help",
        TerminalCommand::Clear => "clear",
        TerminalCommand::Copy => "copy",
        TerminalCommand::CopyRaw => "copy_raw",
        TerminalCommand::Cancel => "cancel",
        TerminalCommand::DetailsLast => "details_last",
        TerminalCommand::Unknown(_) => "unknown",
        TerminalCommand::Text(_) => "plain_text",
    }
}
