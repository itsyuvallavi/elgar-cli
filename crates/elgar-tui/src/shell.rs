//! Minimal TUI shell state and event application.
//!
//! This file connects core session events to visible conversation, status, copy,
//! and simple scripted harness flows.

use std::time::Instant;

use elgar_core::{
    event::Event,
    harness::{run_harness_turn, HarnessTurnResult},
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ControllerProvider,
    session::Session,
};

use crate::{
    layout::{render_section, LayoutRegion},
    panes::{ConversationPane, CopyArea, InputArea, StatusLine},
    terminal::ui::execution_result::render_execution_result,
    turn_metrics::{aggregate_provider_token_usage, duration_millis},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiShell {
    pub conversation: ConversationPane,
    pub input: InputArea,
    pub status: StatusLine,
    pub copy: CopyArea,
}

impl TuiShell {
    pub fn new() -> Self {
        Self {
            conversation: ConversationPane::default(),
            input: InputArea::default(),
            status: StatusLine::ready(),
            copy: CopyArea::default(),
        }
    }

    pub fn regions(&self) -> [LayoutRegion; 3] {
        [
            LayoutRegion::Conversation,
            LayoutRegion::Status,
            LayoutRegion::Input,
        ]
    }

    pub fn render(&self) -> String {
        self.render_with_conversation_body(&self.conversation.render_body())
    }

    pub fn render_scripted_transcript(&self) -> String {
        self.render_with_conversation_body(&self.conversation.render_copy_body())
    }

    fn render_with_conversation_body(&self, conversation_body: &str) -> String {
        [
            render_section(LayoutRegion::Conversation.title(), conversation_body),
            render_section(LayoutRegion::Status.title(), &self.status.render_body()),
            render_section(LayoutRegion::Input.title(), &self.input.render_body()),
        ]
        .join("\n")
    }

    pub fn consume_session(&mut self, session: &Session) {
        self.consume_events(session.events());
    }

    pub fn consume_events<'a>(&mut self, events: impl IntoIterator<Item = &'a Event>) {
        for event in events {
            self.consume_event(event);
        }
    }

    pub fn consume_event(&mut self, event: &Event) {
        self.conversation.push_event(event);
        self.status.observe_event(event);
    }

    pub fn submit_harness_input<P>(
        &mut self,
        provider: &P,
        session: &mut Session,
        input: &str,
    ) -> HarnessTurnResult
    where
        P: ControllerProvider,
    {
        let started = Instant::now();
        let turn_id = session.next_turn_id();
        let _ = append_log_event(
            &session.project_root,
            &session.id,
            LogInput::new(
                turn_id,
                LogPhase::Tui,
                file!(),
                "submit_harness_input",
                "tui_harness_submitted",
            )
            .with_metadata(serde_json::json!({
                "input_chars": input.chars().count()
            })),
        );
        let result = run_harness_turn(provider, session, input);
        self.consume_events(&result.events);
        self.conversation.push_turn_metrics(
            duration_millis(started.elapsed()),
            aggregate_provider_token_usage(&result.events).as_ref(),
        );
        self.conversation.follow_latest();
        let _ = append_log_event(
            &session.project_root,
            &session.id,
            LogInput::new(
                turn_id,
                LogPhase::Render,
                file!(),
                "submit_harness_input",
                "scripted_tui_render_finished",
            )
            .with_duration_ms(duration_millis(started.elapsed()))
            .with_metadata(serde_json::json!({
                "events_applied": result.events.len(),
                "provider_started_count": count_events(&result.events, is_provider_started),
                "provider_finished_count": count_events(&result.events, is_provider_finished),
                "assistant_message_count": count_events(&result.events, is_assistant_message),
                "latest_provider_request_id": latest_provider_request_id(&result.events),
                "conversation_lines": self.conversation.render_lines_with_styles().len()
            })),
        );
        result
    }

    pub fn conversation_copy_text(&self) -> String {
        self.conversation.render_copy_body()
    }

    pub fn raw_details_copy_text(&self) -> Option<String> {
        self.conversation.render_raw_copy_body()
    }

    pub fn push_raw_details(&mut self, details: impl Into<String>) {
        self.conversation.push_raw_details(details);
    }

    pub fn push_latest_raw_details(&mut self) {
        if !self.conversation.push_latest_raw_details() {
            self.conversation
                .push_local_message("No raw details are available.");
        }
        self.conversation.follow_latest();
    }

    pub fn clear_conversation(&mut self) {
        self.conversation.clear();
    }

    pub fn push_local_message(&mut self, message: impl Into<String>) {
        self.conversation.push_local_message(message);
        self.conversation.follow_latest();
    }

    pub fn push_execution_result_message(&mut self, raw_message: String) -> String {
        let display = if let Some(display) = render_execution_result(&raw_message) {
            self.push_raw_details(raw_message);
            display
        } else {
            raw_message
        };
        self.push_local_message(display.clone());
        display
    }
}

fn count_events(events: &[Event], predicate: fn(&Event) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}

fn is_provider_started(event: &Event) -> bool {
    matches!(event, Event::ProviderStarted(_))
}

fn is_provider_finished(event: &Event) -> bool {
    matches!(event, Event::ProviderFinished(_))
}

fn is_assistant_message(event: &Event) -> bool {
    matches!(event, Event::AssistantMessage(_))
}

fn latest_provider_request_id(events: &[Event]) -> Option<&str> {
    events.iter().rev().find_map(|event| match event {
        Event::ProviderFinished(finished) => Some(finished.request_id.as_str()),
        Event::ProviderStarted(started) => Some(started.request_id.as_str()),
        _ => None,
    })
}

impl Default for TuiShell {
    fn default() -> Self {
        Self::new()
    }
}
