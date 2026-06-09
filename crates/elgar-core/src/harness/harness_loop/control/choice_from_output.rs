//! Convert provider output into a primitive harness model choice.
//!
//! Provider tool calls and JSON fallback text both become the same
//! `ModelChoice` type before the coordinator handles them.

use crate::{
    event::ProviderOutput,
    harness::{
        parse_model_choice_with_registry, ModelChoice, PrimitiveToolId, PrimitiveToolRegistry,
        StructuredRequestValidationError, ValidatedStructuredRequest,
    },
    provider::ChatToolCall,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeToolRequest {
    pub tool_call_id: String,
    pub request: ValidatedStructuredRequest,
}

/// Convert one provider output into the normalized harness decision type.
pub(super) fn model_choice_from_provider_output(
    output: &ProviderOutput,
    registry: &PrimitiveToolRegistry,
) -> ModelChoice {
    if output.tool_calls.is_empty() {
        return parse_model_choice_with_registry(&output.text, registry);
    }

    if output.tool_calls.len() > 4 {
        return ModelChoice::InvalidStructuredRequest {
            error: StructuredRequestValidationError::TooManyRequests(4),
            raw: format!("{} provider tool calls", output.tool_calls.len()),
        };
    }

    let mut requests = Vec::with_capacity(output.tool_calls.len());
    for tool_call in &output.tool_calls {
        match validated_request_from_tool_call(tool_call, registry) {
            Ok(request) => requests.push(request),
            Err(error) => {
                return ModelChoice::InvalidStructuredRequest {
                    error,
                    raw: tool_call.function.name.clone(),
                };
            }
        }
    }

    match requests.as_slice() {
        [request] => ModelChoice::StructuredRequest(request.clone()),
        _ => ModelChoice::StructuredRequests(requests),
    }
}

/// Extract native provider tool calls while preserving provider call ids.
pub(super) fn native_tool_requests_from_provider_output(
    output: &ProviderOutput,
    registry: &PrimitiveToolRegistry,
) -> Result<Vec<NativeToolRequest>, StructuredRequestValidationError> {
    if output.tool_calls.len() > 4 {
        return Err(StructuredRequestValidationError::TooManyRequests(4));
    }

    let mut requests = Vec::with_capacity(output.tool_calls.len());
    for tool_call in &output.tool_calls {
        requests.push(NativeToolRequest {
            tool_call_id: tool_call.id.clone(),
            request: validated_request_from_tool_call(tool_call, registry)?,
        });
    }

    Ok(requests)
}

fn validated_request_from_tool_call(
    tool_call: &ChatToolCall,
    registry: &PrimitiveToolRegistry,
) -> Result<ValidatedStructuredRequest, StructuredRequestValidationError> {
    let kind = PrimitiveToolId::parse(&tool_call.function.name).ok_or_else(|| {
        StructuredRequestValidationError::UnknownKind(tool_call.function.name.clone())
    })?;
    if !registry.enabled(kind) {
        return Err(StructuredRequestValidationError::DisabledKind(
            kind.as_str().to_string(),
        ));
    }

    let arguments = if tool_call.function.arguments.trim().is_empty() {
        None
    } else {
        Some(
            serde_json::from_str(&tool_call.function.arguments)
                .map_err(|_| StructuredRequestValidationError::MalformedJson)?,
        )
    };

    Ok(ValidatedStructuredRequest {
        kind,
        reason: format!("provider tool call: {}", kind.as_str()),
        arguments,
    })
}
