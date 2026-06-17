//! Inline prompt frame construction.

use crossterm::terminal::size as terminal_size;

use crate::terminal::TerminalShellContext;

use super::{
    super::approval_card::render_pending_approval_card,
    live_output::LiveProviderOutput,
    wrap::{non_empty_lines, rendered_preview_lines, wrap_preserving_spacing, wrap_words},
};

pub(super) fn inline_prompt_frame_lines_with_cursor(
    context: &TerminalShellContext,
    input: &str,
    cursor: usize,
    width: usize,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut top_lines = vec![String::new()];
    if let Some(approval) = context.pending_approval.as_ref() {
        top_lines.extend(render_pending_approval_card(
            approval,
            width,
            context.selected_approval_action,
        ));
        top_lines.push(String::new());
    }
    top_lines.push(prompt_separator_line(width));

    (
        top_lines,
        prompt_input_lines_with_cursor(input, cursor, width),
        vec![prompt_separator_line(width)],
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

pub(super) fn active_working_frame_lines_with_cursor(
    context: &TerminalShellContext,
    tick: usize,
    elapsed_secs: u64,
    input: &str,
    cursor: usize,
    live_output: &LiveProviderOutput,
    width: usize,
) -> ActiveWorkingFrameLineGroups {
    let response_lines = live_output
        .response_preview()
        .map(|text| with_leading_spacer(rendered_preview_lines(&text, drawable_width(width))))
        .unwrap_or_default();
    let reasoning_lines = if response_lines.is_empty() {
        live_output
            .reasoning_summary()
            .map(|line| {
                with_leading_spacer(non_empty_lines(wrap_words(&line, drawable_width(width))))
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
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

pub(crate) fn frame_separator_line(width: usize) -> String {
    "─".repeat(drawable_width(width))
}

fn prompt_separator_line(width: usize) -> String {
    frame_separator_line(width)
}

pub(crate) fn terminal_width() -> usize {
    terminal_size()
        .ok()
        .map(|(width, _)| usize::from(width))
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

pub(crate) fn drawable_width(width: usize) -> usize {
    width.saturating_sub(1).max(1)
}

#[cfg(test)]
mod tests;
