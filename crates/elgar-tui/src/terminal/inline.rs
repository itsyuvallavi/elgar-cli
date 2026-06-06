//! Inline terminal startup rendering.
//!
//! The interactive loop now lives across `input/`, `turn/`, and `ui/`. This
//! file only prints the initial terminal banner before the first prompt.

use std::io::{self, Write};

use crate::terminal::{
    ui::{
        prompt::{frame_separator_line, terminal_width},
        render::render_terminal_startup,
    },
    TerminalShellContext, ANSI_BOLD, ANSI_CYAN, ANSI_MUTED, ANSI_RESET,
};

/// Prints the initial terminal banner before the first prompt is shown.
pub(super) fn print_inline_startup(context: &TerminalShellContext) -> io::Result<()> {
    writeln!(io::stdout())?;
    writeln!(
        io::stdout(),
        "{ANSI_MUTED}{}{ANSI_RESET}",
        frame_separator_line(terminal_width())
    )?;
    for line in render_terminal_startup(context).lines() {
        if line.starts_with("elgar") || line.starts_with('[') {
            writeln!(io::stdout(), "{ANSI_CYAN}{ANSI_BOLD}{line}{ANSI_RESET}")?;
        } else if line.trim().is_empty() {
            writeln!(io::stdout())?;
        } else {
            writeln!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}")?;
        }
    }
    writeln!(io::stdout())?;
    io::stdout().flush()
}
