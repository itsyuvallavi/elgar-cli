//! Prose safety checks for model-choice parsing.
//!
//! Plain text is allowed before evidence, but prose that embeds tool-shaped
//! protocol JSON is treated as an invalid mixed response.

use serde_json::Value;

use super::json_extract::first_balanced_json_object;

pub(super) fn contains_embedded_structured_request(text: &str) -> bool {
    let text = strip_fenced_code_blocks(text);
    for (index, _) in text.match_indices('{') {
        let candidate = &text[index..];
        let Some(json_text) = first_balanced_json_object(candidate) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(json_text) else {
            continue;
        };
        if value_looks_like_structured_request(&value) {
            return true;
        }
    }

    false
}

fn strip_fenced_code_blocks(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("```") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 3..];
        let Some(end) = after_start.find("```") else {
            result.push_str(&rest[start..]);
            return result;
        };
        rest = &after_start[end + 3..];
    }

    result.push_str(rest);
    result
}

fn value_looks_like_structured_request(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    let type_value = object.get("type").and_then(Value::as_str);
    if matches!(
        type_value,
        Some("structured_request")
            | Some("structured_requests")
            | Some("answer_now")
            | Some("read")
            | Some("ls")
            | Some("find")
            | Some("grep")
            | Some("bash")
            | Some("write")
            | Some("edit")
    ) {
        return true;
    }

    let kind_value = object.get("kind").and_then(Value::as_str);
    matches!(
        kind_value,
        Some("read")
            | Some("ls")
            | Some("find")
            | Some("grep")
            | Some("bash")
            | Some("write")
            | Some("edit")
    )
}
