//! Model-choice structured-request batch parser tests.

use crate::harness::{
    parse_model_choice, ModelChoice, StructuredRequestKind, StructuredRequestValidationError,
    MAX_TOOL_CALL_BATCH,
};

#[test]
fn parse_model_choice_validates_structured_request_batch() {
    let choice = parse_model_choice(
        r#"{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"ls","arguments":{"path":"app"}},{"kind":"read","arguments":{"path":"app/page.tsx"}}]}"#,
    );

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].kind, StructuredRequestKind::Ls);
            assert_eq!(requests[0].reason, "Need app files.");
            assert_eq!(requests[1].kind, StructuredRequestKind::Read);
            assert_eq!(
                requests[1]
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some("app/page.tsx")
            );
        }
        other => panic!("expected structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_validates_fenced_structured_request_batch() {
    let choice = parse_model_choice(
        r#"```
{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"read","arguments":{"path":"app/page.tsx"}},{"kind":"read","arguments":{"path":"app/layout.tsx"}}]}
```"#,
    );

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].kind, StructuredRequestKind::Read);
            assert_eq!(requests[1].kind, StructuredRequestKind::Read);
        }
        other => panic!("expected fenced structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_rejects_empty_structured_request_batch() {
    let choice = parse_model_choice(
        r#"{"type":"structured_requests","reason":"Need files.","requests":[]}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::EmptyRequests,
            ..
        }
    ));
}

#[test]
fn parse_model_choice_rejects_unknown_kind_in_batch() {
    let choice = parse_model_choice(
        r#"{"type":"structured_requests","reason":"Need files.","requests":[{"kind":"delete_repo"}]}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::UnknownKind(_),
            ..
        }
    ));
}

#[test]
fn parse_model_choice_accepts_max_structured_request_batch() {
    let requests = (0..MAX_TOOL_CALL_BATCH)
        .map(|_| r#"{"kind":"ls"}"#)
        .collect::<Vec<_>>()
        .join(",");
    let choice = parse_model_choice(&format!(
        r#"{{"type":"structured_requests","reason":"Max batch.","requests":[{requests}]}}"#
    ));

    match choice {
        ModelChoice::StructuredRequests(requests) => {
            assert_eq!(requests.len(), MAX_TOOL_CALL_BATCH);
        }
        other => panic!("expected max structured request batch, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_rejects_over_limit_structured_request_batch() {
    let requests = (0..=MAX_TOOL_CALL_BATCH)
        .map(|_| r#"{"kind":"ls"}"#)
        .collect::<Vec<_>>()
        .join(",");
    let choice = parse_model_choice(&format!(
        r#"{{"type":"structured_requests","reason":"Too many.","requests":[{requests}]}}"#
    ));

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::TooManyRequests(MAX_TOOL_CALL_BATCH),
            ..
        }
    ));
}
