use std::{fs, path::Path};

use serde_json::json;

use crate::{
    action::{Action, ActionRequest, CreateFileAction, OverwriteFileAction},
    agent_path_utils::{absolute_session_path, path_is_within},
    agent_tool_output::ResolvedAgentToolOutput,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::verified_action_success_message,
    controller_shell_verify::verify_expected_shell_effect,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource, Event,
        VerifiedActionResult,
    },
    fs::Filesystem,
    model_runtime::ValidatedModelToolAction,
    path_resolution::{allowed_root_for_action, resolve_shell_action_paths_for_session},
    policy::{PermissionPolicyMode, PolicyDecision},
    session::{ActionRecord, PendingActionSelection, Session},
    shell::ShellExecutor,
    shell_allowlist::is_read_only_shell_command,
};

pub(crate) fn review_required_action_to_propose<'a>(
    session: &Session,
    outputs: &'a [ResolvedAgentToolOutput],
    policy_mode: PermissionPolicyMode,
) -> Option<&'a ValidatedModelToolAction> {
    let reviewed_actions = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if action_requires_review(session, policy_mode, action) =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    reviewed_actions
        .iter()
        .copied()
        .find(|action| !matches!(action.request, ActionRequest::CreateDirectory(_)))
        .or_else(|| reviewed_actions.first().copied())
}

fn action_requires_review(
    session: &Session,
    policy_mode: PermissionPolicyMode,
    action: &ValidatedModelToolAction,
) -> bool {
    let proposed = Action::proposed(
        "policy-preview",
        action.request.clone(),
        action.summary.clone(),
    );
    policy_decision_for_agent_action(session, policy_mode, &proposed).user_approval_required
}

pub(crate) fn apply_agent_action_with_policy(
    session: &mut Session,
    request: ActionRequest,
    summary: String,
    policy_mode: PermissionPolicyMode,
) -> String {
    let request = match reconcile_create_file_target(session, request) {
        CreateFileReconciliation::Request(request) => request,
        CreateFileReconciliation::AlreadySatisfied(message) => {
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                message.clone(),
                AssistantMessageSource::Controller,
            )));
            return message;
        }
    };
    let proposed = Action::proposed(next_action_id(session), request, summary);
    let policy_decision = policy_decision_for_agent_action(session, policy_mode, &proposed);
    session.trace_event(
        "policy_decision",
        json!({
            "action_id": &proposed.id,
            "action_kind": format!("{:?}", proposed.kind()),
            "mode": policy_decision.mode.as_str(),
            "kind": format!("{:?}", policy_decision.kind),
            "user_approval_required": policy_decision.user_approval_required,
            "filesystem_verification_required": policy_decision.filesystem_verification_required,
            "reason_chars": policy_decision.reason.chars().count(),
        }),
    );

    if policy_decision.user_approval_required {
        return propose_agent_action_for_review(session, proposed, policy_decision);
    }

    let action = proposed.approve();
    let approval_source = policy_decision.approval_source.clone();
    let index = session.actions().len();
    let mut record = ActionRecord::new(action.clone());
    record.policy_decision = Some(policy_decision);
    session.push_action(record);

    let mut approved_event = action_event_for_action(&action);
    if let Some(source) = approval_source {
        approved_event = approved_event.with_approval_source(source);
    }
    session.push_event(Event::ActionApproved(approved_event));

    let execution_action = resolve_shell_action_paths_for_session(session, &action);
    let result: Result<VerifiedActionResult, String> = match &execution_action.request {
        ActionRequest::ShellCommand(shell) => ShellExecutor::execute(shell)
            .map_err(|error| error.to_string())
            .and_then(|result| verify_expected_shell_effect(shell, result)),
        _ => Filesystem::apply_file_action(&action, allowed_root_for_action(session, &action))
            .map_err(|error| error.to_string()),
    };

    match result {
        Ok(result) => record_agent_action_success(session, index, &execution_action, result),
        Err(reason) => {
            let record = session
                .action_mut(index)
                .expect("agent action index must reference an action record");
            record.verified_result = None;
            record.failure_reason = Some(reason.clone());
            record.action = execution_action.mark_failed();
            session.push_event(Event::ActionFailed(action_failed_for_action(
                &execution_action,
                reason.clone(),
            )));
            format!("Tool failed: {reason}")
        }
    }
}

enum CreateFileReconciliation {
    Request(ActionRequest),
    AlreadySatisfied(String),
}

fn reconcile_create_file_target(
    session: &Session,
    request: ActionRequest,
) -> CreateFileReconciliation {
    let create_file = match request {
        ActionRequest::CreateFile(create_file) => create_file,
        ActionRequest::OverwriteFile(overwrite_file) => {
            let target_path =
                resolved_target_path_for_existing_check(session, &overwrite_file.target_path);
            if target_path.is_file() {
                return CreateFileReconciliation::Request(ActionRequest::OverwriteFile(
                    overwrite_file,
                ));
            }
            CreateFileAction {
                target_path: overwrite_file.target_path,
                contents: overwrite_file.contents,
            }
        }
        _ => return CreateFileReconciliation::Request(request),
    };

    let target_path = resolved_target_path_for_existing_check(session, &create_file.target_path);
    if !target_path.is_file() {
        return CreateFileReconciliation::Request(ActionRequest::CreateFile(create_file));
    }

    match fs::read_to_string(&target_path) {
        Ok(existing_contents) if existing_contents == create_file.contents => {
            CreateFileReconciliation::AlreadySatisfied(format!(
                "{} already exists with the requested content.",
                target_path.display()
            ))
        }
        Ok(_) => {
            CreateFileReconciliation::Request(ActionRequest::OverwriteFile(OverwriteFileAction {
                target_path: create_file.target_path,
                contents: create_file.contents,
            }))
        }
        Err(_) => CreateFileReconciliation::Request(ActionRequest::CreateFile(create_file)),
    }
}

pub(crate) fn resolved_target_path_for_existing_check(
    session: &Session,
    target_path: &Path,
) -> std::path::PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        session.cwd.join(target_path)
    }
}

fn propose_agent_action_for_review(
    session: &mut Session,
    action: Action,
    policy_decision: PolicyDecision,
) -> String {
    match session.pending_action_selection() {
        PendingActionSelection::None => {}
        PendingActionSelection::Single(_) => {
            return "A proposed action is already waiting. Ask the user to approve or reject it before proposing another action.".to_string();
        }
        PendingActionSelection::Ambiguous => {
            return "Multiple proposed actions are already waiting. Ask the user to approve or reject pending work before proposing another action.".to_string();
        }
    }

    let target = action.request.approval_target();
    let mut record = ActionRecord::new(action.clone());
    record.policy_decision = Some(policy_decision);
    session.push_event(Event::ActionProposed(
        action_event_for_action(&action).with_target(target.clone()),
    ));
    session.push_action(record);

    format!(
        "Proposed {:?} for review at {target}. Wait for the user to approve or reject before treating it as done.",
        action.kind()
    )
}

fn action_event_for_action(action: &Action) -> ActionEvent {
    let mut event = ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
        .with_target(action.request.approval_target());
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
    let mut failed = ActionFailed::new(action.id.clone(), action.kind(), reason.into())
        .with_target(action.request.approval_target());
    if let ActionRequest::ShellCommand(shell) = &action.request {
        failed = failed.with_shell_details(
            shell.cwd.display().to_string(),
            shell.timeout_seconds,
            shell.expected_effect.clone(),
        );
    }
    failed
}

fn policy_decision_for_agent_action(
    session: &Session,
    mode: PermissionPolicyMode,
    action: &Action,
) -> PolicyDecision {
    if let ActionRequest::ShellCommand(shell) = &action.request {
        if is_read_only_shell_command(shell) {
            return PolicyDecision::allow_apply(
                mode,
                "policy allowlist permits read-only shell inspection commands",
            );
        }
    }

    match (mode, &action.request) {
        (PermissionPolicyMode::FullAccess, _) => PolicyDecision::allow_apply(
            mode,
            "full_access policy validated and allowed the model tool call",
        ),
        (
            PermissionPolicyMode::AutoCreateReviewModify,
            ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "auto_create_review_modify allows validated safe create actions",
        ),
        (
            PermissionPolicyMode::WorkspaceWriteWithReview,
            ActionRequest::CreateFile(_)
            | ActionRequest::CreateDirectory(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_),
        ) if action_targets_are_inside_workspace(session, action) => PolicyDecision::allow_apply(
            mode,
            "workspace_write_with_review allows validated workspace write actions",
        ),
        (
            PermissionPolicyMode::WorkspaceWriteWithReview,
            ActionRequest::CreateFile(_)
            | ActionRequest::CreateDirectory(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_),
        ) => PolicyDecision::require_review(
            mode,
            "workspace_write_with_review gates file writes outside the current workspace",
        ),
        (PermissionPolicyMode::AutoCreateReviewModify, _) => PolicyDecision::require_review(
            mode,
            "auto_create_review_modify gates edits, deletes, moves, and shell commands",
        ),
        (PermissionPolicyMode::WorkspaceWriteWithReview, _) => PolicyDecision::require_review(
            mode,
            "workspace_write_with_review gates deletes, moves, and shell commands",
        ),
        (PermissionPolicyMode::ReviewAll, _) => {
            PolicyDecision::require_review(mode, "review_all requires user approval")
        }
    }
}

fn action_targets_are_inside_workspace(session: &Session, action: &Action) -> bool {
    let targets = workspace_write_targets(&action.request);
    !targets.is_empty()
        && targets
            .into_iter()
            .all(|target| path_is_within(&absolute_session_path(session, target), &session.cwd))
}

fn workspace_write_targets(request: &ActionRequest) -> Vec<&Path> {
    match request {
        ActionRequest::CreateFile(action) => vec![&action.target_path],
        ActionRequest::CreateDirectory(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => Vec::new(),
    }
}

fn record_agent_action_success(
    session: &mut Session,
    index: usize,
    action: &Action,
    result: VerifiedActionResult,
) -> String {
    let message = verified_action_success_message(session, action, &result);
    session.clear_runtime_block();
    let record = session
        .action_mut(index)
        .expect("agent action index must reference an action record");
    record.verified_result = Some(result.clone());
    record.failure_reason = None;
    record.action = action.clone().mark_applied();
    record_verified_project_memory(session, action, &result);
    session.push_event(Event::ActionApplied(ActionApplied::new(
        action.id.clone(),
        action.kind(),
        result,
    )));
    message
}

fn next_action_id(session: &Session) -> String {
    format!("action-{}", session.actions().len() + 1)
}
