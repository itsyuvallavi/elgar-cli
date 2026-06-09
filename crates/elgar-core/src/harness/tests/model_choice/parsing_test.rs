//! Model-choice parser and contract tests.

use crate::harness::{
    loop_decision_contract, model_choice_contract, parse_model_choice,
    parse_model_choice_with_registry, EvidenceDepth, ModelChoice, PrimitiveTool, PrimitiveToolId,
    PrimitiveToolRegistry, PrimitiveToolSideEffectLevel, StructuredRequestKind,
    StructuredRequestValidationError,
};

#[test]
fn loop_contract_encourages_only_independent_batches() {
    let contract = crate::harness::loop_decision_contract(&PrimitiveToolRegistry::stage_3a());

    assert!(contract.contains("multiple independent"));
    assert!(contract.contains("already clearly needs"));
    assert!(contract.contains("Do not batch speculative"));
}

#[test]
fn loop_contract_prefers_user_named_paths() {
    let contract = crate::harness::loop_decision_contract(&PrimitiveToolRegistry::stage_3a());

    assert!(contract.contains("When the user names a path"));
    assert!(contract.contains("For `list <dir>`, request `ls` on that directory"));
    assert!(contract.contains("For `read <dir>`"));
    assert!(contract.contains("Prefer the user-named path over `.`"));
    assert!(contract.contains("`find` pattern such as `README*`"));
}

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
fn parse_model_choice_accepts_answer_now() {
    let choice = parse_model_choice(r#"{"type":"answer_now","reason":"Evidence is enough."}"#);

    assert_eq!(
        choice,
        ModelChoice::AnswerNow {
            reason: "Evidence is enough.".to_string(),
            evidence_depth: EvidenceDepth::Enough
        }
    );
}

#[test]
fn parse_model_choice_accepts_answer_now_limited_depth() {
    let choice = parse_model_choice(
        r#"{"type":"answer_now","reason":"Limited evidence is enough for a bounded answer.","evidence_depth":"limited"}"#,
    );

    assert_eq!(
        choice,
        ModelChoice::AnswerNow {
            reason: "Limited evidence is enough for a bounded answer.".to_string(),
            evidence_depth: EvidenceDepth::Limited
        }
    );
}

#[test]
fn parse_model_choice_rejects_answer_now_insufficient_depth() {
    let choice = parse_model_choice(
        r#"{"type":"answer_now","reason":"Need more context.","evidence_depth":"insufficient"}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::InvalidEvidenceDepth(value),
            ..
        } if value == "insufficient"
    ));
}

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
fn parse_model_choice_rejects_large_structured_request_batch() {
    let choice = parse_model_choice(
        r#"{"type":"structured_requests","reason":"Too many.","requests":[{"kind":"ls"},{"kind":"ls"},{"kind":"ls"},{"kind":"ls"},{"kind":"ls"}]}"#,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::TooManyRequests(4),
            ..
        }
    ));
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

#[test]
fn parse_model_choice_rejects_unknown_kind() {
    let choice =
        parse_model_choice(r#"{"type":"structured_request","kind":"delete_repo","reason":"bad"}"#);

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::UnknownKind(_),
            ..
        }
    ));
}

#[test]
fn parse_model_choice_rejects_disabled_tool() {
    let registry = PrimitiveToolRegistry::new(vec![PrimitiveTool {
        id: PrimitiveToolId::Ls,
        display_name: "Ls",
        description: "Disabled test tool.",
        input_shape: r#"{"type":"structured_request","kind":"ls","reason":"short reason","arguments":{"path":"."}}"#,
        side_effect_level: PrimitiveToolSideEffectLevel::ReadOnly,
        enabled_in_stage: false,
        executable_in_stage: false,
        requires_permission: false,
        limits: &["disabled for test"],
    }]);

    let choice = parse_model_choice_with_registry(
        r#"{"type":"structured_request","kind":"ls","reason":"test","arguments":{"path":"."}}"#,
        &registry,
    );

    assert!(matches!(
        choice,
        ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::DisabledKind(_),
            ..
        }
    ));
}

#[test]
fn model_choice_contract_renders_enabled_stage_3a_tools() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let contract = model_choice_contract(&registry);

    assert!(contract.contains("`read`"));
    assert!(contract.contains("`ls`"));
    assert!(contract.contains("`find`"));
    assert!(contract.contains("`grep`"));
    assert!(contract.contains("`bash`"));
    assert!(contract.contains("`write`"));
    assert!(contract.contains("`edit`"));
    assert!(contract.contains("Available primitive tools"));
}

#[test]
fn loop_contract_guides_broad_requests_without_macro_tools() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let contract = loop_decision_contract(&registry);

    assert!(contract.contains("For broad requests"));
    assert!(contract.contains("gather enough verified evidence"));
    assert!(contract.contains("Do not answer from only a directory listing"));
    assert!(contract.contains("evidence_depth"));
    assert!(contract.contains("If evidence is insufficient"));
    assert!(!contract.contains("review_project"));
    assert!(!contract.contains("inspect_project"));
    assert!(!contract.contains("package.json"));
    assert!(!contract.contains("app/page.tsx"));
}

#[test]
fn stage_3a_executable_tools_are_read_only_primitives() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let executable = registry
        .tools()
        .iter()
        .filter(|tool| tool.executable_in_stage)
        .map(|tool| tool.id)
        .collect::<Vec<_>>();

    assert_eq!(
        executable,
        vec![
            PrimitiveToolId::Read,
            PrimitiveToolId::Ls,
            PrimitiveToolId::Find,
            PrimitiveToolId::Grep
        ]
    );
}
