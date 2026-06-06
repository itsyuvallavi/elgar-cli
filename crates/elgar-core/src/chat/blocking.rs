//! Blocking raw chat turn.
//!
//! This path waits for the full provider response before returning events.

use std::time::Instant;

use serde_json::json;

use crate::{
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderFinished,
        ProviderStarted, UserMessage,
    },
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::{ChatMessage, ControllerProvider},
    provider_visible_text_from_text_only_output,
    session::Session,
};

use super::RawChatTurnResult;

/// Run the smallest provider turn Elgar supports.
///
/// This path deliberately avoids agent routing, tools, memory/context injection,
/// permissions, plans, shell/filesystem actions, and tool-result synthesis.
pub fn run_raw_chat_turn<P>(provider: &P, session: &mut Session, input: &str) -> RawChatTurnResult
where
    P: ControllerProvider,
{
    let start_index = session.events().len();
    let turn_id = session.next_turn_id();
    let turn_started = Instant::now();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_raw_chat_turn",
            "raw_chat_started",
        )
        .with_metadata(json!({
            "input_chars": input.chars().count(),
            "input_bytes": input.len(),
            "request_mode": "raw_chat",
            "tool_count": 0
        })),
    );
    session.push_event(Event::UserMessage(UserMessage::new(input)));

    let request = provider.request_metadata();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Provider,
            file!(),
            "run_raw_chat_turn",
            "provider_request_built",
        )
        .with_metadata(json!({
            "provider": request.provider.clone(),
            "request_id": request.request_id.clone(),
            "model": request.model.clone(),
            "request_mode": "raw_chat",
            "tool_count": 0,
            "message_count": 1
        })),
    );
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "raw_chat", 0),
    ));

    let messages = vec![ChatMessage::user(input)];
    let provider_started = Instant::now();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Provider,
            file!(),
            "run_raw_chat_turn",
            "provider_request_sent",
        )
        .with_metadata(json!({
            "request_id": request.request_id.clone(),
            "request_mode": "raw_chat",
            "tool_count": 0,
            "message_count": 1
        })),
    );
    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            let metrics = output.metrics.clone();
            let _ = append_log_event(
                &session.project_root,
                &session.id,
                LogInput::new(
                    turn_id,
                    LogPhase::Provider,
                    file!(),
                    "run_raw_chat_turn",
                    "provider_response_received",
                )
                .with_duration_ms(provider_started.elapsed().as_millis() as u64)
                .with_metadata(json!({
                    "request_id": request.request_id.clone(),
                    "text_chars": output.text.chars().count(),
                    "thinking_chars": output.thinking.as_ref().map(|value| value.chars().count()).unwrap_or(0),
                    "has_metrics": metrics.is_some(),
                    "backend": metrics.as_ref().and_then(|metrics| metrics.backend.as_ref()).map(|backend| format!("{backend:?}")),
                    "stream": metrics.as_ref().map(|metrics| metrics.stream),
                    "serialized_request_bytes": metrics.as_ref().map(|metrics| metrics.serialized_request_bytes),
                    "prompt_tokens": metrics.as_ref().and_then(|metrics| metrics.usage.as_ref()).map(|usage| usage.prompt_tokens),
                    "completion_tokens": metrics.as_ref().and_then(|metrics| metrics.usage.as_ref()).map(|usage| usage.completion_tokens),
                    "total_tokens": metrics.as_ref().and_then(|metrics| metrics.usage.as_ref()).map(|usage| usage.total_tokens)
                })),
            );
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                request.provider,
                request.request_id,
                output,
            )));
            if let Some(visible_text) = provider_visible_text_from_text_only_output(assistant_text)
            {
                let _ = append_log_event(
                    &session.project_root,
                    &session.id,
                    LogInput::new(
                        turn_id,
                        LogPhase::Session,
                        file!(),
                        "run_raw_chat_turn",
                        "assistant_message_recorded",
                    )
                    .with_metadata(json!({
                        "visible_text_chars": visible_text.chars().count()
                    })),
                );
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    visible_text,
                    AssistantMessageSource::Provider,
                )));
            }
        }
        Err(error) => {
            let _ = append_log_event(
                &session.project_root,
                &session.id,
                LogInput::new(
                    turn_id,
                    LogPhase::Error,
                    file!(),
                    "run_raw_chat_turn",
                    "provider_request_failed",
                )
                .with_duration_ms(provider_started.elapsed().as_millis() as u64)
                .with_metadata(json!({
                    "request_id": request.request_id.clone(),
                    "error": error.to_string()
                })),
            );
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} raw chat request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }

    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_raw_chat_turn",
            "raw_chat_finished",
        )
        .with_duration_ms(turn_started.elapsed().as_millis() as u64)
        .with_metadata(json!({
            "events_created": session.events().len().saturating_sub(start_index)
        })),
    );

    RawChatTurnResult {
        events: session.events()[start_index..].to_vec(),
    }
}
