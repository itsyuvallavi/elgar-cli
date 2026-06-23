//! Tests for plain core event rendering.

use crate::{
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderOutput,
        ProviderStarted, UserMessage,
    },
    renderer::{render_event, render_session},
    session::Session,
};

#[test]
fn renders_current_harness_event_shapes() {
    assert_eq!(
        render_event(&Event::UserMessage(UserMessage::new("hello"))),
        "user: hello"
    );
    assert_eq!(
        render_event(&Event::AssistantMessage(AssistantMessage::new(
            "hi",
            AssistantMessageSource::Provider,
        ))),
        "assistant Provider: hi"
    );

    let started = ProviderStarted::new("lm-studio", "request-1").with_request_details(
        Some("qwen".to_string()),
        "harness_tool_decision",
        4,
    );
    assert_eq!(
        render_event(&Event::ProviderStarted(started)),
        "provider started: lm-studio request request-1 model qwen mode harness_tool_decision tools 4"
    );

    assert_eq!(
        render_event(&Event::ProviderFinished(
            crate::event::ProviderFinished::new(
                "lm-studio",
                "request-1",
                ProviderOutput::new("answer"),
            )
        )),
        "provider finished: lm-studio request request-1"
    );
    assert_eq!(
        render_event(&Event::Error(ErrorEvent::new("failed"))),
        "error: failed"
    );
}

#[test]
fn renders_session_events_as_lines() {
    let mut session = Session::new("session-1", ".", ".");
    session.push_event(Event::UserMessage(UserMessage::new("hello")));
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        "hi",
        AssistantMessageSource::Provider,
    )));

    assert_eq!(
        render_session(&session),
        "user: hello\nassistant Provider: hi"
    );
}
