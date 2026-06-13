//! Tests for conversation pane event handling.

use elgar_core::event::{
    AssistantMessage, AssistantMessageSource, Event, ProviderFinished, ProviderOutput,
    ProviderStarted,
};

use super::super::ConversationPane;

#[test]
fn hides_harness_provider_reasoning_from_visible_conversation() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::ProviderStarted(
        ProviderStarted::new("lm-studio", "decision-1").with_request_details(
            Some("qwen".to_string()),
            "harness_tool_decision",
            0,
        ),
    ));
    pane.push_event(&Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "decision-1",
        ProviderOutput::new("").with_thinking(r#"{"type":"structured_requests","requests":[]}"#),
    )));

    assert!(!pane.render_body().contains("structured_requests"));
}

#[test]
fn keeps_final_assistant_message_visible_after_hidden_decision() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::ProviderStarted(
        ProviderStarted::new("lm-studio", "decision-1").with_request_details(
            Some("qwen".to_string()),
            "harness_tool_decision",
            0,
        ),
    ));
    pane.push_event(&Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "decision-1",
        ProviderOutput::new("").with_thinking(r#"{"type":"structured_requests","requests":[]}"#),
    )));
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "Final answer",
        AssistantMessageSource::Provider,
    )));

    assert!(pane.render_body().contains("Final answer"));
    assert!(!pane.render_body().contains("structured_requests"));
}

#[test]
fn hides_harness_synthesis_provider_reasoning_from_visible_conversation() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::ProviderStarted(
        ProviderStarted::new("lm-studio", "synthesis-1").with_request_details(
            Some("qwen".to_string()),
            "harness_synthesis",
            0,
        ),
    ));
    pane.push_event(&Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "synthesis-1",
        ProviderOutput::new("Final answer")
            .with_thinking(r#"{"type":"structured_requests","requests":[]}"#),
    )));
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "Final answer",
        AssistantMessageSource::Provider,
    )));

    assert!(pane.render_body().contains("Final answer"));
    assert!(!pane.render_body().contains("structured_requests"));
}
