//! Provider reasoning preview rendering.
//!
//! Normal chat shows a compact, readable reasoning note. The full raw provider
//! reasoning remains available through `/details last` and JSONL logs.

/// Render compact provider reasoning for normal chat.
pub(crate) fn render_provider_reasoning_compact(reasoning: Option<&str>) -> Option<String> {
    let reasoning = reasoning?.trim();
    if reasoning.is_empty() {
        return None;
    }

    compact_reasoning_text(reasoning)
}

/// Render compact live reasoning while the provider is still streaming.
pub(crate) fn render_live_reasoning_compact(reasoning: &str) -> Option<String> {
    render_provider_reasoning_compact(Some(reasoning))
}

/// Render full provider reasoning for `/details last` and raw-copy paths.
pub(crate) fn render_provider_reasoning_details(reasoning: &str) -> String {
    let mut details = String::from("Provider reasoning details\nRaw reasoning:\n");
    details.push_str(reasoning.trim_end());
    details
}

fn compact_reasoning_text(reasoning: &str) -> Option<String> {
    let first_paragraph = reasoning
        .split("\n\n")
        .map(normalize_reasoning_paragraph)
        .find(|paragraph| !paragraph.is_empty())?;

    let compact = first_sentences(&first_paragraph, 2);
    (!compact.is_empty()).then_some(compact)
}

fn normalize_reasoning_paragraph(paragraph: &str) -> String {
    paragraph
        .lines()
        .map(str::trim)
        .take_while(|line| !is_structured_detail_line(line))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_structured_detail_line(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line.starts_with("1. ")
        || line.ends_with(':')
}

fn first_sentences(text: &str, max_sentences: usize) -> String {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            let sentence = current.trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_string());
            }
            current.clear();
            if sentences.len() >= max_sentences {
                break;
            }
        }
    }

    if sentences.is_empty() {
        text.trim().to_string()
    } else {
        sentences.join(" ")
    }
}
