use std::io::{self, Write};
#[cfg(all(not(test), target_os = "macos"))]
use std::process::{Command, Stdio};

use crate::TuiShell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalCommand<'a> {
    Empty,
    Help,
    Clear,
    Approve,
    Reject,
    Cancel,
    Status,
    Pending,
    Created,
    Memory,
    PlanPreview,
    Tool(&'a str),
    Permissions(Option<&'a str>),
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
        "/cancel" => TerminalCommand::Cancel,
        "/status" => TerminalCommand::Status,
        "/pending" => TerminalCommand::Pending,
        "/created" => TerminalCommand::Created,
        "/memory" => TerminalCommand::Memory,
        "/plan" | "/plan preview" => TerminalCommand::PlanPreview,
        "/tool" => TerminalCommand::Tool(""),
        command if command.strip_prefix("/tool ").is_some() => TerminalCommand::Tool(
            command
                .strip_prefix("/tool ")
                .map(str::trim)
                .unwrap_or_default(),
        ),
        "/permissions" | "/policy" => TerminalCommand::Permissions(None),
        command
            if command
                .strip_prefix("/permissions ")
                .or_else(|| command.strip_prefix("/policy "))
                .is_some() =>
        {
            TerminalCommand::Permissions(
                command
                    .strip_prefix("/permissions ")
                    .or_else(|| command.strip_prefix("/policy "))
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
        }
        "/copy" => TerminalCommand::Copy,
        "/exit" | "/quit" | "/q" => TerminalCommand::Exit,
        command if command.starts_with('/') => TerminalCommand::Unknown(command),
        text => TerminalCommand::Text(text),
    }
}

pub(super) fn render_terminal_help() -> &'static str {
    "Commands\n/commands              Show commands\n/clear                 Clear the visible conversation\n/new                   Clear the visible conversation\n/cancel                Cancel the active provider turn\n/approve               Apply the pending action\n/reject                Reject the pending action\n/status                Show session status\n/pending               Show pending action\n/created               Show verified creations\n/memory                Show verified memory\n/plan                  Preview latest structured plan\n/plan preview          Preview latest structured plan\n/tool <request>        Run an explicit tool-enabled turn\n/permissions           Show permission mode\n/permissions next      Cycle permission mode\n/permissions <mode>    Set permission mode\n/copy                  Copy the conversation\n/exit                  Quit\n/quit                  Quit\n/q                     Quit\n/help                  Show commands"
}

pub(super) fn clear_terminal_conversation(shell: &mut TuiShell) {
    shell.clear_conversation();
}

pub(super) fn clear_visible_terminal() -> io::Result<()> {
    write!(io::stdout(), "\x1b[2J\x1b[H")?;
    io::stdout().flush()
}

pub(super) fn copy_conversation_to_terminal_clipboard(
    writer: impl Write,
    shell: &mut TuiShell,
) -> io::Result<()> {
    copy_conversation_with_clipboards(writer, shell, copy_text_to_system_clipboard)
}

pub(super) fn copy_conversation_with_clipboards(
    mut writer: impl Write,
    shell: &mut TuiShell,
    system_clipboard: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    let text = shell.conversation_copy_text();

    match system_clipboard(&text) {
        Ok(()) => {
            shell.copy.mark_copied(text.len());
            Ok(())
        }
        Err(system_error) => match copy_text_with_osc52(&mut writer, &text) {
            Ok(()) => {
                shell.copy.mark_copied(text.len());
                Ok(())
            }
            Err(terminal_error) => {
                let message = format!(
                    "system clipboard failed: {system_error}; terminal fallback failed: {terminal_error}"
                );
                shell.copy.mark_failed(message.clone());
                Err(io::Error::new(terminal_error.kind(), message))
            }
        },
    }
}

fn copy_text_with_osc52(mut writer: impl Write, text: &str) -> io::Result<()> {
    writer.write_all(osc52_clipboard_sequence(text).as_bytes())?;
    writer.flush()
}

#[cfg(not(test))]
fn copy_text_to_system_clipboard(text: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        copy_text_with_command("/usr/bin/pbcopy", text)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no local system clipboard command configured",
        ))
    }
}

#[cfg(test)]
fn copy_text_to_system_clipboard(_text: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "system clipboard disabled in unit tests",
    ))
}

#[cfg(all(not(test), target_os = "macos"))]
fn copy_text_with_command(command: &str, text: &str) -> io::Result<()> {
    let mut child = Command::new(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "clipboard command did not open stdin",
        )
    })?;
    stdin.write_all(text.as_bytes())?;
    drop(stdin);

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "clipboard command exited with {status}"
        )))
    }
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn copy_text_with_command(_command: &str, _text: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no local system clipboard command configured",
    ))
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

#[cfg(test)]
mod tests {
    use super::{parse_terminal_command, render_terminal_help, TerminalCommand};

    #[test]
    fn permissions_command_parses_show_cycle_and_set_forms() {
        assert_eq!(
            parse_terminal_command("/permissions"),
            TerminalCommand::Permissions(None)
        );
        assert_eq!(
            parse_terminal_command("/policy"),
            TerminalCommand::Permissions(None)
        );
        assert_eq!(
            parse_terminal_command("/permissions next"),
            TerminalCommand::Permissions(Some("next"))
        );
        assert_eq!(
            parse_terminal_command(" /policy full-access "),
            TerminalCommand::Permissions(Some("full-access"))
        );
    }

    #[test]
    fn help_lists_permissions_command() {
        let help = render_terminal_help();

        assert!(help.contains("/permissions"));
        assert!(help.contains("/permissions next"));
        assert!(help.contains("/permissions <mode>"));
        assert!(help.contains("/status"));
        assert!(help.contains("/pending"));
        assert!(help.contains("/created"));
    }
}
