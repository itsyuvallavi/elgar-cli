//! Parses LM Studio provider responses.
//!
//! This file turns OpenAI-compatible JSON and streaming event lines into
//! Elgar's `ProviderOutput` and stream chunks.

use serde::Deserialize;

use crate::{
    event::{ProviderMetrics, ProviderOutput},
    provider::types::{ChatResponse, ProviderError, ProviderErrorResponse, ProviderStreamChunk},
    token_accounting::ProviderTokenUsage,
};

pub fn parse_chat_response_json(payload: &str) -> Result<ProviderOutput, ProviderError> {
    parse_chat_response_json_with_metrics(payload, None)
}

/// Parses a non-streaming OpenAI-compatible response and attaches metrics.
pub fn parse_chat_response_json_with_metrics(
    payload: &str,
    metrics: Option<ProviderMetrics>,
) -> Result<ProviderOutput, ProviderError> {
    let response: ChatResponse = serde_json::from_str(payload)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;

    let message = response
        .choices
        .iter()
        .filter_map(|choice| choice.message.as_ref())
        .find(|message| !message.content.trim().is_empty() || !message.tool_calls.is_empty())
        .ok_or_else(|| {
            ProviderError::empty_response("provider response contained no text or tool calls")
        })?;

    let mut output = ProviderOutput::new(message.content.trim().to_string())
        .with_tool_calls(message.tool_calls.clone());

    if let Some(thinking) = message.explicit_thinking() {
        output = output.with_thinking(thinking);
    }
    if let Some(mut metrics) = metrics {
        metrics.usage = response.usage.map(provider_usage_from_chat_usage);
        output = output.with_metrics(metrics);
    }

    Ok(output)
}

/// Parses a complete text/event-stream payload after it has been collected.
pub fn parse_chat_stream_response(payload: &str) -> Result<ProviderOutput, ProviderError> {
    let mut text = String::new();
    let mut thinking = String::new();

    for chunk in parse_chat_stream_chunks(payload)? {
        match chunk {
            ProviderStreamChunk::Reasoning(value) => thinking.push_str(&value),
            ProviderStreamChunk::Text(value) => text.push_str(&value),
        }
    }

    provider_output_from_stream_parts(text, thinking)
}

pub fn parse_chat_stream_chunks(payload: &str) -> Result<Vec<ProviderStreamChunk>, ProviderError> {
    let mut chunks = Vec::new();

    for line in payload.lines().map(str::trim) {
        chunks.extend(parse_chat_stream_line(line)?);
    }

    Ok(chunks)
}

/// Parses one SSE `data:` line into zero or more provider chunks.
pub fn parse_chat_stream_line(line: &str) -> Result<Vec<ProviderStreamChunk>, ProviderError> {
    if line.is_empty() || line.starts_with(':') {
        return Ok(Vec::new());
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(Vec::new());
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(Vec::new());
    }

    let response: ChatStreamResponse = serde_json::from_str(data)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    let mut chunks = Vec::new();
    for choice in response.choices {
        if let Some(delta) = choice.delta {
            if let Some(reasoning) = non_empty(delta.reasoning) {
                chunks.push(ProviderStreamChunk::Reasoning(reasoning));
            }
            if let Some(thinking) = non_empty(delta.thinking) {
                chunks.push(ProviderStreamChunk::Reasoning(thinking));
            }
            if let Some(content) = non_empty(delta.content) {
                chunks.push(ProviderStreamChunk::Text(content));
            }
        }
    }

    Ok(chunks)
}

/// Turns accumulated streaming text/reasoning into final provider output.
pub(crate) fn provider_output_from_stream_parts(
    text: String,
    thinking: String,
) -> Result<ProviderOutput, ProviderError> {
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(ProviderError::empty_response(
            "provider stream contained no text",
        ));
    }

    let output = ProviderOutput::new(trimmed_text.to_string());
    let trimmed_thinking = thinking.trim();
    Ok(if trimmed_thinking.is_empty() {
        output
    } else {
        output.with_thinking(trimmed_thinking.to_string())
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

/// Parses a provider error payload while preserving HTTP status.
pub fn parse_provider_error_json(status_code: Option<u16>, payload: &str) -> ProviderError {
    match serde_json::from_str::<ProviderErrorResponse>(payload) {
        Ok(response) => {
            ProviderError::provider(response.error.message, status_code, response.error.code)
        }
        Err(error) => ProviderError::response_parse(error.to_string()).with_status(status_code),
    }
}

fn provider_usage_from_chat_usage(usage: crate::provider::types::ChatUsage) -> ProviderTokenUsage {
    ProviderTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamResponse {
    #[serde(default)]
    choices: Vec<ChatStreamChoice>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamChoice {
    delta: Option<ChatStreamDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default, alias = "thinking_content")]
    thinking: Option<String>,
}
