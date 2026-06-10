//! Slash-command helpers for scripted TUI mode.

use elgar_tui::terminal::{
    parse_terminal_command, render_terminal_help, render_unknown_command, TerminalCommand,
};

pub(super) fn parse_scripted_command(input: &str) -> TerminalCommand<'_> {
    parse_terminal_command(input)
}

pub fn is_tui_exit_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Exit)
}

pub fn is_tui_help_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Help)
}

pub fn is_tui_approval_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Approve)
}

pub fn is_tui_rejection_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Deny)
}

pub fn is_tui_copy_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Copy)
}

pub fn is_tui_copy_raw_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::CopyRaw)
}

pub fn is_tui_details_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::DetailsLast)
}

pub fn is_tui_clear_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Clear)
}

pub fn is_tui_cancel_command(input: &str) -> bool {
    matches!(parse_terminal_command(input), TerminalCommand::Cancel)
}

pub fn tui_unknown_command(input: &str) -> Option<&str> {
    match parse_terminal_command(input) {
        TerminalCommand::Unknown(command) => Some(command),
        _ => None,
    }
}

pub fn render_tui_unknown_command(command: &str) -> String {
    render_unknown_command(command)
}

pub fn render_tui_help() -> &'static str {
    render_terminal_help()
}
