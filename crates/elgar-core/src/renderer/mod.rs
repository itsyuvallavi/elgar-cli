//! Plain text rendering for core events.
//!
//! This is not the TUI renderer. It turns core session events into simple text
//! for CLI output, debugging, and tests.

#[cfg(test)]
mod tests;

use crate::{event::Event, session::Session};

pub fn placeholder_message() -> &'static str {
    "Elgar v0.10 is ready. Run `elgar` from an interactive terminal for the TUI, or pass a prompt/subcommand."
}

/// Render every event currently stored in a session as plain text.
pub fn render_session(session: &Session) -> String {
    session
        .events()
        .iter()
        .map(render_event)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render one core event without terminal colors or layout.
pub fn render_event(event: &Event) -> String {
    match event {
        Event::UserMessage(message) => format!("user: {}", message.content),
        Event::AssistantMessage(message) => {
            format!("assistant {:?}: {}", message.source, message.content)
        }
        Event::ProviderStarted(started) => {
            let mut rendered = format!(
                "provider started: {} request {}",
                started.provider, started.request_id
            );
            if let Some(model) = started.model.as_deref() {
                rendered.push_str(&format!(" model {model}"));
            }
            if let Some(mode) = started.request_mode.as_deref() {
                rendered.push_str(&format!(" mode {mode}"));
            }
            if let Some(tool_count) = started.tool_count {
                rendered.push_str(&format!(" tools {tool_count}"));
            }
            rendered
        }
        Event::ProviderFinished(finished) => {
            format!(
                "provider finished: {} request {}",
                finished.provider, finished.request_id
            )
        }
        Event::ProviderStreamChunk(_) => String::new(),
        Event::Error(error) => format!("error: {}", error.message),
    }
}
