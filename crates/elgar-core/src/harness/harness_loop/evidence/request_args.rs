//! Structured request argument readers for harness evidence execution.

use serde_json::Value;

use crate::harness::ValidatedStructuredRequest;

pub(in crate::harness::harness_loop) fn request_path(
    request: &ValidatedStructuredRequest,
) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("path"))
        .and_then(Value::as_str)
}

pub(in crate::harness::harness_loop) fn request_pattern(
    request: &ValidatedStructuredRequest,
) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("pattern"))
        .and_then(Value::as_str)
}

pub(in crate::harness::harness_loop) fn request_query(
    request: &ValidatedStructuredRequest,
) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("query"))
        .and_then(Value::as_str)
}
