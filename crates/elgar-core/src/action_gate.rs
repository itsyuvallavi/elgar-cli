use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    action::{Action, ActionRequest},
    controller::TurnResult,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::{truth_guard_visible_message, verified_action_success_message},
    controller_shell_verify::verify_expected_shell_effect,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource, Event,
        UserMessage,
    },
    fs::Filesystem,
    policy::ApprovalSource,
    provider::ProviderStub,
    router::Route,
    session::{PendingActionSelection, Session},
    shell::ShellExecutor,
};

/// Narrow approval/rejection gate used after the model/runtime proposes work.
///
/// Normal user text must enter Elgar through `AgentRuntime`. This type exists
/// only for explicit action lifecycle commands such as `/approve` and `/reject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionGate<P = ProviderStub> {
    #[serde(skip)]
    _provider: PhantomData<P>,
}

impl<P> ActionGate<P> {
    pub fn new(_provider: P) -> Self {
        Self {
            _provider: PhantomData,
        }
    }

    pub fn approve(&self, session: &mut Session) -> TurnResult {
        run_lifecycle_turn(
            session,
            "/approve",
            Route::ApproveAction,
            handle_approve_action,
        )
    }

    pub fn reject(&self, session: &mut Session) -> TurnResult {
        run_lifecycle_turn(
            session,
            "/reject",
            Route::RejectAction,
            handle_reject_action,
        )
    }
}

impl Default for ActionGate<ProviderStub> {
    fn default() -> Self {
        Self::new(ProviderStub::default())
    }
}

fn run_lifecycle_turn(
    session: &mut Session,
    command: &'static str,
    route: Route,
    handler: fn(&mut Session),
) -> TurnResult {
    let start_index = session.events().len();
    session.push_event(Event::UserMessage(UserMessage::new(command)));
    handler(session);
    TurnResult {
        route,
        events: session.events()[start_index..].to_vec(),
    }
}

fn handle_reject_action(session: &mut Session) {
    let index = match session.pending_action_selection() {
        PendingActionSelection::Single(index) => index,
        PendingActionSelection::None => {
            push_action_gate_message(session, "No proposed action is waiting for rejection.");
            return;
        }
        PendingActionSelection::Ambiguous => {
            push_ambiguous_pending_action_message(session);
            return;
        }
    };

    let rejected = session.actions()[index].action.reject();
    session.remove_structured_project_plan_for_action(&rejected.id);
    let record = session
        .action_mut(index)
        .expect("latest proposed action index must reference an action record");
    record.action = rejected.clone();
    session.push_event(Event::ActionRejected(
        ActionEvent::new(
            rejected.id.clone(),
            rejected.kind(),
            rejected.summary.clone(),
        )
        .with_target(action_target_label(&rejected)),
    ));
    push_action_gate_message(session, "Rejected action. No filesystem change was made.");
}

fn handle_approve_action(session: &mut Session) {
    let index = match session.pending_action_selection() {
        PendingActionSelection::Single(index) => index,
        PendingActionSelection::None => {
            push_action_gate_message(session, "No proposed action is waiting for approval.");
            return;
        }
        PendingActionSelection::Ambiguous => {
            push_ambiguous_pending_action_message(session);
            return;
        }
    };

    let approved = session.actions()[index].action.approve();
    let record = session
        .action_mut(index)
        .expect("latest proposed action index must reference an action record");
    record.action = approved.clone();
    session.push_event(Event::ActionApproved(
        ActionEvent::new(
            approved.id.clone(),
            approved.kind(),
            approved.summary.clone(),
        )
        .with_target(action_target_label(&approved))
        .with_approval_source(ApprovalSource::user()),
    ));

    if let ActionRequest::ShellCommand(shell_command) = &approved.request {
        match ShellExecutor::execute(shell_command) {
            Ok(result) => match verify_expected_shell_effect(shell_command, result) {
                Ok(result) => {
                    let message = verified_action_success_message(session, &approved, &result);
                    let record = session
                        .action_mut(index)
                        .expect("approved action index must reference an action record");
                    record.verified_result = Some(result.clone());
                    record.failure_reason = None;
                    record.action = approved.mark_applied();
                    record_verified_project_memory(session, &approved, &result);
                    session.mark_structured_project_plan_executed(&approved.id);
                    session.push_event(Event::ActionApplied(ActionApplied::new(
                        approved.id.clone(),
                        approved.kind(),
                        result,
                    )));
                    push_action_gate_message(session, message);
                }
                Err(reason) => {
                    let record = session
                        .action_mut(index)
                        .expect("approved action index must reference an action record");
                    record.verified_result = None;
                    record.failure_reason = Some(reason.clone());
                    record.action = approved.mark_failed();
                    session.remove_structured_project_plan_for_action(&approved.id);
                    session.push_event(Event::ActionFailed(ActionFailed::new(
                        approved.id.clone(),
                        approved.kind(),
                        reason,
                    )));
                    push_action_gate_message(
                        session,
                        "Approved shell command ran, but expected filesystem verification failed.",
                    );
                }
            },
            Err(error) => {
                let reason = error.to_string();
                let record = session
                    .action_mut(index)
                    .expect("approved action index must reference an action record");
                record.verified_result = None;
                record.failure_reason = Some(reason.clone());
                record.action = approved.mark_failed();
                session.remove_structured_project_plan_for_action(&approved.id);
                session.push_event(Event::ActionFailed(ActionFailed::new(
                    approved.id.clone(),
                    approved.kind(),
                    reason,
                )));
                push_action_gate_message(
                    session,
                    "Approved shell command failed before a shell result could be recorded.",
                );
            }
        }
        return;
    }

    let allowed_root = policy_allowed_root_for_action(session, &approved);
    apply_approved_file_action_at_index(
        session,
        index,
        &approved,
        &allowed_root,
        "Approved file action failed. No verified filesystem result was recorded.",
    );
}

fn apply_approved_file_action_at_index(
    session: &mut Session,
    index: usize,
    approved: &Action,
    allowed_root: &Path,
    failure_message: &'static str,
) {
    match Filesystem::apply_file_action(approved, allowed_root) {
        Ok(result) => {
            let message = verified_action_success_message(session, approved, &result);
            let record = session
                .action_mut(index)
                .expect("approved action index must reference an action record");
            record.verified_result = Some(result.clone());
            record.failure_reason = None;
            record.action = approved.mark_applied();
            record_verified_project_memory(session, approved, &result);
            session.push_event(Event::ActionApplied(ActionApplied::new(
                approved.id.clone(),
                approved.kind(),
                result,
            )));
            push_action_gate_message(session, message);
        }
        Err(error) => {
            let reason = error.to_string();
            let record = session
                .action_mut(index)
                .expect("approved action index must reference an action record");
            record.verified_result = None;
            record.failure_reason = Some(reason.clone());
            record.action = approved.mark_failed();
            session.push_event(Event::ActionFailed(ActionFailed::new(
                approved.id.clone(),
                approved.kind(),
                reason,
            )));
            push_action_gate_message(session, failure_message);
        }
    }
}

fn push_action_gate_message(session: &mut Session, message: impl Into<String>) {
    let message = truth_guard_visible_message(session, message.into());
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
}

fn push_ambiguous_pending_action_message(session: &mut Session) {
    push_action_gate_message(
        session,
        "Multiple proposed actions are waiting. Elgar will not approve, reject, or create another action until this session is repaired.",
    );
}

fn policy_allowed_root_for_action(session: &Session, action: &Action) -> PathBuf {
    let Some(target_path) = action_filesystem_target(action) else {
        return session.project_root.clone();
    };

    if !target_path.is_absolute() {
        return session.cwd.clone();
    }

    if let Some(desktop) = home_dir().map(|home| home.join("Desktop")) {
        if target_path.starts_with(&desktop) {
            return desktop;
        }
    }

    if matches!(
        action.request,
        ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_)
    ) {
        if let Some(home) = home_dir() {
            if target_path.starts_with(&home) {
                return home;
            }
        }
    }

    if target_path.starts_with(&session.project_root) {
        return session.project_root.clone();
    }

    session.project_root.clone()
}

fn action_filesystem_target(action: &Action) -> Option<&Path> {
    match &action.request {
        ActionRequest::CreateFile(create_file) => Some(&create_file.target_path),
        ActionRequest::CreateDirectory(create_directory) => Some(&create_directory.target_path),
        ActionRequest::PatchFile(patch_file) => Some(&patch_file.target_path),
        ActionRequest::OverwriteFile(overwrite_file) => Some(&overwrite_file.target_path),
        ActionRequest::DeleteFile(delete_file) => Some(&delete_file.target_path),
        ActionRequest::MoveFile(move_file) => Some(&move_file.target_path),
        ActionRequest::ShellCommand(_) => None,
    }
}

fn action_target_label(action: &Action) -> String {
    match &action.request {
        ActionRequest::CreateFile(create_file) => create_file.target_path.display().to_string(),
        request => request.approval_target(),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        action::{
            Action, ActionRequest, CreateDirectoryAction, OverwriteFileAction, ShellCommandAction,
        },
        event::{Event, FileActionVerification, UserMessage, VerifiedActionResult},
        router::Route,
        session::{ActionRecord, Session},
    };

    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-action-gate-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn action_gate_applies_explicit_approval_only() {
        let root = temp_root("approve");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: "demo".into(),
            }),
            "create demo",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert!(matches!(
            result.events.first(),
            Some(Event::UserMessage(UserMessage { content })) if content == "/approve"
        ));
        assert!(root.join("demo").is_dir());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_approval_runs_shell_and_records_verified_expected_effect() {
        let root = temp_root("approve-shell");
        let expected_directory = root.join("shell-created");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        let mut shell = ShellCommandAction::new("mkdir shell-created", &root);
        shell.expected_directory = Some(expected_directory.clone());
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell),
            "run shell command mkdir shell-created",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert!(expected_directory.is_dir());
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Applied
        );
        let expected_effect = format!(
            "verified directory exists: {}",
            expected_directory.display()
        );
        assert!(session.actions()[0]
            .verified_result
            .as_ref()
            .is_some_and(|verified| matches!(
                verified,
                VerifiedActionResult::Shell(shell)
                    if shell.verified_effect.as_deref() == Some(expected_effect.as_str())
            )));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_shell_approval_fails_when_expected_effect_is_missing() {
        let root = temp_root("approve-shell-missing-effect");
        let missing_file = root.join("missing.txt");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        let mut shell = ShellCommandAction::new("printf done", &root);
        shell.expected_file = Some(missing_file.clone());
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell),
            "run shell command printf done",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert!(!missing_file.exists());
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].verified_result.is_none());
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("expected files were not created")));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_approval_applies_relative_file_action_under_session_cwd() {
        let root = temp_root("approve-relative-cwd");
        let cwd = root.join("playground");
        fs::create_dir_all(cwd.join("demo")).unwrap();
        fs::write(cwd.join("demo/PLAN.md"), "# Old\n").unwrap();
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &cwd);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::OverwriteFile(OverwriteFileAction {
                target_path: "demo/PLAN.md".into(),
                contents: "# New\n".to_string(),
            }),
            "overwrite plan",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert_eq!(
            fs::read_to_string(cwd.join("demo/PLAN.md")).unwrap(),
            "# New\n"
        );
        assert!(!root.join("demo/PLAN.md").exists());
        assert!(session.actions()[0]
            .verified_result
            .as_ref()
            .is_some_and(|verified| matches!(
                verified,
                VerifiedActionResult::File(FileActionVerification::FileOverwritten { path })
                    if path == &cwd.join("demo/PLAN.md").display().to_string()
            )));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_rejects_explicit_pending_action() {
        let root = temp_root("reject");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: "demo".into(),
            }),
            "create demo",
        )));

        let result = gate.reject(&mut session);

        assert_eq!(result.route, Route::RejectAction);
        assert!(matches!(
            result.events.first(),
            Some(Event::UserMessage(UserMessage { content })) if content == "/reject"
        ));
        assert!(!root.join("demo").exists());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionRejected(_))));

        let _ = fs::remove_dir_all(root);
    }
}
