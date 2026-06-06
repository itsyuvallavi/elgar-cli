use std::path::{Component, Path, PathBuf};

use crate::{
    action::ActionRequest,
    agent_path_utils::{
        absolute_session_path, common_path_prefix, cwd_relative_path, normalize_path,
        path_has_no_meaningful_parent, path_is_within,
    },
    agent_tool_output::ResolvedAgentToolOutput,
    controller_project_memory::is_plan_path_or_contents,
    session::Session,
};

pub(crate) fn anchor_verified_plan_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => {
                ResolvedAgentToolOutput::Guidance(guidance)
            }
            ResolvedAgentToolOutput::Skipped {
                tool_call_id,
                message,
                visible,
            } => ResolvedAgentToolOutput::Skipped {
                tool_call_id,
                message,
                visible,
            },
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_verified_plan_action_request(session, action.request);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
        })
        .collect()
}

pub(crate) fn anchor_prompt_project_root_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    prompt_project_root: Option<&Path>,
) -> Vec<ResolvedAgentToolOutput> {
    let Some(prompt_project_root) = prompt_project_root else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_prompt_project_root_action_request(
                    session,
                    action.request,
                    prompt_project_root,
                );
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn anchor_prompt_project_root_action_request(
    session: &Session,
    request: ActionRequest,
    prompt_project_root: &Path,
) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::CreateDirectory(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::CreateDirectory(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::ShellCommand(mut action) => {
            action.cwd = anchor_prompt_project_root_path(session, &action.cwd, prompt_project_root);
            ActionRequest::ShellCommand(action)
        }
        ActionRequest::DeleteFile(_) | ActionRequest::MoveFile(_) => request,
    }
}

fn anchor_prompt_project_root_path(
    session: &Session,
    path: &Path,
    prompt_project_root: &Path,
) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let current_target = absolute_session_path(session, path);
    if path_is_within(&current_target, prompt_project_root) {
        return cwd_relative_path(session, &current_target);
    }
    if is_plan_path_or_contents(path, "") {
        if let Some(rebased_target) =
            rebase_sibling_project_path(session, path, prompt_project_root)
        {
            return cwd_relative_path(session, &rebased_target);
        }
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        return path.to_path_buf();
    }
    cwd_relative_path(session, &normalize_path(prompt_project_root.join(path)))
}

fn rebase_sibling_project_path(
    session: &Session,
    path: &Path,
    project_root: &Path,
) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }

    let project_parent = project_root.parent()?;
    let current_target = absolute_session_path(session, path);
    if !path_is_within(&current_target, project_parent)
        || path_is_within(&current_target, project_root)
    {
        return None;
    }

    let relative_to_parent = current_target.strip_prefix(project_parent).ok()?;
    let mut components = relative_to_parent.components();
    components.next()?;
    let remainder = components.as_path();
    if remainder.as_os_str().is_empty() {
        return None;
    }

    Some(normalize_path(project_root.join(remainder)))
}

pub(crate) fn anchor_bare_plan_artifacts_to_batch_project_root(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    let Some(project_root) = infer_batch_project_root_for_bare_plan_artifact(session, &outputs)
    else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request =
                    anchor_bare_plan_artifact_request(session, action.request, &project_root);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn infer_batch_project_root_for_bare_plan_artifact(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Option<PathBuf> {
    for request in outputs.iter().filter_map(|output| match output {
        ResolvedAgentToolOutput::Action(action)
            if is_bare_plan_artifact_request(&action.request) =>
        {
            Some(&action.request)
        }
        ResolvedAgentToolOutput::Action(_)
        | ResolvedAgentToolOutput::Guidance(_)
        | ResolvedAgentToolOutput::Skipped { .. } => None,
    }) {
        if let Some(root) = infer_project_root_from_plan_artifact_contents(session, request) {
            return Some(root);
        }
    }

    common_batch_project_root(session, outputs)
}

fn is_bare_plan_artifact_request(request: &ActionRequest) -> bool {
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
        _ => return false,
    };

    !path.is_absolute() && path_has_no_meaningful_parent(path)
}

fn infer_project_root_from_plan_artifact_contents(
    session: &Session,
    request: &ActionRequest,
) -> Option<PathBuf> {
    let contents = match request {
        ActionRequest::CreateFile(action) => &action.contents,
        ActionRequest::OverwriteFile(action) => &action.contents,
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => return None,
    };

    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with('|')
            || trimmed.contains("──")
            || trimmed.chars().any(char::is_whitespace)
            || !trimmed.ends_with('/')
        {
            return None;
        }

        let root = trimmed.trim_end_matches('/');
        let path = Path::new(root);
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) {
            return None;
        }

        let root = absolute_session_path(session, path);
        (root != session.cwd && path_is_within(&root, &session.project_root)).then_some(root)
    })
}

fn common_batch_project_root(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Option<PathBuf> {
    let mut roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if !is_bare_plan_artifact_request(&action.request) =>
            {
                batch_project_root_candidate(session, &action.request)
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();

    let mut common = roots.pop()?;
    for root in roots {
        common = common_path_prefix(&common, &root)?;
    }

    (common != session.cwd && path_is_within(&common, &session.project_root)).then_some(common)
}

fn batch_project_root_candidate(session: &Session, request: &ActionRequest) -> Option<PathBuf> {
    let path = match request {
        ActionRequest::CreateFile(action) => absolute_session_path(session, &action.target_path)
            .parent()
            .map(Path::to_path_buf),
        ActionRequest::OverwriteFile(action) => absolute_session_path(session, &action.target_path)
            .parent()
            .map(Path::to_path_buf),
        ActionRequest::CreateDirectory(action) => {
            Some(absolute_session_path(session, &action.target_path))
        }
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => None,
    }?;

    if path == session.cwd {
        return None;
    }
    path_is_within(&path, &session.project_root).then_some(path)
}

fn anchor_bare_plan_artifact_request(
    session: &Session,
    request: ActionRequest,
    project_root: &Path,
) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action)
            if path_has_no_meaningful_parent(&action.target_path) =>
        {
            action.target_path =
                cwd_relative_path(session, &project_root.join(&action.target_path));
            ActionRequest::CreateFile(action)
        }
        ActionRequest::OverwriteFile(mut action)
            if path_has_no_meaningful_parent(&action.target_path) =>
        {
            action.target_path =
                cwd_relative_path(session, &project_root.join(&action.target_path));
            ActionRequest::OverwriteFile(action)
        }
        other => other,
    }
}

pub(crate) fn anchor_verified_plan_action_request(
    session: &Session,
    request: ActionRequest,
) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::CreateDirectory(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::CreateDirectory(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::DeleteFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::DeleteFile(action)
        }
        ActionRequest::MoveFile(mut action) => {
            action.source_path = anchor_verified_plan_path(session, &action.source_path);
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::MoveFile(action)
        }
        ActionRequest::ShellCommand(_) => request,
    }
}

fn anchor_verified_plan_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let current_target = absolute_session_path(session, path);
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        if path_is_within(&current_target, &plan.project_root) {
            return cwd_relative_path(session, &current_target);
        }
    }
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return path.to_path_buf();
    };
    if path_is_within(&current_target, &plan.project_root) {
        return cwd_relative_path(session, &current_target);
    }

    if let Some(rebased_target) = rebase_sibling_project_path(session, path, &plan.project_root) {
        if structured_plan_expects_path(plan, &rebased_target)
            || structured_plan_expects_child_under(plan, &rebased_target)
        {
            return cwd_relative_path(session, &rebased_target);
        }
    }

    let anchored_target = normalize_path(plan.project_root.join(path));
    if !structured_plan_expects_path(plan, &anchored_target)
        && !structured_plan_expects_child_under(plan, &anchored_target)
    {
        return path.to_path_buf();
    }

    cwd_relative_path(session, &anchored_target)
}

pub(crate) fn structured_plan_expects_path(
    plan: &crate::session::StructuredProjectPlan,
    path: &Path,
) -> bool {
    let path = normalize_path(path);
    plan.expected_files
        .iter()
        .chain(plan.expected_directories.iter())
        .any(|expected| normalize_path(expected) == path)
}

pub(crate) fn structured_plan_expects_child_under(
    plan: &crate::session::StructuredProjectPlan,
    directory: &Path,
) -> bool {
    let directory = normalize_path(directory);
    plan.expected_files
        .iter()
        .chain(plan.expected_directories.iter())
        .any(|expected| {
            let expected = normalize_path(expected);
            expected != directory && path_is_within(&expected, &directory)
        })
}
