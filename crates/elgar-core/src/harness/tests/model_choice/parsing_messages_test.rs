//! Model-choice plain-message and mixed-prose parser tests.

use crate::harness::{parse_model_choice, ModelChoice, StructuredRequestValidationError};

#[test]
fn parse_model_choice_accepts_plain_message() {
    let choice = parse_model_choice("Hello there.");

    assert_eq!(
        choice,
        ModelChoice::Message {
            content: "Hello there.".to_string()
        }
    );
}

#[test]
fn parse_model_choice_rejects_prose_with_embedded_structured_request() {
    let choice = parse_model_choice(
        r#"I need to inspect this next.
{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::MixedMessageAndStructuredRequest,
            ..
        }
    ));
}

#[test]
fn parse_model_choice_rejects_prose_with_old_tool_shape() {
    let choice = parse_model_choice(
        r#"I will read the file next.
{"type":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::MixedMessageAndStructuredRequest,
            ..
        }
    ));
}

#[test]
fn parse_model_choice_accepts_prose_with_unrelated_json_example() {
    let choice = parse_model_choice(r#"Here is JSON: {"name":"demo","version":"1.0.0"}"#);

    assert_eq!(
        choice,
        ModelChoice::Message {
            content: r#"Here is JSON: {"name":"demo","version":"1.0.0"}"#.to_string()
        }
    );
}

#[test]
fn parse_model_choice_keeps_prose_with_fenced_json_as_message() {
    let text = r#"Here is an example:

```json
{"type":"structured_request","kind":"ls","reason":"Example only."}
```"#;
    let choice = parse_model_choice(text);

    assert_eq!(
        choice,
        ModelChoice::Message {
            content: text.to_string()
        }
    );
}
