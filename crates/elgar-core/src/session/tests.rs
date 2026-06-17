//! Tests for in-memory session state.

use super::*;
use crate::event::{
    AssistantMessage, AssistantMessageSource, Event, ProviderFinished, ProviderOutput,
    ProviderStreamChunkReceived, ProviderStreamTimings, UserMessage,
};
use crate::provider::ProviderStreamChunk;
use crate::token_accounting::ProviderTokenUsage;

#[test]
fn reset_conversation_clears_events_and_rotates_session_id() {
    let root = std::env::temp_dir().join(format!("elgar-session-reset-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();

    let mut session = Session::new("terminal-tui-session", &root, &root);
    session.push_event(Event::UserMessage(UserMessage::new("hello")));
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        "hi",
        AssistantMessageSource::Provider,
    )));

    session.reset_conversation();

    assert!(session.events().is_empty());
    assert_eq!(session.id, "terminal-tui-session-clear-1");

    session.reset_conversation();
    assert_eq!(session.id, "terminal-tui-session-clear-2");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_finished_session_metadata_includes_stream_timings() {
    let event = Event::ProviderFinished(
        ProviderFinished::new("lm-studio", "request-1", ProviderOutput::new("ok"))
            .with_stream_timings(ProviderStreamTimings::from_stream_marks(
                Some(1200),
                Some(3400),
                Some(2400),
                Some(4600),
                Some(4800),
                5000,
            )),
    );

    let metadata = session_event_metadata(&event);

    assert_eq!(metadata["first_reasoning_ms"], 1200);
    assert_eq!(metadata["first_text_ms"], 3400);
    assert_eq!(metadata["last_reasoning_ms"], 2400);
    assert_eq!(metadata["last_text_ms"], 4600);
    assert_eq!(metadata["last_chunk_ms"], 4800);
    assert_eq!(metadata["reasoning_to_text_ms"], 2200);
    assert_eq!(metadata["last_chunk_to_finish_ms"], 200);
    assert_eq!(metadata["total_stream_ms"], 5000);
}

#[test]
fn runtime_session_id_uses_prefix_and_is_unique() {
    let first = runtime_session_id("terminal-tui");
    let second = runtime_session_id("terminal-tui");

    assert!(first.starts_with("terminal-tui-"));
    assert!(second.starts_with("terminal-tui-"));
    assert_ne!(first, second);
}

#[test]
fn provider_finished_session_metadata_includes_thinking_diagnostics() {
    let event = Event::ProviderFinished(ProviderFinished::new(
        "lm-studio",
        "request-1",
        ProviderOutput::new("ok").with_thinking("count me"),
    ));

    let metadata = session_event_metadata(&event);

    assert_eq!(metadata["provider_response_has_thinking"], true);
    assert_eq!(metadata["provider_response_thinking_chars"], 8);
}

#[test]
fn provider_stream_chunk_session_metadata_includes_chunk_diagnostics() {
    let event = Event::ProviderStreamChunk(ProviderStreamChunkReceived::new(
        "lm-studio",
        "request-1",
        1,
        ProviderStreamChunk::Reasoning("partial thought".to_string()),
    ));

    let metadata = session_event_metadata(&event);

    assert_eq!(metadata["provider_stream_chunk_kind"], "reasoning");
    assert_eq!(metadata["provider_stream_chunk_chars"], 15);
}

#[test]
fn provider_metrics_use_configured_context_window_for_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "elgar-session-context-window-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("session-context-window", &root, &root);
    session.set_context_window_tokens(Some(16_000));
    let mut metrics = ProviderMetrics::new("request-1", Some("model".to_string()), false, 2, 1000);
    metrics.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(1_280),
        completion_tokens: Some(112),
        total_tokens: Some(1_392),
    });

    session.record_provider_metrics(&metrics);

    let snapshot = session.latest_context_window_snapshot();
    assert_eq!(snapshot.context_window_tokens, Some(16_000));
    assert_eq!(snapshot.current_tokens, Some(1_392));
    assert_eq!(snapshot.used_percent, Some(8));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_context_window_snapshot_accumulates_session_tokens() {
    let root = std::env::temp_dir().join(format!(
        "elgar-session-context-window-total-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("session-context-window-total", &root, &root);
    session.set_context_window_tokens(Some(16_000));

    let mut first = ProviderMetrics::new("request-1", Some("model".to_string()), false, 1, 1000);
    first.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(1_000),
        completion_tokens: Some(250),
        total_tokens: Some(1_250),
    });
    session.record_provider_metrics(&first);

    let mut second = ProviderMetrics::new("request-2", Some("model".to_string()), false, 1, 1000);
    second.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(1_500),
        completion_tokens: Some(500),
        total_tokens: Some(2_000),
    });
    session.record_provider_metrics(&second);

    let snapshot = session.latest_context_window_snapshot();
    assert_eq!(snapshot.current_tokens, Some(3_250));
    assert_eq!(snapshot.context_window_tokens, Some(16_000));
    assert_eq!(snapshot.used_percent, Some(20));
    assert_eq!(session.session_token_totals().total_tokens, 3_250);

    let _ = std::fs::remove_dir_all(root);
}
