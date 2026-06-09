//! Terminal raw-mode guard.
//!
//! Entering raw mode lets Elgar read keys directly. Dropping this guard returns
//! the terminal to normal shell behavior.
//!
//! This is terminal raw mode, not model routing. It only controls keyboard IO.

use std::io::{self, Write};

use crossterm::{
    event::{
        DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

use crate::terminal::{ANSI_CURSOR_HIDE, ANSI_CURSOR_SHOW};

/// Restores terminal mode automatically when a prompt exits.
pub(crate) struct TerminalModeGuard;

impl TerminalModeGuard {
    pub(crate) fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        write!(stdout, "{ANSI_CURSOR_HIDE}")?;
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(stdout, PopKeyboardEnhancementFlags, DisableBracketedPaste);
        let _ = write!(stdout, "{ANSI_CURSOR_SHOW}");
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}
