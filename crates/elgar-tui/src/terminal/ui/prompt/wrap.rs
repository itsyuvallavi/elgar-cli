//! Text wrapping helpers for inline terminal prompt rendering.

pub(crate) fn non_empty_lines(lines: Vec<String>) -> Vec<String> {
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

pub(super) fn rendered_preview_lines(text: &str, width: usize) -> Vec<String> {
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

pub(super) fn wrap_preserving_spacing(line: &str, width: usize) -> Vec<String> {
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

pub(crate) fn wrap_words(text: &str, width: usize) -> Vec<String> {
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
