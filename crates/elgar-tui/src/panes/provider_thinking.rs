use elgar_core::provider_visible_text_from_text_only_output;

use crate::{markdown::render_assistant_markdown, reasoning::format_provider_reasoning_summary};

pub(super) fn render_provider_thinking(thinking: Option<&str>) -> Option<String> {
    let thinking = thinking?.trim();
    if thinking.is_empty() {
        return None;
    }
    let thinking = provider_thinking_visible_text(thinking)?;

    ThinkingBlock::collapsed(&thinking).map(|block| block.render_collapsed())
}

fn provider_thinking_visible_text(thinking: &str) -> Option<String> {
    let visible = provider_visible_text_from_text_only_output(thinking.to_string())?;
    let mut visible_lines = Vec::new();

    for line in visible.lines() {
        let visible_sentences = split_provider_thinking_sentences(line)
            .into_iter()
            .filter(|sentence| !is_low_value_provider_tool_planning_thinking(sentence))
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

fn split_provider_thinking_sentences(line: &str) -> Vec<&str> {
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

pub(super) fn is_low_value_provider_tool_planning_thinking(sentence: &str) -> bool {
    let normalized = normalized_provider_thinking_sentence(sentence);

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
        || is_generic_provider_tool_planning_thinking(&normalized)
        || normalized.starts_with("provide tool call")
        || normalized.contains("initialise project")
        || normalized.contains("initialize project")
}

fn is_generic_provider_tool_planning_thinking(normalized: &str) -> bool {
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

fn normalized_provider_thinking_sentence(sentence: &str) -> String {
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
struct ThinkingBlock {
    summary: String,
    detail: String,
    expanded: bool,
}

impl ThinkingBlock {
    fn collapsed(detail: &str) -> Option<Self> {
        Some(Self {
            summary: compact_thinking_summary(detail)?,
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

fn compact_thinking_summary(thinking: &str) -> Option<String> {
    let rendered = render_assistant_markdown(thinking);
    let summary = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 96;
    format_provider_reasoning_summary(&summary, MAX_CHARS)
}
