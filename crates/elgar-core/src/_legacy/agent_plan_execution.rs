use std::path::{Path, PathBuf};

use crate::{
    action::ActionRequest,
    agent_path_utils::{absolute_session_path, normalize_path, path_is_within},
    agent_plan_creation::plan_creation_root_for_action,
    agent_prompt_context::display_agent_context_path,
    agent_tool_anchors::structured_plan_expects_path,
    agent_tool_output::ResolvedAgentToolOutput,
    session::{Session, StructuredProjectPlan, VerifiedPlanReference},
};

pub(crate) fn guard_plan_execution_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    plan_execution_in_progress: bool,
    allow_shell_commands: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if !plan_execution_in_progress {
        return outputs;
    }

    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: guidance.tool_call_id,
                message: plan_execution_continue_message(session),
                visible: false,
            },
            ResolvedAgentToolOutput::Action(action)
                if is_unexpected_plan_execution_directory(session, plan, &action.request) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped directory creation outside the verified plan; file tools create parent directories when needed.".to_string(),
                    visible: false,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if is_existing_plan_execution_directory(session, plan, &action.request) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped directory creation because the verified plan directory already exists.".to_string(),
                    visible: false,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if allow_shell_commands
                    && matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => {
                if let Some(message) =
                    nonconstructive_plan_execution_skip_message(session, plan, &action.request)
                {
                    let visible = is_off_plan_file_creation_attempt(session, plan, &action.request);
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message,
                        visible,
                    }
                } else {
                    ResolvedAgentToolOutput::Action(action)
                }
            }
            other => other,
        })
        .collect()
}

pub(crate) fn resolved_outputs_complete_missing_plan_paths(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return false;
    };

    let create_files = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => match &action.request {
                ActionRequest::CreateFile(file) => {
                    Some(absolute_session_path(session, &file.target_path))
                }
                ActionRequest::OverwriteFile(file) => {
                    Some(absolute_session_path(session, &file.target_path))
                }
                ActionRequest::CreateDirectory(_)
                | ActionRequest::PatchFile(_)
                | ActionRequest::DeleteFile(_)
                | ActionRequest::MoveFile(_)
                | ActionRequest::ShellCommand(_) => None,
            },
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    let create_directories = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => match &action.request {
                ActionRequest::CreateDirectory(directory) => {
                    Some(absolute_session_path(session, &directory.target_path))
                }
                ActionRequest::CreateFile(_)
                | ActionRequest::OverwriteFile(_)
                | ActionRequest::PatchFile(_)
                | ActionRequest::DeleteFile(_)
                | ActionRequest::MoveFile(_)
                | ActionRequest::ShellCommand(_) => None,
            },
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();

    let files_satisfied = plan.expected_files.iter().all(|expected| {
        expected.is_file()
            || create_files
                .iter()
                .any(|created| normalize_path(created) == *expected)
    });
    let directories_satisfied = plan.expected_directories.iter().all(|expected| {
        expected.is_dir()
            || create_directories
                .iter()
                .any(|created| normalize_path(created) == *expected)
            || create_files
                .iter()
                .any(|created| path_is_within(created, expected))
    });

    files_satisfied && directories_satisfied
}

pub(crate) fn preflight_verified_plan_tool_outputs(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Result<(), String> {
    let Some(plan) = session.project_memory().latest_verified_plan() else {
        return Ok(());
    };

    for target_path in outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => Some(action),
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .flat_map(|action| plan_preflight_paths(session, &action.request))
    {
        let target_path = absolute_session_path(session, target_path);
        if !path_is_within(&target_path, &plan.project_root) {
            return Err(plan_preflight_outside_root_message(
                session,
                plan,
                &target_path,
            ));
        }
    }

    Ok(())
}

pub(crate) fn should_preflight_verified_plan_tool_outputs(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
    plan_execution_batch: bool,
    plan_execution_intent: bool,
) -> bool {
    if plan_execution_batch || plan_execution_intent {
        return true;
    }

    session.project_memory().latest_verified_plan().is_some()
        && session.project_memory().latest_structured_plan().is_none()
        && outputs.iter().any(|output| {
            matches!(
                output,
                ResolvedAgentToolOutput::Action(_) | ResolvedAgentToolOutput::Guidance(_)
            )
        })
}

fn plan_preflight_paths<'a>(session: &Session, request: &'a ActionRequest) -> Vec<&'a Path> {
    if plan_creation_root_for_action(session, request).is_some() {
        return Vec::new();
    }

    match request {
        ActionRequest::CreateFile(action) => vec![&action.target_path],
        ActionRequest::CreateDirectory(_) => Vec::new(),
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(action) => vec![&action.target_path],
        ActionRequest::MoveFile(action) => vec![&action.source_path, &action.target_path],
        ActionRequest::ShellCommand(_) => Vec::new(),
    }
}

fn plan_preflight_outside_root_message(
    session: &Session,
    plan: &VerifiedPlanReference,
    target_path: &Path,
) -> String {
    format!(
        "The verified plan is rooted at {}, but the tool call targets {} outside that project. No filesystem action was applied.",
        display_agent_context_path(session, &plan.project_root),
        display_agent_context_path(session, target_path)
    )
}

pub(crate) fn missing_expected_plan_paths_message(session: &Session) -> Option<String> {
    let missing_directories = missing_expected_plan_directories(session);
    let missing_files = missing_expected_plan_files(session);
    if missing_directories.is_empty() && missing_files.is_empty() {
        return None;
    }

    let mut lines = vec!["The verified plan is not complete.".to_string()];
    if !missing_directories.is_empty() {
        lines.push("Missing expected directories:".to_string());
        lines.extend(
            missing_directories
                .iter()
                .map(|path| format!("- {}", display_agent_context_path(session, path))),
        );
    }
    if !missing_files.is_empty() {
        lines.push("Missing expected files:".to_string());
        lines.extend(
            missing_files
                .iter()
                .map(|path| format!("- {}", display_agent_context_path(session, path))),
        );
    }
    lines.push("Use create_files for multiple missing expected paths when possible; otherwise use create_directory for missing expected directories and create_file for missing expected files under the verified plan root. Do not ask whether to create expected paths.".to_string());
    lines.push(
        "When multiple expected paths are missing, call the needed file and directory tools in one assistant response when possible."
            .to_string(),
    );
    Some(lines.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MissingPlanPathCounts {
    directories: usize,
    files: usize,
}

impl MissingPlanPathCounts {
    pub(crate) fn total(self) -> usize {
        self.directories + self.files
    }
}

pub(crate) fn missing_expected_plan_path_counts(session: &Session) -> MissingPlanPathCounts {
    MissingPlanPathCounts {
        directories: missing_expected_plan_directories(session).len(),
        files: missing_expected_plan_files(session).len(),
    }
}

pub(crate) fn plan_execution_no_progress_message(session: &Session) -> Option<String> {
    missing_expected_plan_paths_message(session).map(|message| {
        format!(
            "{message}\nStopped because the last tool response did not create any remaining expected plan paths."
        )
    })
}

pub(crate) fn plan_execution_incomplete_after_partial_batch_message(message: String) -> String {
    format!(
        "{message}\nStopped after creating verified plan paths because this batch did not complete the verified plan. No further model repair request was sent."
    )
}

pub(crate) fn plan_execution_repair_message_or_mark_complete(
    session: &mut Session,
) -> Option<String> {
    let message = missing_expected_plan_paths_message(session);
    if message.is_none() {
        session.mark_latest_structured_project_plan_completed();
    }
    message
}

fn missing_expected_plan_directories(session: &Session) -> Vec<PathBuf> {
    session
        .project_memory()
        .latest_structured_plan()
        .map(|plan| {
            plan.expected_directories
                .iter()
                .filter(|path| !path.is_dir())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn missing_expected_plan_files(session: &Session) -> Vec<PathBuf> {
    session
        .project_memory()
        .latest_structured_plan()
        .map(|plan| {
            plan.expected_files
                .iter()
                .filter(|path| !path.is_file())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn plan_execution_continue_message(session: &Session) -> String {
    missing_expected_plan_paths_message(session).unwrap_or_else(|| {
        "The verified plan already defines concrete expected paths; continue under the verified plan root without asking for clarification.".to_string()
    })
}

fn is_unexpected_plan_execution_directory(
    session: &Session,
    plan: &StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    !structured_plan_expects_path(plan, &target_path)
}

fn is_existing_plan_execution_directory(
    session: &Session,
    plan: &StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    structured_plan_expects_path(plan, &target_path) && target_path.is_dir()
}

fn nonconstructive_plan_execution_skip_message(
    session: &Session,
    plan: &StructuredProjectPlan,
    request: &ActionRequest,
) -> Option<String> {
    match request {
        ActionRequest::CreateFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            if !structured_plan_expects_path(plan, &target_path) {
                return Some(off_plan_file_creation_skip_message(session, &target_path));
            }
            target_path.is_file().then(|| {
                "Skipped tool call because the expected file already exists in the verified plan."
                    .to_string()
            })
        }
        ActionRequest::OverwriteFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            if !structured_plan_expects_path(plan, &target_path) {
                return Some(off_plan_file_creation_skip_message(session, &target_path));
            }
            target_path.is_file().then(|| {
                "Skipped tool call because the expected file already exists in the verified plan."
                    .to_string()
            })
        }
        ActionRequest::CreateDirectory(_) => None,
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_) => Some(
            "Skipped tool call because it does not create a missing expected path from the verified plan."
                .to_string(),
        ),
        ActionRequest::ShellCommand(_) => Some(
            "Skipped shell command during verified plan execution; verification commands are recorded in the plan and should be run separately unless the plan explicitly includes a generated script or output path."
                .to_string(),
        ),
    }
}

fn is_off_plan_file_creation_attempt(
    session: &Session,
    plan: &StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    match request {
        ActionRequest::CreateFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path)
        }
        ActionRequest::OverwriteFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path)
        }
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => false,
    }
}

fn off_plan_file_creation_skip_message(session: &Session, target_path: &Path) -> String {
    format!(
        "Skipped off-plan file `{}` because it is not listed in the verified plan. Verification commands can stay in the plan's Verification section; create a script file only when that file is explicitly included in the plan scope.",
        display_agent_context_path(session, target_path)
    )
}
