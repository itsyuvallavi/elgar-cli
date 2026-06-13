//! Inline prompt rendering.
//!
//! This module owns the editable prompt and live provider preview renderers.
//! Frame construction, live preview state, and wrapping helpers live in
//! submodules to keep each UI responsibility small.

mod frame;
mod live_output;
mod wrap;

use std::io::{self, Write};

use super::render::{write_transcript_line_ansi, CodeLineStyleState};
use crate::terminal::{TerminalShellContext, ANSI_CYAN, ANSI_MUTED, ANSI_RESET};

use frame::{active_working_frame_lines_with_cursor, inline_prompt_frame_lines_with_cursor};
pub(crate) use frame::{drawable_width, frame_separator_line, terminal_width};
pub(crate) use live_output::LiveProviderOutput;
pub(crate) use wrap::{non_empty_lines, wrap_words};

#[derive(Debug, Clone)]
pub(crate) struct InlinePromptRenderer {
    context: TerminalShellContext,
    rows: usize,
}

impl InlinePromptRenderer {
    pub(crate) fn new(context: TerminalShellContext) -> Self {
        Self { context, rows: 0 }
    }

    pub(crate) fn set_context(&mut self, context: TerminalShellContext) {
        self.context = context;
    }

    /// Draw the editable prompt and place the cursor at the current input index.
    pub(crate) fn render_with_cursor(&mut self, input: &str, cursor: usize) -> io::Result<()> {
        self.clear()?;
        let width = terminal_width();
        let (top_lines, input_lines, bottom_lines, footer_lines) =
            inline_prompt_frame_lines_with_cursor(&self.context, input, cursor, width);

        for line in &top_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &input_lines {
            write!(io::stdout(), "{ANSI_CYAN}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &bottom_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        let footer_ansi = self.context.footer_ansi();
        for line in &footer_lines {
            write!(io::stdout(), "{footer_ansi}{line}{ANSI_RESET}\r\n")?;
        }

        self.rows = top_lines.len() + input_lines.len() + bottom_lines.len() + footer_lines.len();
        io::stdout().flush()
    }

    pub(crate) fn clear(&mut self) -> io::Result<()> {
        if self.rows > 0 {
            write!(io::stdout(), "\x1b[{}A\r\x1b[J", self.rows)?;
            self.rows = 0;
        }
        io::stdout().flush()
    }
}

impl Drop for InlinePromptRenderer {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

#[derive(Debug)]
pub(crate) struct InlineWorkingRenderer {
    context: TerminalShellContext,
    rows: usize,
}

impl InlineWorkingRenderer {
    pub(crate) fn new(context: TerminalShellContext) -> Self {
        Self { context, rows: 0 }
    }

    pub(crate) fn render_with_cursor(
        &mut self,
        tick: usize,
        elapsed_secs: u64,
        input: &str,
        cursor: usize,
        live_output: &LiveProviderOutput,
    ) -> io::Result<()> {
        self.clear()?;
        let width = terminal_width();
        let (
            thinking_lines,
            reasoning_lines,
            response_lines,
            top_lines,
            input_lines,
            bottom_lines,
            footer_lines,
        ) = active_working_frame_lines_with_cursor(
            &self.context,
            tick,
            elapsed_secs,
            input,
            cursor,
            live_output,
            width,
        );

        for line in &thinking_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &reasoning_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        let mut code_state = CodeLineStyleState::default();
        for line in &response_lines {
            write_transcript_line_ansi(line, "\r\n", &mut code_state)?;
        }
        for line in &top_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &input_lines {
            write!(io::stdout(), "{ANSI_CYAN}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &bottom_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        let footer_ansi = self.context.footer_ansi();
        for line in &footer_lines {
            write!(io::stdout(), "{footer_ansi}{line}{ANSI_RESET}\r\n")?;
        }

        self.rows = thinking_lines.len()
            + reasoning_lines.len()
            + response_lines.len()
            + top_lines.len()
            + input_lines.len()
            + bottom_lines.len()
            + footer_lines.len();
        io::stdout().flush()
    }

    pub(crate) fn clear(&mut self) -> io::Result<()> {
        if self.rows > 0 {
            write!(io::stdout(), "\x1b[{}A\r\x1b[J", self.rows)?;
            self.rows = 0;
        }
        io::stdout().flush()
    }
}

impl Drop for InlineWorkingRenderer {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}
