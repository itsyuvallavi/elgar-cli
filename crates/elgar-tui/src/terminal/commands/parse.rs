//! Turns submitted terminal text into a typed command.
//!
//! This file only classifies input. It does not execute commands.

use super::TerminalCommand;

pub fn parse_terminal_command(input: &str) -> TerminalCommand<'_> {
    let trimmed = input.trim();
    match trimmed {
        "" => TerminalCommand::Empty,
        "/help" | "/commands" => TerminalCommand::Help,
        "/clear" | "/new" => TerminalCommand::Clear,
        "/cancel" => TerminalCommand::Cancel,
        "/approve" => TerminalCommand::Approve,
        "/approve continue" => TerminalCommand::ApproveContinue,
        "/deny" | "/reject" => TerminalCommand::Deny,
        "/details" | "/details last" => TerminalCommand::DetailsLast,
        "/copy" => TerminalCommand::Copy,
        "/copy raw" | "/copy details" => TerminalCommand::CopyRaw,
        command if command == "/permissions" || command.starts_with("/permissions ") => {
            TerminalCommand::Permissions(command.trim_start_matches("/permissions").trim())
        }
        "/exit" | "/quit" | "/q" => TerminalCommand::Exit,
        command if command.starts_with('/') => TerminalCommand::Unknown(command),
        text => TerminalCommand::Text(text),
    }
}
