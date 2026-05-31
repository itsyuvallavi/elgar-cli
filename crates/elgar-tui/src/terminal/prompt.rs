use std::io::{self, Write};

use crossterm::terminal::size as terminal_size;
#[cfg(test)]
use elgar_core::provider::ProviderStreamChunk;
use elgar_core::provider_visible_text_from_text_only_output;

use crate::{markdown::render_assistant_markdown, reasoning::format_provider_reasoning_summary};

use super::{transcript_output_ansi, TerminalShellContext, ANSI_CYAN, ANSI_MUTED, ANSI_RESET};

#[cfg(test)]
pub(super) const LIVE_REASONING_PREVIEW_BYTES: usize = 1024;
#[cfg(test)]
pub(super) const LIVE_RESPONSE_PREVIEW_BYTES: usize = 4096;
const LIVE_REASONING_SUMMARY_CHARS: usize = 160;

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

    pub(super) fn render_with_cursor(&mut self, input: &str, cursor: usize) -> io::Result<()> {
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

    pub(super) fn render_with_cursor(
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
        for line in &response_lines {
            write!(
                io::stdout(),
                "{}{line}{ANSI_RESET}\r\n",
                live_response_ansi()
            )?;
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

pub(super) fn live_response_ansi() -> &'static str {
    transcript_output_ansi()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LiveProviderOutput {
    reasoning: String,
    response: String,
    suppress_reasoning_preview: bool,
    suppress_response_preview: bool,
}

impl LiveProviderOutput {
    pub(super) fn suppress_reasoning_preview(&mut self) {
        self.suppress_reasoning_preview = true;
    }

    pub(super) fn suppress_response_preview(&mut self) {
        self.suppress_response_preview = true;
    }

    #[cfg(test)]
    pub(super) fn push_chunk(&mut self, chunk: ProviderStreamChunk) {
        match chunk {
            ProviderStreamChunk::Reasoning(value) => {
                append_capped(&mut self.reasoning, &value, LIVE_REASONING_PREVIEW_BYTES);
            }
            ProviderStreamChunk::Text(value) => {
                append_capped(&mut self.response, &value, LIVE_RESPONSE_PREVIEW_BYTES);
            }
        }
    }

    fn reasoning_summary(&self) -> Option<String> {
        if self.suppress_reasoning_preview {
            return None;
        }

        compact_streaming_text(&self.reasoning)
            .and_then(|text| format_provider_reasoning_summary(&text, LIVE_REASONING_SUMMARY_CHARS))
    }

    fn response_preview(&self) -> Option<String> {
        if self.suppress_response_preview {
            return None;
        }

        let visible = provider_visible_text_from_text_only_output(self.response.clone())?;
        let rendered = render_assistant_markdown(&visible);
        if rendered.trim().is_empty() {
            None
        } else {
            Some(rendered)
        }
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

#[cfg(test)]
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

#[cfg(test)]
pub(super) fn inline_prompt_frame_lines(
    context: &TerminalShellContext,
    input: &str,
    width: usize,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    inline_prompt_frame_lines_with_cursor(context, input, input.len(), width)
}

pub(super) fn inline_prompt_frame_lines_with_cursor(
    context: &TerminalShellContext,
    input: &str,
    cursor: usize,
    width: usize,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    (
        vec![String::new(), frame_separator_line(width)],
        prompt_input_lines_with_cursor(input, cursor, width),
        vec![frame_separator_line(width)],
        context
            .footer_body_for_width(drawable_width(width))
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

type ActiveWorkingFrameLineGroups = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

#[cfg(test)]
pub(super) fn active_working_frame_lines(
    context: &TerminalShellContext,
    tick: usize,
    elapsed_secs: u64,
    input: &str,
    live_output: &LiveProviderOutput,
    width: usize,
) -> ActiveWorkingFrameLineGroups {
    active_working_frame_lines_with_cursor(
        context,
        tick,
        elapsed_secs,
        input,
        input.len(),
        live_output,
        width,
    )
}

pub(super) fn active_working_frame_lines_with_cursor(
    context: &TerminalShellContext,
    tick: usize,
    elapsed_secs: u64,
    input: &str,
    cursor: usize,
    live_output: &LiveProviderOutput,
    width: usize,
) -> ActiveWorkingFrameLineGroups {
    let reasoning_lines = live_output
        .reasoning_summary()
        .map(|line| with_leading_spacer(non_empty_lines(wrap_words(&line, drawable_width(width)))))
        .unwrap_or_default();
    let response_lines = live_output
        .response_preview()
        .map(|text| with_leading_spacer(rendered_preview_lines(&text, drawable_width(width))))
        .unwrap_or_default();
    let progress_lines = if reasoning_lines.is_empty() && response_lines.is_empty() {
        with_leading_spacer(vec![provider_progress_line(tick, elapsed_secs)])
    } else {
        Vec::new()
    };
    let (top_lines, input_lines, bottom_lines, footer_lines) =
        inline_prompt_frame_lines_with_cursor(context, input, cursor, width);
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

fn provider_progress_line(tick: usize, elapsed_secs: u64) -> String {
    let base = match tick % 4 {
        0 => "Thinking",
        1 => "Thinking.",
        2 => "Thinking..",
        _ => "Thinking...",
    };
    let progress = if elapsed_secs == 0 {
        base.to_string()
    } else {
        format!("{base} {elapsed_secs}s")
    };
    format!("{progress} · /cancel")
}

fn with_leading_spacer(mut lines: Vec<String>) -> Vec<String> {
    let mut spaced = Vec::with_capacity(lines.len() + 1);
    spaced.push(String::new());
    spaced.append(&mut lines);
    spaced
}

fn rendered_preview_lines(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        if preserves_spacing(raw_line) {
            lines.extend(wrap_preserving_spacing(raw_line, width));
        } else {
            lines.extend(wrap_words(raw_line, width));
        }
    }
    lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn preserves_spacing(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn wrap_preserving_spacing(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for character in line.chars() {
        if current.chars().count() >= width {
            lines.push(current);
            current = String::new();
        }
        current.push(character);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn compact_streaming_text(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn prompt_input_lines_with_cursor(input: &str, cursor: usize, width: usize) -> Vec<String> {
    let width = drawable_width(width);
    let prefix = "▸ ";
    let continuation = "  ";
    let first_width = width.saturating_sub(prefix.chars().count()).max(1);
    let continuation_width = width.saturating_sub(continuation.chars().count()).max(1);
    let input = input_with_visual_cursor(input, cursor);
    let wrapped = non_empty_lines(wrap_preserving_spacing(&input, first_width));
    let mut lines = Vec::new();
    for (index, line) in wrapped.into_iter().enumerate() {
        if index == 0 {
            lines.push(format!("{prefix}{line}"));
        } else {
            for continuation_line in
                non_empty_lines(wrap_preserving_spacing(&line, continuation_width))
            {
                lines.push(format!("{continuation}{continuation_line}"));
            }
        }
    }
    lines
}

fn input_with_visual_cursor(input: &str, cursor: usize) -> String {
    let cursor = floor_char_boundary(input, cursor);
    let mut rendered = String::with_capacity(input.len() + "▌".len());
    rendered.push_str(&input[..cursor]);
    rendered.push('▌');
    rendered.push_str(&input[cursor..]);
    rendered
}

fn floor_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
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
