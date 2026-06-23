//! Synthetic tool-call helpers for JSON fallback model choices.
//!
//! Native provider tool calls are preferred. These helpers preserve the same
//! role/tool-result conversation shape when a fallback JSON request is accepted.

use crate::{
    harness::{
        harness_loop::control::choice_from_output::NativeToolRequest, ValidatedStructuredRequest,
    },
    provider::{ChatMessage, ChatToolCall, ChatToolCallFunction},
};

pub(super) fn synthetic_native_tool_request(
    round_index: usize,
    request_index: usize,
    request: ValidatedStructuredRequest,
) -> NativeToolRequest {
    NativeToolRequest {
        tool_call_id: format!("json-fallback-{round_index}-{request_index}"),
        request,
    }
}

pub(super) fn synthetic_assistant_tool_call(request: &NativeToolRequest) -> ChatMessage {
    synthetic_assistant_tool_calls(std::slice::from_ref(request))
}

pub(super) fn synthetic_assistant_tool_calls(requests: &[NativeToolRequest]) -> ChatMessage {
    let tool_calls = requests
        .iter()
        .map(|request| ChatToolCall {
            id: request.tool_call_id.clone(),
            tool_type: "function".to_string(),
            function: ChatToolCallFunction {
                name: request.request.kind.as_str().to_string(),
                arguments: request
                    .request
                    .arguments
                    .as_ref()
                    .map(serde_json::Value::to_string)
                    .unwrap_or_else(|| "{}".to_string()),
            },
        })
        .collect::<Vec<_>>();

    ChatMessage::assistant("").with_tool_calls(tool_calls)
}
