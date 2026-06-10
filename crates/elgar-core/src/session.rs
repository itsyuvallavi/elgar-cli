//! Holds Elgar's in-memory session state.
//!
//! A session stores provider/user events, token accounting, and local JSONL
//! records for later inspection.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    context::ContextAccounting,
    event::{Event, ProviderMetrics},
    harness::{PendingApproval, PendingApprovalStatus},
    logs::{
        sessions,
        system::{append_log_event, LogInput, LogPhase},
    },
    token_accounting::{ContextWindowSnapshot, LastTurnTokenUsage, SessionTokenTotals},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    events: Vec<Event>,
    provider_metadata: Option<ProviderMetadata>,
    #[serde(default)]
    context_accounting: ContextAccounting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_context_window_snapshot: Option<ContextWindowSnapshot>,
    #[serde(default)]
    session_token_totals: SessionTokenTotals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_turn_token_usage: Option<LastTurnTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_approval: Option<PendingApproval>,
    #[serde(default)]
    approval_sequence: u64,
}

impl Session {
    pub fn new(
        id: impl Into<String>,
        project_root: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            project_root: project_root.into(),
            cwd: cwd.into(),
            events: Vec::new(),
            provider_metadata: None,
            context_accounting: ContextAccounting::unknown(),
            latest_context_window_snapshot: None,
            session_token_totals: SessionTokenTotals::default(),
            latest_turn_token_usage: None,
            pending_approval: None,
            approval_sequence: 0,
        }
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        self.provider_metadata.as_ref()
    }

    pub fn context_accounting(&self) -> &ContextAccounting {
        &self.context_accounting
    }

    pub fn latest_context_window_snapshot(&self) -> ContextWindowSnapshot {
        self.latest_context_window_snapshot
            .clone()
            .unwrap_or_else(|| {
                if self.context_accounting.estimated_tokens.is_some() {
                    ContextWindowSnapshot::from_context_estimate(&self.context_accounting)
                } else {
                    ContextWindowSnapshot::unknown(self.context_accounting.max_window_tokens)
                }
            })
    }

    pub fn session_token_totals(&self) -> &SessionTokenTotals {
        &self.session_token_totals
    }

    pub fn latest_turn_token_usage(&self) -> Option<&LastTurnTokenUsage> {
        self.latest_turn_token_usage.as_ref()
    }

    pub fn pending_approval(&self) -> Option<&PendingApproval> {
        self.pending_approval.as_ref()
    }

    pub(crate) fn set_pending_approval(&mut self, mut approval: PendingApproval) {
        approval.status = PendingApprovalStatus::Pending;
        self.pending_approval = Some(approval);
    }

    pub(crate) fn take_pending_approval(&mut self) -> Option<PendingApproval> {
        self.pending_approval.take()
    }

    pub(crate) fn next_approval_id(&mut self) -> String {
        self.approval_sequence = self.approval_sequence.saturating_add(1);
        format!("approval-{}", self.approval_sequence)
    }

    pub fn next_turn_id(&self) -> u64 {
        self.events
            .iter()
            .filter(|event| matches!(event, Event::UserMessage(_)))
            .count() as u64
    }

    /// Clears in-memory conversation state and rotates the session id.
    ///
    /// JSONL logs for the previous id remain on disk for audit. New turns use a
    /// fresh session id so durable memory indexes start empty.
    pub fn reset_conversation(&mut self) {
        self.events.clear();
        self.pending_approval = None;
        self.latest_turn_token_usage = None;
        self.id = rotate_session_id(&self.id);
    }

    pub(crate) fn push_event(&mut self, event: Event) {
        self.log_event(&event);
        self.log_session_system_event(&event);
        self.events.push(event);
    }

    /// Records durable harness facts in the session JSONL log.
    ///
    /// These entries are compact memory/audit facts, not full prompts or raw
    /// evidence bodies.
    pub(crate) fn log_harness_event(&self, kind: impl Into<String>, metadata: serde_json::Value) {
        let turn_index = self.next_turn_id().saturating_sub(1);
        let _ = sessions::append_session_event(
            &self.project_root,
            &self.id,
            turn_index,
            kind,
            metadata,
        );
    }

    pub(crate) fn record_provider_metrics(&mut self, metrics: &ProviderMetrics) {
        if let Some(metadata) = self.provider_metadata.as_mut() {
            metadata.model = metrics.model.clone();
            metadata.request_id = Some(metrics.request_id.clone());
            metadata.metrics = Some(metrics.clone());
        }

        let Some(usage) = metrics.usage.as_ref() else {
            return;
        };

        self.latest_context_window_snapshot = Some(ContextWindowSnapshot::from_provider_usage(
            usage,
            self.context_accounting.max_window_tokens,
            metrics.request_id.clone(),
        ));
        self.session_token_totals.add_provider_usage(usage);
        self.latest_turn_token_usage = LastTurnTokenUsage::from_provider_metrics(metrics);
    }

    fn log_event(&self, event: &Event) {
        let metadata = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
        let _ = sessions::append_session_event(
            &self.project_root,
            &self.id,
            0,
            event_log_kind(event),
            metadata,
        );
    }

    fn log_session_system_event(&self, event: &Event) {
        let event_count = self.events.len();
        let turn_id = self.log_turn_id_for_event(event);
        let _ = append_log_event(
            &self.project_root,
            &self.id,
            LogInput::new(
                turn_id,
                LogPhase::Session,
                file!(),
                "push_event",
                "session_event_recorded",
            )
            .with_metadata(json!({
                "event_kind": event_log_kind(event),
                "event_index": event_count
            })),
        );
    }

    fn log_turn_id_for_event(&self, event: &Event) -> u64 {
        let existing_user_turns = self
            .events
            .iter()
            .filter(|event| matches!(event, Event::UserMessage(_)))
            .count() as u64;
        if matches!(event, Event::UserMessage(_)) {
            existing_user_turns
        } else {
            existing_user_turns.saturating_sub(1)
        }
    }
}

fn rotate_session_id(current: &str) -> String {
    if let Some((base, suffix)) = current.rsplit_once("-clear-") {
        if let Ok(generation) = suffix.parse::<u32>() {
            return format!("{base}-clear-{}", generation + 1);
        }
    }
    format!("{current}-clear-1")
}

fn event_log_kind(event: &Event) -> &'static str {
    match event {
        Event::UserMessage(_) => "user_message",
        Event::AssistantMessage(_) => "assistant_message",
        Event::ProviderStarted(_) => "provider_started",
        Event::ProviderFinished(_) => "provider_finished",
        Event::Error(_) => "error",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ProviderMetrics>,
}

impl ProviderMetadata {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: None,
            request_id: None,
            metrics: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AssistantMessage, AssistantMessageSource, Event, UserMessage};

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
}
