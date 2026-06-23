use std::path::{Path, PathBuf};

use crate::{
    action::ActionRequest,
    agent_path_utils::{absolute_session_path, normalize_path, path_is_within},
    agent_prompt_context::display_agent_context_path,
    agent_tool_anchors::{structured_plan_expects_child_under, structured_plan_expects_path},
    agent_tool_output::{resolved_outputs_are_shell_only, ResolvedAgentToolOutput},
    agent_turn_router::has_verified_session_state,
    controller_project_memory::is_plan_path_or_contents,
    plan_contract::{PlanContractDraftIssue, PlanContractDraftIssueKind},
    session::{Session, StructuredProjectPlanStatus},
};

pub(crate) fn guard_plan_creation_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    plan_created_this_turn: bool,
    plan_creation_repair_in_progress: bool,
    allow_implementation_after_plan_creation: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if plan_creation_repair_in_progress {
        return outputs
            .into_iter()
            .map(|output| match output {
                ResolvedAgentToolOutput::Guidance(guidance) => {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: guidance.tool_call_id,
                        message: plan_creation_repair_message(session),
                        visible: false,
                    }
                }
                ResolvedAgentToolOutput::Action(action)
                    if is_latest_verified_plan_file_action(session, &action.request) =>
                {
                    ResolvedAgentToolOutput::Action(action)
                }
                ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped non-plan repair action. Update the same verified plan file before execution.".to_string(),
                    visible: false,
                },
                skipped => skipped,
            })
            .collect();
    }

    if has_existing_plan_contract_or_reference(session)
        && !latest_structured_plan_is_completed(session)
        && resolved_outputs_touch_structured_plan(session, &outputs)
    {
        return outputs;
    }

    let plan_roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                plan_creation_root_for_action(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if plan_roots.is_empty() && !plan_created_this_turn && !plan_creation_repair_in_progress {
        if allow_implementation_after_plan_creation {
            if resolved_outputs_touch_structured_plan(session, &outputs) {
                return outputs;
            }
            if plain_create_batch_can_run_as_execute(session, &outputs) {
                return outputs;
            }
            if has_verified_session_state(session) && resolved_outputs_are_shell_only(&outputs) {
                return outputs;
            }
            return outputs
                .into_iter()
                .map(|output| match output {
                    ResolvedAgentToolOutput::Guidance(guidance) => {
                        ResolvedAgentToolOutput::Skipped {
                            tool_call_id: guidance.tool_call_id,
                            message: plan_creation_first_message(),
                            visible: false,
                        }
                    }
                    ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: plan_creation_first_message(),
                        visible: false,
                    },
                    skipped => skipped,
                })
                .collect();
        }
        return outputs;
    }

    let mut allowed_plan_file_used = false;
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if (plan_created_this_turn || plan_creation_repair_in_progress)
                    && !allow_implementation_after_plan_creation =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped implementation tool calls after creating the verified plan. Ask to execute the plan when you want to apply it.".to_string(),
                    visible: true,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if (plan_created_this_turn || plan_creation_repair_in_progress)
            =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action)
                if plan_creation_root_for_action(session, &action.request).is_some() =>
            {
                if !allowed_plan_file_used {
                    allowed_plan_file_used = true;
                    ResolvedAgentToolOutput::Action(action)
                } else {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                        visible: true,
                    }
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if is_plan_parent_setup_action(session, &action.request, &plan_roots) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action)
                if !plan_roots.is_empty() && !plan_created_this_turn =>
            {
                if allow_implementation_after_plan_creation {
                    ResolvedAgentToolOutput::Action(action)
                } else {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                        visible: true,
                    }
                }
            }
            ResolvedAgentToolOutput::Action(action) if allow_implementation_after_plan_creation => {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: action.tool_call_id,
                message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                visible: true,
            },
            other => other,
        })
        .collect()
}

pub(crate) fn resolved_outputs_touch_structured_plan(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return false;
    };

    outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => Some(&action.request),
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .flat_map(plan_guard_paths)
        .any(|path| {
            let path = absolute_session_path(session, path);
            structured_plan_expects_path(plan, &path)
                || structured_plan_expects_child_under(plan, &path)
        })
}

pub(crate) fn plain_create_batch_can_run_as_execute(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    if has_existing_plan_contract_or_reference(session)
        && !latest_structured_plan_is_completed(session)
    {
        return false;
    }

    let mut saw_action = false;
    for output in outputs {
        match output {
            ResolvedAgentToolOutput::Action(action)
                if matches!(
                    action.request,
                    ActionRequest::CreateFile(_)
                        | ActionRequest::CreateDirectory(_)
                        | ActionRequest::OverwriteFile(_)
                ) =>
            {
                saw_action = true;
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => return false,
        }
    }

    saw_action
}

pub(crate) fn latest_structured_plan_is_completed(session: &Session) -> bool {
    session
        .project_memory()
        .latest_structured_plan()
        .is_some_and(|plan| plan.runtime_status() == StructuredProjectPlanStatus::Completed)
}

pub(crate) fn has_existing_plan_contract_or_reference(session: &Session) -> bool {
    session.latest_plan_contract().is_some()
        || session.project_memory().latest_verified_plan().is_some()
        || session.project_memory().latest_structured_plan().is_some()
}

pub(crate) fn prioritize_plan_creation_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    let plan_roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                plan_creation_root_for_action(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if plan_roots.is_empty() {
        return outputs;
    }

    let mut setup = Vec::new();
    let mut plans = Vec::new();
    let mut rest = Vec::new();
    for output in outputs {
        match &output {
            ResolvedAgentToolOutput::Action(action)
                if is_plan_parent_setup_action(session, &action.request, &plan_roots) =>
            {
                setup.push(output);
            }
            ResolvedAgentToolOutput::Action(action)
                if plan_creation_root_for_action(session, &action.request).is_some() =>
            {
                plans.push(output);
            }
            ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. }
            | ResolvedAgentToolOutput::Action(_) => {
                rest.push(output);
            }
        }
    }

    setup.extend(plans);
    setup.extend(rest);
    setup
}

pub(crate) fn latest_plan_contract_needs_repair(session: &Session) -> bool {
    session
        .latest_plan_contract()
        .is_some_and(|contract| !contract.review_draft().is_approvable())
}

pub(crate) fn is_latest_verified_plan_file_action(
    session: &Session,
    request: &ActionRequest,
) -> bool {
    let Some(contract) = session.latest_plan_contract() else {
        return false;
    };
    let target_path = match request {
        ActionRequest::CreateFile(action) => &action.target_path,
        ActionRequest::OverwriteFile(action) => &action.target_path,
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => return false,
    };

    absolute_session_path(session, target_path) == normalize_path(&contract.source_plan_path)
}

pub(crate) fn plan_creation_non_plan_repair_skip_message() -> String {
    "Skipped non-plan repair action. Update the same verified plan file before execution."
        .to_string()
}

pub(crate) fn plan_creation_first_message() -> String {
    "Create the project plan file first, then create implementation files from the verified plan."
        .to_string()
}

pub(crate) fn no_tool_action_repair_message() -> String {
    "This route requires tool actions. Use create_file/create_directory/overwrite_file for filesystem changes, shell_command for command execution, or ask concise guidance if a required target is missing.".to_string()
}

pub(crate) fn plan_creation_repair_message(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "The verified plan draft is not ready. Update the same plan file with a concrete file tree, Verification section, and Acceptance Criteria section before creating implementation files.".to_string();
    };
    let review = contract.review_draft();
    let mut lines = vec![
        "The verified plan draft is not approvable yet.".to_string(),
        "Update the same plan file before creating implementation files.".to_string(),
        "Blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push("The plan file must include a concrete fenced file tree or path list, a `Verification` section with bullet checks, and an `Acceptance Criteria` section with bullet criteria.".to_string());
    lines.push("Do not ask the user whether to rename the project root.".to_string());
    lines.push("Keep the existing project root and choose valid package or module names inside it, for example by using underscores for Python package paths.".to_string());
    lines.push("If verification or acceptance criteria reference a file path, include that path in the plan scope.".to_string());
    lines.join("\n")
}

pub(crate) fn plan_creation_needs_revision_notice(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "The plan needs revision before execution. Review /plan for details.".to_string();
    };
    let review = contract.review_draft();
    let mut lines = vec![
        "The plan needs revision before execution.".to_string(),
        "Blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push("Use /plan to review the current contract details.".to_string());
    lines.join("\n")
}

pub(crate) fn plan_execution_blocked_by_contract_repair_message(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "Cannot execute the plan yet. Update the verified plan before creating implementation files.".to_string();
    };
    let review = contract.review_draft();
    let plan_path = display_agent_context_path(session, &contract.source_plan_path);
    let mut lines = vec![
        "Cannot execute the plan yet.".to_string(),
        "The plan contract has blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push(format!(
        "Update the same verified plan file `{plan_path}` to fix these blockers before creating implementation files."
    ));
    lines.push("Do not create implementation files in this repair step.".to_string());
    lines.push("Do not ask the user whether to rename the project root; keep the existing project root and choose valid package or module names inside it.".to_string());
    lines.join("\n")
}

fn plan_draft_issue_message(issue: &PlanContractDraftIssue) -> String {
    let path = issue
        .path
        .as_ref()
        .map(|path| format!(": {}", path.display()))
        .unwrap_or_default();
    match &issue.kind {
        PlanContractDraftIssueKind::ContractNotDraft { status } => {
            format!("plan contract is not a draft ({status:?})")
        }
        PlanContractDraftIssueKind::MissingSourcePlan => format!("missing source plan{path}"),
        PlanContractDraftIssueKind::MissingProjectRoot => format!("missing project root{path}"),
        PlanContractDraftIssueKind::SourcePlanOutsideProjectRoot => {
            format!("source plan is outside the project root{path}")
        }
        PlanContractDraftIssueKind::EmptyExecutableScope => {
            "no executable expected paths; include a concrete fenced file tree or path list"
                .to_string()
        }
        PlanContractDraftIssueKind::PathOutsideProjectRoot => {
            format!("planned path is outside the project root{path}")
        }
        PlanContractDraftIssueKind::MalformedScopePath => {
            format!("planned path is malformed{path}")
        }
        PlanContractDraftIssueKind::ReferencedPathMissingFromScope => {
            format!("referenced path is missing from the plan scope{path}")
        }
        PlanContractDraftIssueKind::InvalidPythonModuleReference { module } => {
            format!("invalid Python module reference `{module}`")
        }
        PlanContractDraftIssueKind::DuplicateScopePath => {
            format!("duplicate planned path{path}")
        }
        PlanContractDraftIssueKind::MissingVerificationSteps => {
            "missing `Verification` section with bullet checks".to_string()
        }
        PlanContractDraftIssueKind::MissingAcceptanceCriteria => {
            "missing `Acceptance Criteria` section with bullet criteria".to_string()
        }
    }
}

pub(crate) fn plan_creation_root_for_action(
    session: &Session,
    request: &ActionRequest,
) -> Option<PathBuf> {
    let path = match request {
        ActionRequest::CreateFile(action)
            if is_plan_path_or_contents(&action.target_path, &action.contents) =>
        {
            &action.target_path
        }
        ActionRequest::OverwriteFile(action)
            if is_plan_path_or_contents(&action.target_path, &action.contents) =>
        {
            &action.target_path
        }
        _ => return None,
    };

    absolute_session_path(session, path)
        .parent()
        .map(Path::to_path_buf)
}

pub(crate) fn is_plan_parent_setup_action(
    session: &Session,
    request: &ActionRequest,
    plan_roots: &[PathBuf],
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    plan_roots
        .iter()
        .any(|plan_root| path_is_within(plan_root, &target_path))
}

fn plan_guard_paths(request: &ActionRequest) -> Vec<&Path> {
    match request {
        ActionRequest::CreateFile(action) => vec![&action.target_path],
        ActionRequest::CreateDirectory(action) => vec![&action.target_path],
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(action) => vec![&action.target_path],
        ActionRequest::MoveFile(action) => vec![&action.source_path, &action.target_path],
        ActionRequest::ShellCommand(_) => Vec::new(),
    }
}
