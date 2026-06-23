//! Width-aware text wrapping for code block display.

use super::CODE_WRAP_CONTINUATION_PREFIX;

pub(super) fn split_to_width(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if line.is_empty() {
        return vec![String::new()];
    }

    if line.chars().count() <= width {
        return vec![line.to_string()];
    }

    let continuation_width = width
        .saturating_sub(CODE_WRAP_CONTINUATION_PREFIX.chars().count())
        .max(1);
    let mut segments = Vec::new();
    let mut rest = line;
    let mut is_continuation = false;

    while !rest.is_empty() {
        let available_width = if is_continuation {
            continuation_width
        } else {
            width
        };
        let (chunk, next) = split_code_display_chunk(rest, available_width);
        let chunk = chunk.trim_end();
        if is_continuation {
            segments.push(format!("{CODE_WRAP_CONTINUATION_PREFIX}{chunk}"));
        } else {
            segments.push(chunk.to_string());
        }
        rest = next.trim_start_matches(char::is_whitespace);
        is_continuation = true;
    }

    segments
}

fn split_code_display_chunk(line: &str, width: usize) -> (&str, &str) {
    if line.chars().count() <= width {
        return (line, "");
    }

    let byte_limit = byte_index_after_chars(line, width);
    if let Some((break_index, _character)) = line[..byte_limit]
        .char_indices()
        .rev()
        .find(|(index, character)| *index > 0 && character.is_whitespace())
    {
        return (&line[..break_index], &line[break_index..]);
    }

    (&line[..byte_limit], &line[byte_limit..])
}

fn byte_index_after_chars(text: &str, max_chars: usize) -> usize {
    text.char_indices()
        .nth(max_chars)
        .map(|(index, _character)| index)
        .unwrap_or(text.len())
}

pub(super) fn truncate_to_width(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
