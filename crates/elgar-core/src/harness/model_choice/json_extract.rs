//! JSON extraction helpers for model-choice parsing.
//!
//! These helpers normalize provider text into a JSON value when the model
//! intended to return structured protocol JSON.

use serde_json::Value;

pub(super) fn parse_model_choice_json_value(text: &str) -> Result<Value, serde_json::Error> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => Ok(value),
        Err(error) => {
            let Some(json_text) = first_balanced_json_object_with_allowed_trailing_junk(text)
            else {
                return Err(error);
            };
            serde_json::from_str::<Value>(json_text)
        }
    }
}

pub(super) fn first_balanced_json_object(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&text[..index + character.len_utf8()]);
                }
            }
            _ => {}
        }
    }

    None
}

fn first_balanced_json_object_with_allowed_trailing_junk(text: &str) -> Option<&str> {
    let json_text = first_balanced_json_object(text)?;
    let trailing = &text[json_text.len()..];
    if is_allowed_trailing_provider_junk(trailing) {
        return Some(json_text);
    }
    None
}

fn is_allowed_trailing_provider_junk(trailing: &str) -> bool {
    let trailing = trailing.trim();
    trailing.is_empty()
        || matches!(
            trailing,
            "<tool_call|>" | "<|tool_call|>" | "<tool_call>" | "</tool_call>"
        )
}

pub(super) fn normalize_model_choice_text(text: &str) -> String {
    let text = strip_allowed_provider_markers(text.trim()).trim();
    let text = unwrap_single_fenced_block(text).unwrap_or(text);
    strip_allowed_provider_markers(text.trim())
        .trim()
        .to_string()
}

fn unwrap_single_fenced_block(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("```")?;
    let inner = rest.strip_suffix("```")?;

    if inner.trim().is_empty() {
        return None;
    }

    if let Some(body) = inner.strip_prefix('\n') {
        return Some(body.trim());
    }

    let inner = inner.trim();
    let (first_line, body) = inner.split_once('\n')?;
    let language = first_line.trim();
    if language.is_empty() || language.eq_ignore_ascii_case("json") {
        return Some(body.trim());
    }

    None
}

fn strip_allowed_provider_markers(text: &str) -> &str {
    let mut current = text.trim();
    loop {
        let next = strip_one_allowed_provider_marker(current).trim();
        if next.len() == current.len() {
            return current;
        }
        current = next;
    }
}

fn strip_one_allowed_provider_marker(text: &str) -> &str {
    for marker in [
        "<tool_call|>",
        "<|tool_call|>",
        "<tool_call>",
        "</tool_call>",
    ] {
        if let Some(value) = text.strip_suffix(marker) {
            return value;
        }
    }
    text
}
