use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    action::{Action, ActionLifecycleState},
    event::{Event, ProviderMetrics, VerifiedActionResult},
};

/// Core-owned state for one controller session.
///
/// This is an inspectable record of controller facts. Provider events and
/// metadata may capture what a provider said or which provider was used, but
/// they do not prove filesystem state, action success, or verified results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    events: Vec<Event>,
    actions: Vec<ActionRecord>,
    provider_metadata: Option<ProviderMetadata>,
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
            actions: Vec::new(),
            provider_metadata: None,
        }
    }

    /// Controller-recorded event facts for read-only UI and renderer consumers.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Controller-owned action records for read-only UI and renderer consumers.
    pub fn actions(&self) -> &[ActionRecord] {
        &self.actions
    }

    /// Select the one action still waiting on user approval/rejection.
    ///
    /// Only `Proposed` actions are pending. `Approved`, `Applied`, `Rejected`,
    /// and `Failed` records are non-pending for selection, including when a
    /// session is restored with those states already present.
    pub fn pending_action_selection(&self) -> PendingActionSelection {
        let mut proposed = self
            .actions
            .iter()
            .enumerate()
            .filter(|(_index, record)| record.action.state == ActionLifecycleState::Proposed);

        let Some((index, _record)) = proposed.next() else {
            return PendingActionSelection::None;
        };

        if proposed.next().is_some() {
            PendingActionSelection::Ambiguous
        } else {
            PendingActionSelection::Single(index)
        }
    }

    /// Provider request metadata recorded by the controller for inspection only.
    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        self.provider_metadata.as_ref()
    }

    pub(crate) fn push_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub(crate) fn push_action(&mut self, action: ActionRecord) {
        self.actions.push(action);
    }

    pub(crate) fn action_mut(&mut self, index: usize) -> Option<&mut ActionRecord> {
        self.actions.get_mut(index)
    }

    pub(crate) fn set_provider_metadata(&mut self, metadata: ProviderMetadata) {
        self.provider_metadata = Some(metadata);
    }
}

/// A data-only record of an action as known by the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action: Action,
    pub verified_result: Option<VerifiedActionResult>,
    pub failure_reason: Option<String>,
}

impl ActionRecord {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            verified_result: None,
            failure_reason: None,
        }
    }
}

pub type ActionState = ActionLifecycleState;

/// Deterministic result of selecting a pending action from a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingActionSelection {
    /// No `Proposed` action exists.
    None,
    /// Exactly one `Proposed` action exists, addressed by session action index.
    Single(usize),
    /// More than one `Proposed` action exists, so no action is selected.
    Ambiguous,
}

/// Provider configuration/request metadata recorded for inspection.
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
    use std::path::PathBuf;

    use crate::action::{Action, ActionLifecycleState};
    use crate::event::{
        ActionKind, AssistantMessage, AssistantMessageSource, Event, ProviderFinished,
        ProviderOutput, VerifiedActionResult,
    };

    use super::{ActionRecord, PendingActionSelection, ProviderMetadata, Session};

    #[test]
    fn new_session_stores_identity_paths_and_empty_state() {
        let session = Session::new("session-1", "/repo", "/repo/crates");

        assert_eq!(session.id, "session-1");
        assert_eq!(session.project_root, PathBuf::from("/repo"));
        assert_eq!(session.cwd, PathBuf::from("/repo/crates"));
        assert!(session.events.is_empty());
        assert!(session.actions.is_empty());
        assert_eq!(session.provider_metadata, None);

        let debug = format!("{session:?}");
        assert!(debug.contains("session-1"));
        assert!(debug.contains("project_root"));
    }

    #[test]
    fn session_can_hold_controller_events_action_records_and_provider_metadata() {
        let mut session = Session::new("session-2", "/repo", "/repo");

        session
            .events
            .push(Event::AssistantMessage(AssistantMessage::new(
                "I can suggest writing hello.py.",
                AssistantMessageSource::Provider,
            )));

        let mut action = ActionRecord::new(
            Action::proposed_write_file("action-1", "hello.py", "contents", "write hello.py")
                .approve()
                .mark_applied(),
        );
        action.verified_result = Some(VerifiedActionResult::FileWritten {
            path: "hello.py".to_string(),
        });
        session.actions.push(action);

        let mut provider_metadata = ProviderMetadata::new("lm-studio");
        provider_metadata.model = Some("local-model".to_string());
        provider_metadata.request_id = Some("request-1".to_string());
        session.provider_metadata = Some(provider_metadata);

        assert_eq!(session.events.len(), 1);
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Applied
        );
        assert_eq!(session.actions[0].action.kind(), ActionKind::CreateFile);
        assert_eq!(
            session.actions[0].verified_result,
            Some(VerifiedActionResult::FileWritten {
                path: "hello.py".to_string()
            })
        );
        assert_eq!(
            session
                .provider_metadata
                .as_ref()
                .map(|metadata| metadata.provider.as_str()),
            Some("lm-studio")
        );
    }

    #[test]
    fn provider_prose_does_not_create_action_or_verified_truth() {
        let mut session = Session::new("session-3", "/repo", "/repo");
        session.provider_metadata = Some(ProviderMetadata::new("stub-provider"));
        session
            .events
            .push(Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("I wrote hello.py successfully."),
            )));

        assert!(session.actions.is_empty());
        assert!(session
            .actions
            .iter()
            .all(|action| action.verified_result.is_none()));
    }

    #[test]
    fn provider_prose_does_not_advance_existing_action_state() {
        let mut session = Session::new("session-4", "/repo", "/repo");
        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-1",
                "hello.py",
                "contents",
                "write hello.py",
            )));

        session
            .events
            .push(Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("Approved and wrote hello.py."),
            )));

        assert_eq!(session.actions.len(), 1);
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions[0].verified_result, None);
    }

    #[test]
    fn pending_action_selection_is_explicit_for_zero_one_and_multiple_proposed_actions() {
        let mut session = Session::new("session-5", "/repo", "/repo");

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::None
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-1",
                "first.py",
                "",
                "write first.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Single(0)
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-2",
                "second.py",
                "",
                "write second.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Ambiguous
        );
    }

    #[test]
    fn pending_action_selection_ignores_non_proposed_terminal_states() {
        let mut session = Session::new("session-6", "/repo", "/repo");
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-1", "approved.py", "", "write approved.py")
                .approve(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-2", "applied.py", "", "write applied.py")
                .approve()
                .mark_applied(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-3", "rejected.py", "", "write rejected.py")
                .reject(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-4", "failed.py", "", "write failed.py")
                .mark_failed(),
        ));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::None
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-5",
                "pending.py",
                "",
                "write pending.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Single(4)
        );
    }
}
