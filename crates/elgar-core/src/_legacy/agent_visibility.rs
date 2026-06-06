use serde_json::Value;

use crate::{
    event::{AssistantMessage, AssistantMessageSource, Event, VerifiedActionResult},
    model_runtime::RawModelToolCall,
    provider::{ChatMessage, ChatToolCall, ChatToolCallFunction},
    provider_visible_text_from_text_only_output,
    session::Session,
};

pub(crate) fn chat_assistant_tool_call_message(
    content: String,
    tool_calls: &[RawModelToolCall],
) -> ChatMessage {
    ChatMessage::assistant(content).with_tool_calls(
        tool_calls
            .iter()
            .map(|tool_call| ChatToolCall {
                id: tool_call.id.clone(),
                tool_type: "function".to_string(),
                function: ChatToolCallFunction {
                    name: tool_call.name.raw_label(),
                    arguments: arguments_json_string(&tool_call.arguments),
                },
            })
            .collect(),
    )
}

fn arguments_json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn push_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message = message.into();
    if let Some(message) = provider_visible_text_from_text_only_output(message) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            message,
            AssistantMessageSource::Provider,
        )));
    }
}

pub(crate) fn push_provider_message_after_tool_turn_if_visible(
    session: &mut Session,
    turn_start_index: usize,
    message: impl Into<String>,
    allow_provider_message_after_verified_action: bool,
) {
    if turn_has_verified_action_applied(session, turn_start_index) {
        debug_assert!(session
            .actions_in_latest_action_turn()
            .iter()
            .any(|record| record.verified_result.is_some()));
        if allow_provider_message_after_verified_action
            || turn_verified_actions_are_shell_only(session, turn_start_index)
        {
            push_provider_message_if_visible(session, message);
        }
        return;
    }

    let message = message.into();
    if allow_provider_message_after_verified_action {
        push_provider_message_if_visible(session, message);
        return;
    }

    if provider_visible_text_from_text_only_output(message).is_some() {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "No verified filesystem change occurred this turn, so no completion claim was recorded.",
            AssistantMessageSource::Controller,
        )));
    }
}

fn turn_has_verified_action_applied(session: &Session, turn_start_index: usize) -> bool {
    session
        .events()
        .iter()
        .skip(turn_start_index)
        .any(|event| matches!(event, Event::ActionApplied(_)))
}

fn turn_verified_actions_are_shell_only(session: &Session, turn_start_index: usize) -> bool {
    let mut saw_shell = false;
    for event in session.events().iter().skip(turn_start_index) {
        let Event::ActionApplied(applied) = event else {
            continue;
        };
        match applied.result {
            VerifiedActionResult::Shell(_) => saw_shell = true,
            _ => return false,
        }
    }
    saw_shell
}

pub(crate) fn push_plain_provider_message_if_visible(
    session: &mut Session,
    message: impl Into<String>,
) {
    let message = message.into();
    if looks_like_raw_tool_protocol(&message) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "The model returned raw tool protocol as text, so no filesystem action was executed. Ask again normally so the model can choose the execute route.",
            AssistantMessageSource::Controller,
        )));
        return;
    }

    push_provider_message_if_visible(session, message);
}

pub(crate) fn looks_like_raw_tool_protocol(message: &str) -> bool {
    [
        "to=filesystem.",
        "filesystem.create",
        "filesystem.write",
        "filesystem.patch",
        "filesystem.move",
        "filesystem.delete",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}
