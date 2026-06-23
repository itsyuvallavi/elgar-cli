//! Clear helpers for terminal commands.
//!
//! This handles local screen/conversation clearing only.

use std::io::{self, Write};

use crate::TuiShell;

pub(crate) fn clear_terminal_conversation(shell: &mut TuiShell) {
    shell.clear_conversation();
}

pub(crate) fn clear_visible_terminal() -> io::Result<()> {
    write!(io::stdout(), "\x1b[2J\x1b[H")?;
    io::stdout().flush()
}
