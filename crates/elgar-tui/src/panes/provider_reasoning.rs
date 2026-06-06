//! Provider reasoning preview rendering.
//!
//! This formats model reasoning/thinking text when the provider returns it.

use elgar_core::provider_visible_text_from_text_only_output;

use crate::markdown::render_assistant_markdown;

/// Render provider reasoning if there is useful reasoning text to show.
pub(super) fn render_provider_reasoning(reasoning: Option<&str>) -> Option<String> {
    let reasoning = reasoning?.trim();
    if reasoning.is_empty() {
        return None;
    }
    let reasoning = provider_reasoning_visible_text(reasoning)?;

    ReasoningBlock::collapsed(&reasoning).map(|block| block.render_collapsed())
}

fn provider_reasoning_visible_text(reasoning: &str) -> Option<String> {
    let visible = provider_visible_text_from_text_only_output(reasoning.to_string())?;
    let mut visible_lines = Vec::new();

    for line in visible.lines() {
        let visible_sentences = split_provider_reasoning_sentences(line)
            .into_iter()
            .filter(|sentence| !is_low_value_provider_tool_planning_reasoning(sentence))
            .map(str::trim)
            .filter(|sentence| !sentence.is_empty())
            .collect::<Vec<_>>();

        if visible_sentences.is_empty() {
            continue;
        }

        visible_lines.push(visible_sentences.join(" "));
    }

    let text = visible_lines.join("\n").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn split_provider_reasoning_sentences(line: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let bytes = line.as_bytes();

    for (index, character) in line.char_indices() {
        if !matches!(character, '.' | '?' | '!') {
            continue;
        }

        let next = index + character.len_utf8();
        if next == line.len() || bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
            sentences.push(&line[start..next]);
            start = next;
        }
    }

    if start < line.len() {
        sentences.push(&line[start..]);
    }

    sentences
}

pub(super) fn is_low_value_provider_tool_planning_reasoning(sentence: &str) -> bool {
    let normalized = normalized_provider_reasoning_sentence(sentence);

    normalized == "path"
        || normalized.starts_with("desktop relative")
        || normalized.starts_with("desktoprelative")
        || normalized.starts_with("project relative")
        || normalized.starts_with("projectrelative")
        || normalized.starts_with("create directory")
        || normalized.starts_with("create file")
        || normalized.starts_with("create files")
        || normalized.starts_with("use create_directory")
        || normalized.starts_with("use create_file")
        || normalized.starts_with("use shellcommand")
        || normalized.starts_with("use shell command")
        || is_generic_provider_tool_planning_reasoning(&normalized)
        || normalized.starts_with("provide tool call")
        || normalized.contains("initialise project")
        || normalized.contains("initialize project")
}

fn is_generic_provider_tool_planning_reasoning(normalized: &str) -> bool {
    if normalized.contains("tool call") {
        return true;
    }

    let Some(rest) = normalized.strip_prefix("use ") else {
        return false;
    };

    let words = rest.split_whitespace().collect::<Vec<_>>();
    if words.len() > 6 {
        return false;
    }

    words.contains(&"tool")
}

fn normalized_provider_reasoning_sentence(sentence: &str) -> String {
    sentence
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '.' | ':' | ';' | '?' | '!' | '-' | '`' | '"' | '\''))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReasoningBlock {
    summary: String,
    detail: String,
    expanded: bool,
}

impl ReasoningBlock {
    fn collapsed(detail: &str) -> Option<Self> {
        Some(Self {
            summary: compact_reasoning_summary(detail)?,
            detail: render_assistant_markdown(detail),
            expanded: false,
        })
    }

    fn render_collapsed(&self) -> String {
        let _future_expanded_detail = if self.expanded {
            Some(self.detail.as_str())
        } else {
            None
        };
        self.summary.clone()
    }
}

fn compact_reasoning_summary(reasoning: &str) -> Option<String> {
    let rendered = render_assistant_markdown(reasoning);
    let summary = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 500;
    format_provider_reasoning_summary(&summary, MAX_CHARS)
}

pub(crate) fn format_provider_reasoning_summary(text: &str, max_chars: usize) -> Option<String> {
    let text = clean_reasoning_preamble(text.trim());
    if text.is_empty() {
        return None;
    }

    Some(truncate_chars(&text, max_chars))
}

fn clean_reasoning_preamble(text: &str) -> String {
    let text = text
        .strip_prefix("Here's a thinking process:")
        .or_else(|| text.strip_prefix("Here’s a thinking process:"))
        .unwrap_or(text)
        .trim();
    let text = text
        .strip_prefix("1. Analyze User Input:")
        .unwrap_or(text)
        .trim();

    text.to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}
