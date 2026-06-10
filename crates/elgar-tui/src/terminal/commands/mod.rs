//! Slash-command support for the terminal UI.
//!
//! This folder keeps command parsing separate from command helpers like copy,
//! clear, and help text.

mod clear;
mod clipboard;
mod messages;
mod parse;

#[cfg(test)]
mod tests;

pub(super) use clear::{clear_terminal_conversation, clear_visible_terminal};
pub(super) use clipboard::{
    copy_conversation_to_terminal_clipboard, copy_raw_details_to_terminal_clipboard,
};
pub use messages::{render_terminal_help, render_unknown_command};
pub use parse::parse_terminal_command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCommand<'a> {
    Empty,
    Help,
    Clear,
    Cancel,
    Approve,
    Deny,
    DetailsLast,
    Copy,
    CopyRaw,
    Exit,
    Unknown(&'a str),
    Text(&'a str),
}
