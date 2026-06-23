//! Submitted-input logging helpers.

use elgar_core::{
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

use crate::terminal::commands::TerminalCommand;

pub(super) fn log_input_classified(
    session: &Session,
    turn_id: u64,
    submitted: &str,
    command: &TerminalCommand<'_>,
) {
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
            "classification": terminal_command_name(command)
        })),
    );
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
        TerminalCommand::Approve => "approve",
        TerminalCommand::ApproveContinue => "approve_continue",
        TerminalCommand::Deny => "deny",
        TerminalCommand::Permissions(_) => "permissions",
        TerminalCommand::DetailsLast => "details_last",
        TerminalCommand::Unknown(_) => "unknown",
        TerminalCommand::Text(_) => "plain_text",
    }
}
