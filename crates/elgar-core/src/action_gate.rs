use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    action::{Action, ActionRequest, ShellCommandAction},
    controller::TurnResult,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::{truth_guard_visible_message, verified_action_success_message},
    controller_shell_verify::verify_expected_shell_effect,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource, Event,
        ShellActionVerification, UserMessage, VerifiedActionResult,
    },
    fs::Filesystem,
    path_resolution::resolve_shell_action_paths_for_session,
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
    session.start_reasoning_trace(command);
    session.record_reasoning_route(lifecycle_route_label(route));
    session.push_event(Event::UserMessage(UserMessage::new(command)));
    handler(session);
    session.finish_trace_turn();
    TurnResult {
        route,
        events: session.events()[start_index..].to_vec(),
    }
}

fn lifecycle_route_label(route: Route) -> &'static str {
    match route {
        Route::ApproveAction => "approve_action",
        Route::RejectAction => "reject_action",
        _ => "action_lifecycle",
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
    session.push_event(Event::ActionRejected(action_event_for_action(&rejected)));
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
        action_event_for_action(&approved).with_approval_source(ApprovalSource::user()),
    ));

    let approved_for_execution = resolve_shell_action_paths_for_session(session, &approved);
    if let ActionRequest::ShellCommand(shell_command) = &approved_for_execution.request {
        session.trace_event(
            "shell_command_start",
            shell_command_action_metadata(&approved_for_execution.id, shell_command),
        );
        match ShellExecutor::execute(shell_command) {
            Ok(result) => {
                if let VerifiedActionResult::Shell(shell) = &result {
                    session.trace_event(
                        "shell_command_finish",
                        shell_command_result_metadata(&approved_for_execution.id, shell),
                    );
                }
                match verify_expected_shell_effect(shell_command, result) {
                    Ok(result) => {
                        let message = verified_action_success_message(
                            session,
                            &approved_for_execution,
                            &result,
                        );
                        session.clear_runtime_block();
                        let record = session
                            .action_mut(index)
                            .expect("approved action index must reference an action record");
                        record.verified_result = Some(result.clone());
                        record.failure_reason = None;
                        record.action = approved_for_execution.mark_applied();
                        record_verified_project_memory(session, &approved_for_execution, &result);
                        session.mark_structured_project_plan_executed(&approved_for_execution.id);
                        session.push_event(Event::ActionApplied(ActionApplied::new(
                            approved_for_execution.id.clone(),
                            approved_for_execution.kind(),
                            result,
                        )));
                        push_action_gate_message(session, message);
                    }
                    Err(reason) => {
                        session.trace_event(
                            "shell_command_verification_failed",
                            shell_command_failure_metadata(
                                &approved_for_execution.id,
                                shell_command,
                                "verification_failed",
                                &reason,
                            ),
                        );
                        let record = session
                            .action_mut(index)
                            .expect("approved action index must reference an action record");
                        record.verified_result = None;
                        record.failure_reason = Some(reason.clone());
                        record.action = approved_for_execution.mark_failed();
                        session
                            .remove_structured_project_plan_for_action(&approved_for_execution.id);
                        session.push_event(Event::ActionFailed(action_failed_for_action(
                            &approved_for_execution,
                            reason,
                        )));
                        push_action_gate_message(
                        session,
                        "Approved shell command ran, but expected filesystem verification failed.",
                    );
                    }
                }
            }
            Err(error) => {
                let reason = error.to_string();
                session.trace_event(
                    "shell_command_failed",
                    shell_command_failure_metadata(
                        &approved_for_execution.id,
                        shell_command,
                        "execution_error",
                        &reason,
                    ),
                );
                let record = session
                    .action_mut(index)
                    .expect("approved action index must reference an action record");
                record.verified_result = None;
                record.failure_reason = Some(reason.clone());
                record.action = approved_for_execution.mark_failed();
                session.remove_structured_project_plan_for_action(&approved_for_execution.id);
                session.push_event(Event::ActionFailed(action_failed_for_action(
                    &approved_for_execution,
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

fn action_event_for_action(action: &Action) -> ActionEvent {
    let mut event = ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
        .with_target(action_target_label(action));
    if let ActionRequest::ShellCommand(shell) = &action.request {
        event = event.with_shell_details(
            shell.cwd.display().to_string(),
            shell.timeout_seconds,
            shell.expected_effect.clone(),
        );
    }
    event
}

fn action_failed_for_action(action: &Action, reason: impl Into<String>) -> ActionFailed {
    let mut failed = ActionFailed::new(action.id.clone(), action.kind(), reason.into());
    failed = failed.with_target(action_target_label(action));
    if let ActionRequest::ShellCommand(shell) = &action.request {
        failed = failed.with_shell_details(
            shell.cwd.display().to_string(),
            shell.timeout_seconds,
            shell.expected_effect.clone(),
        );
    }
    failed
}

fn shell_command_action_metadata(action_id: &str, shell: &ShellCommandAction) -> Value {
    json!({
        "action_id": action_id,
        "action_kind": "ShellCommand",
        "command": &shell.command,
        "command_chars": shell.command.chars().count(),
        "cwd": shell.cwd.display().to_string(),
        "timeout_seconds": shell.timeout_seconds,
        "expected_effect_chars": shell.expected_effect.chars().count(),
        "expected_file": shell.expected_file.as_ref().map(|path| path.display().to_string()),
        "expected_files": shell.expected_files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "expected_directory": shell.expected_directory.as_ref().map(|path| path.display().to_string()),
        "expected_directories": shell.expected_directories.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "stdout_cap_bytes": shell.output_caps.stdout_bytes,
        "stderr_cap_bytes": shell.output_caps.stderr_bytes,
    })
}

fn shell_command_result_metadata(action_id: &str, shell: &ShellActionVerification) -> Value {
    json!({
        "action_id": action_id,
        "action_kind": "ShellCommand",
        "command": &shell.command,
        "command_chars": shell.command.chars().count(),
        "cwd": &shell.cwd,
        "exit_code": shell.exit_code,
        "elapsed_millis": shell.elapsed_millis,
        "timed_out": shell.timed_out,
        "stdout_bytes": shell.stdout.len(),
        "stderr_bytes": shell.stderr.len(),
        "stdout_truncated": shell.stdout_truncated,
        "stderr_truncated": shell.stderr_truncated,
        "stdout_tail": shell_output_tail(&shell.stdout),
        "stderr_tail": shell_output_tail(&shell.stderr),
        "verified_effect_present": shell.verified_effect.is_some(),
    })
}

fn shell_command_failure_metadata(
    action_id: &str,
    shell: &ShellCommandAction,
    category: &str,
    reason: &str,
) -> Value {
    let mut metadata = shell_command_action_metadata(action_id, shell);
    if let Some(object) = metadata.as_object_mut() {
        object.insert("category".to_string(), json!(category));
        object.insert("reason_chars".to_string(), json!(reason.chars().count()));
        object.insert("reason".to_string(), json!(reason));
    }
    metadata
}

fn shell_output_tail(output: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let chars = output.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(MAX_CHARS);
    chars[start..].iter().collect()
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
            session.clear_runtime_block();
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

    use serde_json::Value;

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
    fn action_gate_shell_approval_fails_when_exit_is_nonzero() {
        let root = temp_root("approve-shell-nonzero");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        let shell = ShellCommandAction::new("exit 7", &root);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell),
            "run shell command exit 7",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].verified_result.is_none());
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("shell command exited with status 7")));
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
    fn action_gate_shell_approval_fails_when_command_times_out() {
        std::env::set_var("ELGAR_SESSION_LOG", "on");
        let root = temp_root("approve-shell-timeout");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        let mut shell = ShellCommandAction::new("sleep 1", &root);
        shell.timeout_seconds = 0;
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell),
            "run shell command sleep 1",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].verified_result.is_none());
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("shell command timed out after")));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        let events = session_log_events(&root, "session-1");
        let kinds = session_log_kinds(&events);
        assert!(kinds.contains(&"action_approved".to_string()));
        assert!(kinds.contains(&"shell_command_start".to_string()));
        assert!(kinds.contains(&"shell_command_finish".to_string()));
        assert!(kinds.contains(&"shell_command_verification_failed".to_string()));
        assert!(kinds.contains(&"action_failed".to_string()));

        let start = session_log_event(&events, "shell_command_start");
        assert_eq!(start["metadata"]["command"].as_str(), Some("sleep 1"));
        assert_eq!(start["metadata"]["timeout_seconds"].as_u64(), Some(0));
        assert_eq!(
            start["metadata"]["cwd"].as_str(),
            Some(root.to_str().unwrap())
        );

        let finish = session_log_event(&events, "shell_command_finish");
        assert_eq!(finish["metadata"]["command"].as_str(), Some("sleep 1"));
        assert_eq!(finish["metadata"]["timed_out"].as_bool(), Some(true));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_shell_approval_resolves_relative_cwd_and_expected_paths() {
        let root = temp_root("approve-shell-relative-paths");
        fs::create_dir_all(root.join("work")).unwrap();
        let expected_file = root.join("work/out.txt");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        let mut shell = ShellCommandAction::new("printf ok > out.txt", "work");
        shell.expected_file = Some("out.txt".into());
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell),
            "run shell command printf ok",
        )));

        let result = gate.approve(&mut session);

        assert_eq!(result.route, Route::ApproveAction);
        assert_eq!(fs::read_to_string(&expected_file).unwrap(), "ok");
        let Some(VerifiedActionResult::Shell(shell)) =
            session.actions()[0].verified_result.as_ref()
        else {
            panic!("expected verified shell result");
        };
        assert_eq!(shell.cwd, root.join("work").display().to_string());
        assert_eq!(
            shell.verified_effect.as_deref(),
            Some(format!("verified file exists: {}", expected_file.display()).as_str())
        );
        assert!(result
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

    fn session_log_events(root: &std::path::Path, session_id: &str) -> Vec<Value> {
        let path = crate::local_session_log::session_log_file_path(root, session_id);
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn session_log_kinds(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| event.get("kind").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn session_log_event<'a>(events: &'a [Value], kind: &str) -> &'a Value {
        events
            .iter()
            .find(|event| event.get("kind").and_then(Value::as_str) == Some(kind))
            .unwrap_or_else(|| panic!("missing session log event {kind}"))
    }
}
