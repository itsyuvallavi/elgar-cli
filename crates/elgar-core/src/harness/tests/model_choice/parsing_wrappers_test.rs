//! Model-choice wrapper and provider-marker parser tests.

use crate::harness::{
    parse_model_choice, ModelChoice, StructuredRequestKind, StructuredRequestValidationError,
};

#[test]
fn parse_model_choice_accepts_structured_request_with_provider_tool_marker_suffix() {
    let choice = parse_model_choice(
        r#"{"type":"structured_request","kind":"ls","reason":"Need app listing.","arguments":{"path":"app"}}<tool_call|>"#,
    );

    match choice {
        ModelChoice::StructuredRequest(request) => {
            assert_eq!(request.kind, StructuredRequestKind::Ls);
            assert_eq!(
                request
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some("app")
            );
        }
        other => panic!("expected structured request, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_accepts_structured_batch_with_provider_tool_marker_suffix() {
    let choice = parse_model_choice(
        r#"{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"read","arguments":{"path":"app/page.tsx"}},{"kind":"read","arguments":{"path":"app/layout.tsx"}}]}<tool_call|>"#,
    );

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].kind, StructuredRequestKind::Read);
            assert_eq!(requests[1].kind, StructuredRequestKind::Read);
        }
        other => panic!("expected structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_accepts_fenced_structured_request_with_marker_inside_fence() {
    let choice = parse_model_choice(
        r#"```json
{"type":"structured_request","kind":"ls","reason":"Need app listing.","arguments":{"path":"app"}}<tool_call|>
```"#,
    );

    match choice {
        ModelChoice::StructuredRequest(request) => {
            assert_eq!(request.kind, StructuredRequestKind::Ls);
            assert_eq!(
                request
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some("app")
            );
        }
        other => panic!("expected fenced marked structured request, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_accepts_fenced_structured_request_with_marker_after_fence() {
    let choice = parse_model_choice(
        r#"```json
{"type":"structured_request","kind":"ls","reason":"Need app listing.","arguments":{"path":"app"}}
```<tool_call|>"#,
    );

    match choice {
        ModelChoice::StructuredRequest(request) => {
            assert_eq!(request.kind, StructuredRequestKind::Ls);
            assert_eq!(
                request
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some("app")
            );
        }
        other => panic!("expected marked fenced structured request, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_accepts_fenced_structured_batch_with_marker_inside_fence() {
    let choice = parse_model_choice(
        r#"```json
{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"read","arguments":{"path":"app/page.tsx"}},{"kind":"read","arguments":{"path":"app/layout.tsx"}}]}<tool_call|>
```"#,
    );

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].kind, StructuredRequestKind::Read);
            assert_eq!(requests[1].kind, StructuredRequestKind::Read);
        }
        other => panic!("expected fenced marked structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_accepts_fenced_structured_batch_with_marker_after_fence() {
    let choice = parse_model_choice(
        r#"```
{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"read","arguments":{"path":"app/page.tsx"}},{"kind":"read","arguments":{"path":"app/layout.tsx"}}]}
```<tool_call|>"#,
    );

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].kind, StructuredRequestKind::Read);
            assert_eq!(requests[1].kind, StructuredRequestKind::Read);
        }
        other => panic!("expected marked fenced structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_rejects_structured_request_with_normal_text_suffix() {
    let choice = parse_model_choice(
        r#"{"type":"structured_request","kind":"ls","reason":"Need app listing.","arguments":{"path":"app"}} and then explain"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::MalformedJson,
            ..
        }
    ));
}

#[test]
fn parse_model_choice_rejects_unbalanced_json_with_provider_marker_suffix() {
    let choice = parse_model_choice(
        r#"{"type":"structured_request","kind":"ls","reason":"Need app listing"<tool_call|>"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::MalformedJson,
            ..
        }
    ));
}
