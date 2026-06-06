//! Streaming raw chat turn.
//!
//! This path forwards provider chunks while the final response is still being
//! built, then records the same final session events as blocking raw chat.

use crate::{
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderFinished,
        ProviderStarted, UserMessage,
    },
    provider::{ChatMessage, ControllerProvider, ProviderStreamChunk},
    provider_visible_text_from_text_only_output,
    session::Session,
};

use super::RawChatTurnResult;

/// Run raw chat while forwarding provider stream chunks to the caller.
///
/// This is the same raw path as `run_raw_chat_turn`, but it lets the TUI see
/// reasoning/text chunks before the final provider output is complete.
pub fn run_raw_chat_turn_streaming<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> RawChatTurnResult
where
    P: ControllerProvider,
{
    let start_index = session.events().len();
    session.push_event(Event::UserMessage(UserMessage::new(input)));

    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "raw_chat", 0),
    ));

    let messages = vec![ChatMessage::user(input)];
    match provider.chat_messages_streaming_with_metadata(messages, &request, on_chunk) {
        Ok(output) => {
            let assistant_text = output.text.clone();
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
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    visible_text,
                    AssistantMessageSource::Provider,
                )));
            }
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} raw chat request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }

    RawChatTurnResult {
        events: session.events()[start_index..].to_vec(),
    }
}
