//! Model-choice single structured-request parser tests.

use crate::harness::{parse_model_choice, ModelChoice, StructuredRequestKind};

#[test]
fn parse_model_choice_validates_structured_request() {
    let choice = parse_model_choice(
        r#"{"type":"structured_request","kind":"ls","reason":"Need directory listing.","arguments":{"path":"."}}"#,
    );

    match choice {
        ModelChoice::StructuredRequest(request) => {
            assert_eq!(request.kind, StructuredRequestKind::Ls);
            assert_eq!(request.reason, "Need directory listing.");
            assert_eq!(
                request
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some(".")
            );
        }
        other => panic!("expected structured request, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_validates_fenced_structured_request() {
    let choice = parse_model_choice(
        r#"```json
{"type":"structured_request","kind":"ls","reason":"Need directory listing.","arguments":{"path":"app"}}
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
        other => panic!("expected fenced structured request, got {other:?}"),
    }
}

#[test]
fn parse_model_choice_captures_read_arguments() {
    let choice = parse_model_choice(
        r#"{"type":"structured_request","kind":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
    );

    match choice {
        ModelChoice::StructuredRequest(request) => {
            assert_eq!(request.kind, StructuredRequestKind::Read);
            assert_eq!(
                request
                    .arguments
                    .as_ref()
                    .and_then(|value| value.get("path"))
                    .and_then(serde_json::Value::as_str),
                Some("package.json")
            );
        }
        other => panic!("expected structured request, got {other:?}"),
    }
}
