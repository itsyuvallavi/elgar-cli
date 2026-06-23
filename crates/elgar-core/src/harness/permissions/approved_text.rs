//! Argument text helpers for approved primitives.

use serde_json::Value;

pub(super) fn argument_text<'a>(arguments: &'a Option<Value>, key: &str) -> Option<&'a str> {
    arguments
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn argument_raw_text<'a>(arguments: &'a Option<Value>, key: &str) -> Option<&'a str> {
    arguments
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
}
