//! Redacts local log metadata according to the local detail mode.
//!
//! Safe mode is the default. Full mode is opt-in for local debugging only.

use serde_json::Value;

const DETAIL_ENV: &str = "ELGAR_LOG_DETAIL";

pub(super) fn redact_metadata(metadata: Value) -> Value {
    if detail_mode_is_full() {
        return metadata;
    }

    redact_value(metadata)
}

fn detail_mode_is_full() -> bool {
    std::env::var(DETAIL_ENV)
        .ok()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("full"))
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if key_is_sensitive(&key) {
                        (key, Value::String("[redacted]".to_string()))
                    } else {
                        (key, redact_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        value => value,
    }
}

fn key_is_sensitive(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "prompt"
            | "input"
            | "text"
            | "output"
            | "content"
            | "thinking"
            | "reasoning"
            | "stdout"
            | "stderr"
            | "file_contents"
            | "env"
            | "token"
            | "api_key"
            | "secret"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::redact_value;

    #[test]
    fn redacts_sensitive_keys_recursively() {
        let value = json!({
            "request_mode": "harness_tool_decision",
            "prompt": "hello",
            "nested": {
                "thinking": "private"
            }
        });

        let redacted = redact_value(value);

        assert_eq!(redacted["request_mode"], "harness_tool_decision");
        assert_eq!(redacted["prompt"], "[redacted]");
        assert_eq!(redacted["nested"]["thinking"], "[redacted]");
    }
}
