use std::io::{self, Write};

use crossterm::terminal::size as terminal_size;
use elgar_core::provider::ProviderStreamChunk;

use super::{TerminalShellContext, ANSI_CYAN, ANSI_MUTED, ANSI_RESET};

pub(super) const LIVE_REASONING_PREVIEW_BYTES: usize = 1024;
pub(super) const LIVE_RESPONSE_PREVIEW_BYTES: usize = 4096;

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

    pub(super) fn render(
        &mut self,
        tick: usize,
        elapsed_secs: u64,
        input: &str,
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
        ) = active_working_frame_lines(
            &self.context,
            tick,
            elapsed_secs,
            input,
            live_output,
            width,
        );

        for line in &thinking_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &reasoning_lines {
            write!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}\r\n")?;
        }
        for line in &response_lines {
            write!(io::stdout(), "{ANSI_CYAN}{line}{ANSI_RESET}\r\n")?;
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
            + reasoning_lines.len()
            + response_lines.len()
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LiveProviderOutput {
    reasoning: String,
    response: String,
    response_visible_chars: usize,
}

impl LiveProviderOutput {
    pub(super) fn push_chunk(&mut self, chunk: ProviderStreamChunk) {
        match chunk {
            ProviderStreamChunk::Reasoning(value) => {
                append_capped(&mut self.reasoning, &value, LIVE_REASONING_PREVIEW_BYTES);
            }
            ProviderStreamChunk::Text(value) => {
                append_capped(&mut self.response, &value, LIVE_RESPONSE_PREVIEW_BYTES);
                let total = self.response.chars().count();
                self.response_visible_chars = self.response_visible_chars.min(total);
            }
        }
    }

    pub(super) fn advance_response_reveal(&mut self) {
        let total = self.response.chars().count();
        if self.response_visible_chars < total {
            self.response_visible_chars = (self.response_visible_chars + 8).min(total);
        }
    }

    fn reasoning_summary(&self) -> Option<String> {
        compact_streaming_text(&self.reasoning)
    }

    fn response_preview(&self) -> Option<String> {
        compact_streaming_text(&self.visible_response())
    }

    fn visible_response(&self) -> String {
        self.response
            .chars()
            .take(self.response_visible_chars)
            .collect()
    }

    #[cfg(test)]
    pub(super) fn reasoning_preview_bytes(&self) -> usize {
        self.reasoning.len()
    }

    #[cfg(test)]
    pub(super) fn response_preview_bytes(&self) -> usize {
        self.response.len()
    }
}

fn append_capped(target: &mut String, value: &str, max_bytes: usize) {
    target.push_str(value);
    if target.len() <= max_bytes {
        return;
    }

    let mut keep_from = target.len().saturating_sub(max_bytes);
    while keep_from < target.len() && !target.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    target.drain(..keep_from);
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
    _tick: usize,
    _elapsed_secs: u64,
    input: &str,
    live_output: &LiveProviderOutput,
    width: usize,
) -> (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
) {
    let progress_lines = Vec::new();
    let reasoning_lines = live_output
        .reasoning_summary()
        .map(|line| non_empty_lines(wrap_words(&line, drawable_width(width))))
        .unwrap_or_default();
    let response_lines = live_output
        .response_preview()
        .map(|line| non_empty_lines(wrap_words(&line, drawable_width(width))))
        .unwrap_or_default();
    let (top_lines, input_lines, bottom_lines, footer_lines) =
        inline_prompt_frame_lines(context, input, width);
    (
        progress_lines,
        reasoning_lines,
        response_lines,
        top_lines,
        input_lines,
        bottom_lines,
        footer_lines,
    )
}

fn compact_streaming_text(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(240).collect())
    }
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
