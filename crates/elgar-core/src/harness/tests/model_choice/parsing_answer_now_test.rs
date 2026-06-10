//! Model-choice `answer_now` parser tests.

use crate::harness::{
    parse_model_choice, EvidenceDepth, ModelChoice, StructuredRequestValidationError,
};

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
