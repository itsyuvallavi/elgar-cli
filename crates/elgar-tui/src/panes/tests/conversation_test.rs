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
fn renders_provider_sections_in_response_container() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "# Summary\nTodo app created.\n# Files\n- `app/page.tsx`\n# Verification\nbuild passed",
        AssistantMessageSource::Provider,
    )));

    let body = pane.render_body();
    assert!(body.contains("╭─ response"));
    assert!(body.contains("Summary"));
    assert!(body.contains("`app/page.tsx`"));
    assert!(body.contains("Verification"));
}

#[test]
fn hides_pending_approval_waiting_boilerplate() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "`write` on `hello-world.md` is prepared and waiting for approval before execution.",
        AssistantMessageSource::Provider,
    )));

    assert_eq!(pane.render_body(), "(empty conversation)");
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
