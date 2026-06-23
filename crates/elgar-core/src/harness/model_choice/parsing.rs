//! Entry points for model-choice parsing.
//!
//! This module keeps the top-level parse flow small. JSON extraction, prose
//! safety checks, and protocol validation live in sibling modules.

use crate::harness::PrimitiveToolRegistry;

use super::{
    json_extract::{normalize_model_choice_text, parse_model_choice_json_value},
    prose_guard::contains_embedded_structured_request,
    types::{ModelChoice, StructuredRequestValidationError},
    validation::validate_model_choice_json,
};

/// Parse model text using the default primitive tool registry.
pub fn parse_model_choice(text: &str) -> ModelChoice {
    parse_model_choice_with_registry(text, &PrimitiveToolRegistry::stage_3a())
}

/// Parse model text using a caller-provided primitive tool registry.
pub fn parse_model_choice_with_registry(
    text: &str,
    registry: &PrimitiveToolRegistry,
) -> ModelChoice {
    let normalized = normalize_model_choice_text(text);
    if !normalized.starts_with('{') {
        if contains_embedded_structured_request(&normalized) {
            return ModelChoice::InvalidStructuredRequest {
                error: StructuredRequestValidationError::MixedMessageAndStructuredRequest,
                raw: normalized,
            };
        }
        return ModelChoice::Message {
            content: normalized,
        };
    }

    let value = match parse_model_choice_json_value(&normalized) {
        Ok(value) => value,
        Err(_) => {
            return ModelChoice::InvalidStructuredRequest {
                error: StructuredRequestValidationError::MalformedJson,
                raw: normalized,
            };
        }
    };

    match validate_model_choice_json(&value, registry) {
        Ok(choice) => choice,
        Err(error) => ModelChoice::InvalidStructuredRequest {
            error,
            raw: normalized,
        },
    }
}
