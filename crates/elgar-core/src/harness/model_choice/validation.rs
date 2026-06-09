//! Protocol validation for model-choice JSON.
//!
//! This module turns parsed JSON into validated `ModelChoice` values using the
//! active primitive tool registry.

use serde_json::Value;

use crate::harness::{PrimitiveToolId, PrimitiveToolRegistry};

use super::types::{
    EvidenceDepth, ModelChoice, StructuredRequestValidationError, ValidatedStructuredRequest,
};

const MAX_STRUCTURED_REQUEST_BATCH: usize = 4;

pub(super) fn validate_model_choice_json(
    value: &Value,
    registry: &PrimitiveToolRegistry,
) -> Result<ModelChoice, StructuredRequestValidationError> {
    let choice_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(StructuredRequestValidationError::MissingType)?;

    match choice_type {
        "message" => {
            let content = value
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            Ok(ModelChoice::Message { content })
        }
        "answer_now" => {
            let reason = value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or(StructuredRequestValidationError::MissingReason)?
                .to_string();
            let evidence_depth = parse_evidence_depth(value)?;
            Ok(ModelChoice::AnswerNow {
                reason,
                evidence_depth,
            })
        }
        "structured_request" => {
            let reason = value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or(StructuredRequestValidationError::MissingReason)?
                .to_string();

            Ok(ModelChoice::StructuredRequest(validate_structured_request(
                value, registry, &reason,
            )?))
        }
        "structured_requests" => {
            let reason = value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or(StructuredRequestValidationError::MissingReason)?
                .to_string();
            let requests = value
                .get("requests")
                .and_then(Value::as_array)
                .ok_or(StructuredRequestValidationError::MissingRequests)?;
            if requests.is_empty() {
                return Err(StructuredRequestValidationError::EmptyRequests);
            }
            if requests.len() > MAX_STRUCTURED_REQUEST_BATCH {
                return Err(StructuredRequestValidationError::TooManyRequests(
                    MAX_STRUCTURED_REQUEST_BATCH,
                ));
            }

            let mut validated = Vec::with_capacity(requests.len());
            for request in requests {
                validated.push(validate_structured_request(request, registry, &reason)?);
            }

            Ok(ModelChoice::StructuredRequests(validated))
        }
        other => Err(StructuredRequestValidationError::UnknownType(
            other.to_string(),
        )),
    }
}

fn parse_evidence_depth(value: &Value) -> Result<EvidenceDepth, StructuredRequestValidationError> {
    let Some(depth_text) = value.get("evidence_depth").and_then(Value::as_str) else {
        return Ok(EvidenceDepth::Enough);
    };
    EvidenceDepth::parse(depth_text.trim()).ok_or_else(|| {
        StructuredRequestValidationError::InvalidEvidenceDepth(depth_text.to_string())
    })
}

fn validate_structured_request(
    value: &Value,
    registry: &PrimitiveToolRegistry,
    fallback_reason: &str,
) -> Result<ValidatedStructuredRequest, StructuredRequestValidationError> {
    let kind_text = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(StructuredRequestValidationError::MissingKind)?;
    let kind = PrimitiveToolId::parse(kind_text)
        .ok_or_else(|| StructuredRequestValidationError::UnknownKind(kind_text.into()))?;
    if !registry.enabled(kind) {
        return Err(StructuredRequestValidationError::DisabledKind(
            kind.as_str().to_string(),
        ));
    }

    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(fallback_reason)
        .to_string();

    Ok(ValidatedStructuredRequest {
        kind,
        reason,
        arguments: value.get("arguments").cloned(),
    })
}
