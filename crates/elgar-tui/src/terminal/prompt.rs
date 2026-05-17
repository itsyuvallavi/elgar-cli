use std::io::{self, Write};

use crossterm::terminal::size as terminal_size;

use super::{TerminalShellContext, ANSI_CYAN, ANSI_MUTED, ANSI_RESET};

pub(super) fn non_empty_lines(lines: Vec<String>) -> Vec<String> {
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

#[derive(Debug, Clone)]
pub(super) struct InlinePromptRenderer {
    context: TerminalShellContext,
    rows: usize,
}

impl InlinePromptRenderer {
    pub(super) fn new(context: TerminalShellContext) -> Self {
        Self { context, rows: 0 }
    }

    pub(super) fn render(&mut self, input: &str) -> io::Result<()> {
        self.clear()?;
        let width = terminal_width();
        let (top_lines, input_lines, bottom_lines, footer_lines) =
            inline_prompt_frame_lines(&self.context, input, width);

        for line in &top_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &input_lines {
            write!(io::stdout(), "{ANSI_CYAN}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &bottom_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &footer_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }

        self.rows = top_lines.len() + input_lines.len() + bottom_lines.len() + footer_lines.len();
        io::stdout().flush()
    }

    pub(super) fn clear(&mut self) -> io::Result<()> {
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
pub(super) struct InlineWorkingRenderer {
    context: TerminalShellContext,
    rows: usize,
}

impl InlineWorkingRenderer {
    pub(super) fn new(context: TerminalShellContext) -> Self {
        Self { context, rows: 0 }
    }

    pub(super) fn render(&mut self, tick: usize, elapsed_secs: u64) -> io::Result<()> {
        self.clear()?;
        let width = terminal_width();
        let (thinking_lines, top_lines, input_lines, bottom_lines, footer_lines) =
            active_working_frame_lines(&self.context, tick, elapsed_secs, width);

        for line in &thinking_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
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
        for line in &footer_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }

        self.rows = thinking_lines.len()
            + top_lines.len()
            + input_lines.len()
            + bottom_lines.len()
            + footer_lines.len();
        io::stdout().flush()
    }

    pub(super) fn clear(&mut self) -> io::Result<()> {
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

pub(super) fn inline_prompt_frame_lines(
    context: &TerminalShellContext,
    input: &str,
    width: usize,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    (
        vec![String::new(), frame_separator_line(width)],
        prompt_input_lines(input, width),
        vec![frame_separator_line(width)],
        context
            .footer_body_for_width(drawable_width(width))
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

pub(super) fn active_working_frame_lines(
    context: &TerminalShellContext,
    tick: usize,
    elapsed_secs: u64,
    width: usize,
) -> (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
) {
    let marker = working_marker(tick);
    let line = format!("{marker} thinking {elapsed_secs}s");
    let thinking_lines = non_empty_lines(wrap_words(&line, drawable_width(width)));
    let (top_lines, input_lines, bottom_lines, footer_lines) =
        inline_prompt_frame_lines(context, "", width);
    (
        thinking_lines,
        top_lines,
        input_lines,
        bottom_lines,
        footer_lines,
    )
}

fn working_marker(tick: usize) -> &'static str {
    const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
    FRAMES[tick % FRAMES.len()]
}

fn prompt_input_lines(input: &str, width: usize) -> Vec<String> {
    let width = drawable_width(width);
    let prefix = "▸ ";
    let continuation = "  ";
    let first_width = width.saturating_sub(prefix.chars().count()).max(1);
    let continuation_width = width.saturating_sub(continuation.chars().count()).max(1);
    let wrapped = non_empty_lines(wrap_words(input, first_width));
    let mut lines = Vec::new();
    for (index, line) in wrapped.into_iter().enumerate() {
        if index == 0 {
            lines.push(format!("{prefix}{line}"));
        } else {
            for continuation_line in non_empty_lines(wrap_words(&line, continuation_width)) {
                lines.push(format!("{continuation}{continuation_line}"));
            }
        }
    }
    lines
}

pub(super) fn frame_separator_line(width: usize) -> String {
    "─".repeat(drawable_width(width))
}

pub(super) fn terminal_width() -> usize {
    terminal_size()
        .ok()
        .map(|(width, _)| usize::from(width))
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

pub(super) fn drawable_width(width: usize) -> usize {
    width.saturating_sub(1).max(1)
}

pub(super) fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in raw_line.split_whitespace() {
            let word_len = word.chars().count();
            let line_len = line.chars().count();
            if line.is_empty() {
                if word_len <= width {
                    line.push_str(word);
                } else {
                    lines.extend(split_long_word(word, width));
                }
            } else if line_len + 1 + word_len <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut line));
                if word_len <= width {
                    line.push_str(word);
                } else {
                    lines.extend(split_long_word(word, width));
                }
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for character in word.chars() {
        if chunk.chars().count() == width {
            chunks.push(std::mem::take(&mut chunk));
        }
        chunk.push(character);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}
