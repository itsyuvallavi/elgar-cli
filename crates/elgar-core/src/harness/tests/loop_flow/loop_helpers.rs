//! Shared helpers for primitive harness loop tests.

use crate::{
    event::ProviderOutput,
    provider::{ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction},
};

pub(super) fn tool_call_output(name: &str, arguments: &str, id: &str) -> ProviderOutput {
    tool_calls_output(vec![(name, arguments, id)])
}

pub(super) fn tool_calls_output(calls: Vec<(&str, &str, &str)>) -> ProviderOutput {
    ProviderOutput::new("").with_tool_calls(
        calls
            .into_iter()
            .map(|(name, arguments, id)| ChatToolCall {
                id: id.to_string(),
                tool_type: "function".to_string(),
                function: ChatToolCallFunction {
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                },
            })
            .collect(),
    )
}

pub(super) fn tool_message_contents(messages: &[ChatMessage]) -> Vec<&str> {
    messages
        .iter()
        .filter(|message| matches!(message.role, ChatRole::Tool))
        .map(|message| message.content.as_str())
        .collect()
}
