//! Tests for cross-turn chat history in harness prompts.

use crate::{
    event::{AssistantMessage, AssistantMessageSource, Event, UserMessage},
    harness::run_primitive_harness_loop,
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;

#[test]
fn primitive_loop_includes_prior_user_and_assistant_messages() {
    let root = std::env::temp_dir().join(format!("elgar-session-history-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();

    let provider = QueuedProvider::new(vec!["Continuing from earlier context."]);
    let mut session = Session::new("history-session", &root, &root);
    session.push_event(Event::UserMessage(UserMessage::new("read package.json")));
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        "Summarized package.json.",
        AssistantMessageSource::Provider,
    )));
    session.push_event(Event::UserMessage(UserMessage::new("list app")));
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        "Listed app files.",
        AssistantMessageSource::Provider,
    )));

    let result =
        run_primitive_harness_loop(&provider, &mut session, "what did we do so far?").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(calls.len(), 1);
    assert!(calls[0].len() > 2);
    assert!(calls[0][0]
        .content
        .contains("When the user directly asks to open/show a file"));
    assert!(calls[0][0]
        .content
        .contains("ask one concise clarification question"));
    assert!(calls[0]
        .iter()
        .any(|message| message.content.contains("read package.json")));
    assert!(calls[0]
        .iter()
        .any(|message| message.content.contains("Summarized package.json")));
    assert_eq!(
        result.final_text.as_deref(),
        Some("Continuing from earlier context.")
    );

    let _ = std::fs::remove_dir_all(root);
}
