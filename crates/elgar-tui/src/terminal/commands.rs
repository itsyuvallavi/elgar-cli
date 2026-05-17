use std::io::{self, Write};

use elgar_core::router::{route_input, Route};

use crate::TuiShell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalCommand<'a> {
    Empty,
    Help,
    Clear,
    Approve,
    Reject,
    Copy,
    Exit,
    Unknown(&'a str),
    Text(&'a str),
}

pub(super) fn parse_terminal_command(input: &str) -> TerminalCommand<'_> {
    let trimmed = input.trim();
    match trimmed {
        "" => TerminalCommand::Empty,
        "/help" | "/commands" => TerminalCommand::Help,
        "/clear" | "/new" => TerminalCommand::Clear,
        "/approve" => TerminalCommand::Approve,
        "/reject" => TerminalCommand::Reject,
        "/copy" => TerminalCommand::Copy,
        "/exit" | "/quit" | "/q" => TerminalCommand::Exit,
        command if command.starts_with('/') => TerminalCommand::Unknown(command),
        text => TerminalCommand::Text(text),
    }
}

pub(super) fn terminal_text_starts_provider_turn(text: &str) -> bool {
    matches!(route_input(text), Route::AskModel | Route::Unknown)
}

pub(super) fn render_terminal_help() -> &'static str {
    "Commands\n/commands  Show commands\n/clear     Clear the visible conversation\n/new       Clear the visible conversation\n/approve   Apply the pending action\n/reject    Reject the pending action\n/copy      Copy the conversation\n/exit      Quit\n/quit      Quit\n/q         Quit\n/help      Show commands"
}

pub(super) fn clear_terminal_conversation(shell: &mut TuiShell) {
    shell.clear_conversation();
}

pub(super) fn clear_visible_terminal() -> io::Result<()> {
    write!(io::stdout(), "\x1b[2J\x1b[H")?;
    io::stdout().flush()
}

pub(super) fn copy_conversation_to_terminal_clipboard(
    mut writer: impl Write,
    shell: &mut TuiShell,
) -> io::Result<()> {
    let text = shell.conversation_copy_text();
    let result = writer
        .write_all(osc52_clipboard_sequence(&text).as_bytes())
        .and_then(|_| writer.flush());

    match result {
        Ok(()) => {
            shell.copy.mark_copied(text.len());
            Ok(())
        }
        Err(error) => {
            shell.copy.mark_failed(error.to_string());
            Err(error)
        }
    }
}

pub(super) fn osc52_clipboard_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", encode_base64(text.as_bytes()))
}

pub(super) fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let triple = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);

        encoded.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}
