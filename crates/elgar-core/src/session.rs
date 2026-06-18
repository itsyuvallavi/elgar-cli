//! Holds Elgar's in-memory session state.
//!
//! A session stores provider/user events, token accounting, and local JSONL
//! records for later inspection.

mod id;
mod logging;
mod status_logging;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    context::ContextAccounting,
    event::{Event, ProviderMetrics},
    harness::{PendingApproval, PendingApprovalStatus, PermissionMode},
    logs::{
        sessions,
        system::{append_log_event, LogInput, LogPhase},
    },
    token_accounting::{ContextWindowSnapshot, LastTurnTokenUsage, SessionTokenTotals},
};

use id::rotate_session_id;
pub use id::runtime_session_id;
use logging::{event_log_kind, session_event_metadata};
use status_logging::log_session_context_status;

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
    #[serde(default)]
    permission_mode: PermissionMode,
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
            permission_mode: PermissionMode::default(),
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

    pub fn set_context_window_tokens(&mut self, context_window_tokens: Option<u64>) {
        self.context_accounting.max_window_tokens = context_window_tokens;
        self.latest_context_window_snapshot = None;
    }

    pub fn latest_context_window_snapshot(&self) -> ContextWindowSnapshot {
        self.latest_context_window_snapshot
            .clone()
            .unwrap_or_else(|| {
                if self.session_token_totals.total_tokens > 0 {
                    ContextWindowSnapshot::from_session_totals(
                        &self.session_token_totals,
                        self.context_accounting.max_window_tokens,
                        self.provider_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.request_id.clone())
                            .unwrap_or_default(),
                    )
                } else if self.context_accounting.estimated_tokens.is_some() {
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

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode = mode;
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

        self.session_token_totals.add_provider_usage(usage);
        self.latest_context_window_snapshot = Some(ContextWindowSnapshot::from_session_totals(
            &self.session_token_totals,
            self.context_accounting.max_window_tokens,
            metrics.request_id.clone(),
        ));
        self.latest_turn_token_usage = LastTurnTokenUsage::from_provider_metrics(metrics);
        log_session_context_status(self);
    }

    fn log_event(&self, event: &Event) {
        let metadata = session_event_metadata(event);
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
                "event_index": event_count,
                "request_id": event_request_id(event),
                "provider_stream_total_ms": event_provider_stream_total_ms(event),
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

fn event_request_id(event: &Event) -> Option<&str> {
    match event {
        Event::ProviderStarted(started) => Some(started.request_id.as_str()),
        Event::ProviderFinished(finished) => Some(finished.request_id.as_str()),
        Event::ProviderStreamChunk(chunk) => Some(chunk.request_id.as_str()),
        _ => None,
    }
}

fn event_provider_stream_total_ms(event: &Event) -> Option<u64> {
    match event {
        Event::ProviderFinished(finished) => finished
            .stream_timings
            .as_ref()
            .map(|timings| timings.total_ms),
        _ => None,
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
