//! Tests for terminal command parsing.

use super::super::{parse_terminal_command, TerminalCommand};

#[test]
fn parses_local_commands() {
    assert_eq!(parse_terminal_command(""), TerminalCommand::Empty);
    assert_eq!(parse_terminal_command("/help"), TerminalCommand::Help);
    assert_eq!(parse_terminal_command("/clear"), TerminalCommand::Clear);
    assert_eq!(parse_terminal_command("/cancel"), TerminalCommand::Cancel);
    assert_eq!(parse_terminal_command("/approve"), TerminalCommand::Approve);
    assert_eq!(parse_terminal_command("/deny"), TerminalCommand::Deny);
    assert_eq!(parse_terminal_command("/reject"), TerminalCommand::Deny);
    assert_eq!(
        parse_terminal_command("/details"),
        TerminalCommand::DetailsLast
    );
    assert_eq!(parse_terminal_command("/copy"), TerminalCommand::Copy);
    assert_eq!(
        parse_terminal_command("/copy raw"),
        TerminalCommand::CopyRaw
    );
    assert_eq!(parse_terminal_command("/exit"), TerminalCommand::Exit);
}

#[test]
fn parses_plain_text_and_unknown_slash_commands() {
    assert_eq!(
        parse_terminal_command("/raw"),
        TerminalCommand::Unknown("/raw")
    );
    assert_eq!(
        parse_terminal_command(" /raw hello "),
        TerminalCommand::Unknown("/raw hello")
    );
    assert_eq!(
        parse_terminal_command("raw hello"),
        TerminalCommand::Text("raw hello")
    );
}
