//! Session event metadata helpers.

use serde_json::json;

use crate::event::Event;

pub(super) fn session_event_metadata(event: &Event) -> serde_json::Value {
    let mut metadata = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
    let Event::ProviderFinished(finished) = event else {
        return metadata;
    };

    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "provider_response_has_thinking".to_string(),
            json!(finished.output.has_thinking()),
        );
        object.insert(
            "provider_response_thinking_chars".to_string(),
            json!(finished.output.thinking_chars()),
        );
    }
    metadata
}

pub(super) fn event_log_kind(event: &Event) -> &'static str {
    match event {
        Event::UserMessage(_) => "user_message",
        Event::AssistantMessage(_) => "assistant_message",
        Event::ProviderStarted(_) => "provider_started",
        Event::ProviderFinished(_) => "provider_finished",
        Event::Error(_) => "error",
    }
}
