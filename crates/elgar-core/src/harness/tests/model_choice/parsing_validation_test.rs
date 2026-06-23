//! Model-choice registry and validation parser tests.

use crate::harness::{
    parse_model_choice, parse_model_choice_with_registry, ModelChoice, PrimitiveTool,
    PrimitiveToolId, PrimitiveToolRegistry, PrimitiveToolSideEffectLevel,
    StructuredRequestValidationError,
};

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
