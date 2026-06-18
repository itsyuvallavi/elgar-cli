//! Tests for conversation pane event handling.

use elgar_core::event::{
    AssistantMessage, AssistantMessageSource, Event, ProviderFinished, ProviderOutput,
    ProviderStarted,
};

use super::super::ConversationPane;

#[test]
fn renders_harness_provider_reasoning_in_visible_conversation() {
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
        ProviderOutput::new("")
            .with_thinking("The user is asking for a greeting. I should answer briefly."),
    )));

    assert!(pane
        .render_body()
        .contains("The user is asking for a greeting. I should answer briefly."));
    assert!(!pane.render_body().contains("reasoning · "));
}

#[test]
fn keeps_final_assistant_message_visible_after_reasoning() {
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
        ProviderOutput::new("").with_thinking("Need to answer from verified context."),
    )));
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "Final answer",
        AssistantMessageSource::Provider,
    )));

    let body = pane.render_body();
    assert!(body.contains("Need to answer from verified context."));
    assert!(!body.contains("reasoning · "));
    assert!(body.contains("Final answer"));
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
fn renders_harness_synthesis_provider_reasoning_in_visible_conversation() {
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
        ProviderOutput::new("Final answer").with_thinking("Need to summarize the verified result."),
    )));
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "Final answer",
        AssistantMessageSource::Provider,
    )));

    let body = pane.render_body();
    assert!(body.contains("Need to summarize the verified result."));
    assert!(!body.contains("reasoning · "));
    assert!(body.contains("Final answer"));
}

#[test]
fn copy_body_omits_provider_reasoning() {
    let mut pane = ConversationPane::default();
    pane.push_event(&Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "request-1",
        ProviderOutput::new("").with_thinking("Reasoning should stay visible in chat."),
    )));
    pane.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "Final answer",
        AssistantMessageSource::Provider,
    )));

    assert!(pane
        .render_body()
        .contains("Reasoning should stay visible in chat."));
    assert!(!pane
        .render_copy_body()
        .contains("Reasoning should stay visible in chat."));
    assert!(pane.render_copy_body().contains("Final answer"));
}

#[test]
fn long_provider_reasoning_is_compact_but_kept_in_details() {
    let long_reasoning = [
        "The user is asking about my capabilities.",
        "I should inspect the available tool list from the prompt.",
        "I should avoid claiming tools that are unavailable.",
        "I should explain file reading, listing, finding, grep searching, bash commands, writing, and editing.",
        "I should also mention approval for side effects.",
        "I should keep the final answer concise and direct.",
        "I should not repeat every instruction verbatim in normal chat.",
        "I should leave the full diagnostic text in details for inspection.",
    ]
    .join(" ");

    let mut pane = ConversationPane::default();
    pane.push_event(&Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "request-1",
        ProviderOutput::new("").with_thinking(&long_reasoning),
    )));

    let body = pane.render_body();
    assert!(
        body.starts_with("The user is asking about my capabilities."),
        "{body}"
    );
    assert!(
        body.contains("I should inspect the available tool list from the prompt."),
        "{body}"
    );
    assert!(!body.contains("I should keep the final answer concise and direct."));
    assert!(!body.contains("reasoning · "), "{body}");
    assert!(!body.ends_with("..."), "{body}");
    assert!(body.len() < long_reasoning.len(), "{body}");

    let details = pane.latest_raw_details().expect("raw reasoning details");
    assert!(details.contains("Provider reasoning details"));
    assert!(details.contains("I should keep the final answer concise and direct."));
}
