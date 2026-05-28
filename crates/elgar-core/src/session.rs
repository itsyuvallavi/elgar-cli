use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    action::{Action, ActionLifecycleState},
    context::ContextAccounting,
    event::{Event, ProviderMetrics, VerifiedActionResult},
    policy::PolicyDecision,
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
    #[serde(default)]
    project_memory: ProjectMemory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_provider_prompt_memory_selection: Option<ProviderPromptMemorySelection>,
    #[serde(default)]
    context_accounting: ContextAccounting,
}

pub const PROJECT_MEMORY_LIMIT: usize = 8;
pub const PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT: usize = PROJECT_MEMORY_LIMIT;

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
            project_memory: ProjectMemory::default(),
            latest_provider_prompt_memory_selection: None,
            context_accounting: ContextAccounting::unknown(),
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

    /// Controller-owned project references learned only from verified actions.
    pub fn project_memory(&self) -> &ProjectMemory {
        &self.project_memory
    }

    /// Latest provider prompt memory selection trace recorded by the controller.
    pub fn latest_provider_prompt_memory_selection(
        &self,
    ) -> Option<&ProviderPromptMemorySelection> {
        self.latest_provider_prompt_memory_selection.as_ref()
    }

    /// Controller-recorded context accounting for UI display and provider budgeting.
    pub fn context_accounting(&self) -> &ContextAccounting {
        &self.context_accounting
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

    pub(crate) fn set_context_accounting(&mut self, context_accounting: ContextAccounting) {
        self.context_accounting = context_accounting;
    }

    pub(crate) fn record_verified_folder_reference(&mut self, reference: VerifiedFolderReference) {
        self.project_memory.remember_verified_folder(reference);
    }

    pub(crate) fn record_verified_plan_reference(&mut self, reference: VerifiedPlanReference) {
        self.project_memory.remember_verified_plan(reference);
    }

    pub(crate) fn record_structured_project_plan(&mut self, plan: StructuredProjectPlan) {
        self.project_memory.remember_structured_plan(plan);
    }

    pub(crate) fn set_latest_provider_prompt_memory_selection(
        &mut self,
        selection: Option<ProviderPromptMemorySelection>,
    ) {
        self.latest_provider_prompt_memory_selection = selection.map(|mut selection| {
            selection.bound();
            selection
        });
    }

    pub(crate) fn mark_structured_project_plan_executed(&mut self, action_id: &str) {
        self.project_memory.mark_structured_plan_executed(action_id);
    }

    pub(crate) fn mark_latest_structured_project_plan_executing(&mut self) {
        self.project_memory
            .mark_latest_structured_plan_status(StructuredProjectPlanStatus::Executing);
    }

    pub(crate) fn mark_latest_structured_project_plan_completed(&mut self) {
        self.project_memory
            .mark_latest_structured_plan_status(StructuredProjectPlanStatus::Completed);
    }

    pub(crate) fn remove_structured_project_plan_for_action(&mut self, action_id: &str) {
        self.project_memory
            .remove_structured_plan_for_action(action_id);
    }
}

/// A data-only record of an action as known by the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action: Action,
    pub verified_result: Option<VerifiedActionResult>,
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision: Option<PolicyDecision>,
}

impl ActionRecord {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            verified_result: None,
            failure_reason: None,
            policy_decision: None,
        }
    }
}

pub type ActionState = ActionLifecycleState;

/// Bounded trace of memory facts selected or omitted for the latest provider prompt.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderPromptMemorySelection {
    #[serde(default)]
    pub selected: Vec<ProviderPromptMemorySelectedFact>,
    #[serde(default)]
    pub omitted: Vec<ProviderPromptMemoryOmittedFact>,
}

impl ProviderPromptMemorySelection {
    pub fn new(
        selected: Vec<ProviderPromptMemorySelectedFact>,
        omitted: Vec<ProviderPromptMemoryOmittedFact>,
    ) -> Self {
        let mut selection = Self { selected, omitted };
        selection.bound();
        selection
    }

    fn bound(&mut self) {
        trim_to_limit(
            &mut self.selected,
            PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT,
        );
        trim_to_limit(
            &mut self.omitted,
            PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPromptMemorySelectedFact {
    pub kind: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,
    pub source_action_id: String,
}

impl ProviderPromptMemorySelectedFact {
    pub fn new(
        kind: impl Into<String>,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            path,
            project_root,
            source_action_id: source_action_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPromptMemoryOmittedFact {
    pub kind: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,
    pub source_action_id: String,
    pub reason: String,
}

impl ProviderPromptMemoryOmittedFact {
    pub fn new(
        kind: impl Into<String>,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            path,
            project_root,
            source_action_id: source_action_id.into(),
            reason: reason.into(),
        }
    }
}

/// Controller-owned memory for project-building references.
///
/// This is not provider memory. Entries are created only by controller code
/// after approved filesystem/shell actions have verified their expected
/// effects, except structured plans, which are controller proposals derived
/// from verified plan files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectMemory {
    #[serde(default)]
    pub verified_folders: Vec<VerifiedFolderReference>,
    #[serde(default)]
    pub verified_plans: Vec<VerifiedPlanReference>,
    #[serde(default)]
    pub structured_plans: Vec<StructuredProjectPlan>,
}

impl ProjectMemory {
    pub fn latest_verified_folder(&self) -> Option<&VerifiedFolderReference> {
        self.verified_folders.last()
    }

    pub fn latest_verified_plan(&self) -> Option<&VerifiedPlanReference> {
        self.verified_plans.last()
    }

    pub fn latest_structured_plan(&self) -> Option<&StructuredProjectPlan> {
        self.structured_plans.last()
    }

    pub fn latest_executed_structured_plan(&self) -> Option<&StructuredProjectPlan> {
        self.structured_plans
            .iter()
            .rev()
            .find(|plan| plan.runtime_status() == StructuredProjectPlanStatus::Completed)
    }

    fn remember_verified_folder(&mut self, reference: VerifiedFolderReference) {
        self.verified_folders
            .retain(|existing| existing.path != reference.path);
        self.verified_folders.push(reference);
        trim_to_memory_limit(&mut self.verified_folders);
    }

    fn remember_verified_plan(&mut self, reference: VerifiedPlanReference) {
        self.verified_plans
            .retain(|existing| existing.path != reference.path);
        self.verified_plans.push(reference);
        trim_to_memory_limit(&mut self.verified_plans);
    }

    fn remember_structured_plan(&mut self, plan: StructuredProjectPlan) {
        self.structured_plans
            .retain(|existing| existing.source_plan_path != plan.source_plan_path);
        self.structured_plans.push(plan);
        trim_to_memory_limit(&mut self.structured_plans);
    }

    fn mark_structured_plan_executed(&mut self, action_id: &str) {
        if let Some(plan) = self
            .structured_plans
            .iter_mut()
            .rev()
            .find(|plan| plan.source_action_id.as_deref() == Some(action_id))
        {
            plan.status = StructuredProjectPlanStatus::Completed;
        }
    }

    fn mark_latest_structured_plan_status(&mut self, status: StructuredProjectPlanStatus) {
        if let Some(plan) = self.structured_plans.last_mut() {
            plan.status = status;
        }
    }

    fn remove_structured_plan_for_action(&mut self, action_id: &str) {
        self.structured_plans
            .retain(|plan| plan.source_action_id.as_deref() != Some(action_id));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedFolderReference {
    pub path: PathBuf,
    pub source_action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPlanReference {
    pub path: PathBuf,
    pub project_root: PathBuf,
    pub source_action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredProjectPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action_id: Option<String>,
    pub source_plan_path: PathBuf,
    pub project_root: PathBuf,
    pub stage: String,
    #[serde(default)]
    pub status: StructuredProjectPlanStatus,
    pub expected_directories: Vec<PathBuf>,
    pub expected_files: Vec<PathBuf>,
}

impl StructuredProjectPlan {
    pub fn runtime_status(&self) -> StructuredProjectPlanStatus {
        if self.is_stale() {
            return StructuredProjectPlanStatus::Stale;
        }

        if self.has_expected_paths() && self.expected_paths_complete() {
            return StructuredProjectPlanStatus::Completed;
        }

        self.status
    }

    pub fn expected_directories_present_count(&self) -> usize {
        self.expected_directories
            .iter()
            .filter(|path| path.is_dir())
            .count()
    }

    pub fn expected_files_present_count(&self) -> usize {
        self.expected_files
            .iter()
            .filter(|path| path.is_file())
            .count()
    }

    fn is_stale(&self) -> bool {
        !self.source_plan_path.is_file()
            || !self.project_root.is_dir()
            || (self.status == StructuredProjectPlanStatus::Completed
                && self.has_expected_paths()
                && !self.expected_paths_complete())
    }

    fn has_expected_paths(&self) -> bool {
        !self.expected_directories.is_empty() || !self.expected_files.is_empty()
    }

    fn expected_paths_complete(&self) -> bool {
        self.expected_directories.iter().all(|path| path.is_dir())
            && self.expected_files.iter().all(|path| path.is_file())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StructuredProjectPlanStatus {
    Draft,
    #[default]
    #[serde(alias = "Proposed")]
    Verified,
    Executing,
    #[serde(alias = "Executed")]
    Completed,
    Stale,
}

fn trim_to_memory_limit<T>(items: &mut Vec<T>) {
    trim_to_limit(items, PROJECT_MEMORY_LIMIT);
}

fn trim_to_limit<T>(items: &mut Vec<T>, limit: usize) {
    if items.len() > limit {
        let overflow = items.len() - limit;
        items.drain(0..overflow);
    }
}

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
    use std::{fs, path::PathBuf};

    use crate::action::{Action, ActionLifecycleState};
    use crate::event::{
        ActionKind, AssistantMessage, AssistantMessageSource, Event, ProviderFinished,
        ProviderOutput, VerifiedActionResult,
    };

    use super::{
        ActionRecord, PendingActionSelection, ProjectMemory, ProviderMetadata, Session,
        StructuredProjectPlan, StructuredProjectPlanStatus,
    };

    #[test]
    fn new_session_stores_identity_paths_and_empty_state() {
        let session = Session::new("session-1", "/repo", "/repo/crates");

        assert_eq!(session.id, "session-1");
        assert_eq!(session.project_root, PathBuf::from("/repo"));
        assert_eq!(session.cwd, PathBuf::from("/repo/crates"));
        assert!(session.events.is_empty());
        assert!(session.actions.is_empty());
        assert_eq!(session.provider_metadata, None);
        assert_eq!(session.project_memory, ProjectMemory::default());

        let debug = format!("{session:?}");
        assert!(debug.contains("session-1"));
        assert!(debug.contains("project_root"));
    }

    #[test]
    fn structured_plan_runtime_status_tracks_verified_completed_and_stale() {
        let root =
            std::env::temp_dir().join(format!("elgar-session-plan-status-{}", std::process::id()));
        let project = root.join("DemoApp");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();

        let mut plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path.clone(),
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::default(),
            expected_directories: vec![project.join("src")],
            expected_files: vec![
                project.join("src/main.py"),
                project.join("requirements.txt"),
            ],
        };

        assert_eq!(plan.runtime_status(), StructuredProjectPlanStatus::Verified);
        assert_eq!(plan.expected_directories_present_count(), 0);
        assert_eq!(plan.expected_files_present_count(), 0);

        plan.status = StructuredProjectPlanStatus::Executing;
        assert_eq!(
            plan.runtime_status(),
            StructuredProjectPlanStatus::Executing
        );

        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/main.py"), "print('hello')\n").unwrap();
        fs::write(project.join("requirements.txt"), "").unwrap();
        assert_eq!(
            plan.runtime_status(),
            StructuredProjectPlanStatus::Completed
        );
        assert_eq!(plan.expected_directories_present_count(), 1);
        assert_eq!(plan.expected_files_present_count(), 2);

        plan.status = StructuredProjectPlanStatus::Completed;
        fs::remove_file(project.join("requirements.txt")).unwrap();
        assert_eq!(plan.runtime_status(), StructuredProjectPlanStatus::Stale);

        let _ = fs::remove_dir_all(root);
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
