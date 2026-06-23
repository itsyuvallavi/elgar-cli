//! Parses LM Studio provider responses.
//!
//! This file turns OpenAI-compatible JSON and streaming event lines into
//! Elgar's `ProviderOutput` and stream chunks.

use serde::Deserialize;

use crate::{
    event::{ProviderMetrics, ProviderOutput},
    provider::types::{
        ChatResponse, ChatToolCall, ChatToolCallDelta, ChatToolCallFunction, ProviderError,
        ProviderErrorResponse, ProviderStreamChunk,
    },
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
    let mut tool_calls = Vec::<StreamedToolCallBuilder>::new();

    for chunk in parse_chat_stream_chunks(payload)? {
        match chunk {
            ProviderStreamChunk::Reasoning(value) => thinking.push_str(&value),
            ProviderStreamChunk::Text(value) => text.push_str(&value),
            ProviderStreamChunk::ToolCallDelta(delta) => {
                while tool_calls.len() <= delta.index {
                    tool_calls.push(StreamedToolCallBuilder::default());
                }
                if let Some(call) = tool_calls.get_mut(delta.index) {
                    call.apply(delta);
                }
            }
        }
    }

    let tool_calls = tool_calls
        .into_iter()
        .filter_map(StreamedToolCallBuilder::into_tool_call)
        .collect::<Vec<_>>();

    provider_output_from_stream_parts_with_tools(text, thinking, tool_calls)
}

pub fn parse_chat_stream_chunks(payload: &str) -> Result<Vec<ProviderStreamChunk>, ProviderError> {
    let mut chunks = Vec::new();

    for line in payload.lines().map(str::trim) {
        chunks.extend(parse_chat_stream_line(line)?);
    }

    Ok(chunks)
}

/// Returns true when one SSE line is the OpenAI-compatible completion sentinel.
pub fn is_chat_stream_done_line(line: &str) -> bool {
    let Some(data) = line.strip_prefix("data:") else {
        return false;
    };

    data.trim() == "[DONE]"
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
    if is_chat_stream_done_line(line) {
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
            for tool_call in delta.tool_calls {
                let function = tool_call.function;
                chunks.push(ProviderStreamChunk::ToolCallDelta(ChatToolCallDelta {
                    index: tool_call.index,
                    id: non_empty(tool_call.id),
                    tool_type: non_empty(tool_call.tool_type),
                    function_name: function
                        .as_ref()
                        .and_then(|function| non_empty(function.name.clone())),
                    function_arguments: function.and_then(|function| function.arguments),
                }));
            }
        }
    }

    Ok(chunks)
}

/// Parses provider-reported token usage from one streaming SSE line, when present.
pub fn parse_chat_stream_usage_line(
    line: &str,
) -> Result<Option<ProviderTokenUsage>, ProviderError> {
    if line.is_empty() || line.starts_with(':') || is_chat_stream_done_line(line) {
        return Ok(None);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let response: ChatStreamResponse = serde_json::from_str(data.trim())
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;

    Ok(response.usage.map(provider_usage_from_chat_usage))
}

fn provider_output_from_stream_parts_with_tools(
    text: String,
    thinking: String,
    tool_calls: Vec<ChatToolCall>,
) -> Result<ProviderOutput, ProviderError> {
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::empty_response(
            "provider stream contained no text or tool calls",
        ));
    }

    let output = ProviderOutput::new(trimmed_text.to_string()).with_tool_calls(tool_calls);
    let trimmed_thinking = thinking.trim();
    Ok(if trimmed_thinking.is_empty() {
        output
    } else {
        output.with_thinking(trimmed_thinking.to_string())
    })
}

#[derive(Debug, Default)]
struct StreamedToolCallBuilder {
    id: Option<String>,
    tool_type: Option<String>,
    function_name: String,
    function_arguments: String,
}

impl StreamedToolCallBuilder {
    fn apply(&mut self, delta: ChatToolCallDelta) {
        if let Some(id) = delta.id {
            self.id = Some(id);
        }
        if let Some(tool_type) = delta.tool_type {
            self.tool_type = Some(tool_type);
        }
        if let Some(name) = delta.function_name {
            self.function_name.push_str(&name);
        }
        if let Some(arguments) = delta.function_arguments {
            self.function_arguments.push_str(&arguments);
        }
    }

    fn into_tool_call(self) -> Option<ChatToolCall> {
        if self.function_name.trim().is_empty() {
            return None;
        }

        Some(ChatToolCall {
            id: self.id.unwrap_or_else(|| "streamed-tool-call".to_string()),
            tool_type: self.tool_type.unwrap_or_else(|| "function".to_string()),
            function: ChatToolCallFunction {
                name: self.function_name,
                arguments: self.function_arguments,
            },
        })
    }
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
    #[serde(default)]
    usage: Option<crate::provider::types::ChatUsage>,
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
    #[serde(default)]
    tool_calls: Vec<ChatStreamToolCallDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "type")]
    tool_type: Option<String>,
    #[serde(default)]
    function: Option<ChatStreamToolCallFunctionDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamToolCallFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
