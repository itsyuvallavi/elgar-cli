use std::path::{Path, PathBuf};

use crate::{
    action::ActionRequest,
    agent_path_utils::{absolute_session_path, cwd_relative_path, normalize_path, path_is_within},
    agent_plan_creation::plan_creation_root_for_action,
    agent_prompt_context::latest_verified_folder_for_prompt,
    agent_tool_output::ResolvedAgentToolOutput,
    session::Session,
};

pub(crate) fn guard_redundant_directory_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    let file_targets = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                created_file_target_path(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if file_targets.is_empty() {
        return outputs;
    }

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if is_redundant_directory_action(session, &action.request, &file_targets) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped redundant directory creation because a file tool call in the same batch already creates that parent directory.".to_string(),
                    visible: false,
                }
            }
            other => other,
        })
        .collect()
}

pub(crate) fn anchor_verified_folder_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_verified_folder_action_request(session, action.request);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn anchor_verified_folder_action_request(
    session: &Session,
    request: ActionRequest,
) -> ActionRequest {
    if session.project_memory().latest_structured_plan().is_some() {
        return request;
    }
    if plan_creation_root_for_action(session, &request).is_some() {
        return request;
    }

    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path = anchor_verified_folder_create_path(session, &action.target_path);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::DeleteFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::DeleteFile(action)
        }
        ActionRequest::MoveFile(mut action) => {
            let anchored_source =
                anchor_verified_folder_existing_path(session, &action.source_path);
            let source_was_anchored = anchored_source != action.source_path;
            action.source_path = anchored_source;
            if source_was_anchored {
                action.target_path =
                    anchor_path_under_verified_folder(session, &action.target_path)
                        .unwrap_or(action.target_path);
            } else {
                action.target_path =
                    anchor_verified_folder_create_path(session, &action.target_path);
            }
            ActionRequest::MoveFile(action)
        }
        ActionRequest::CreateDirectory(_) | ActionRequest::ShellCommand(_) => request,
    }
}

fn anchor_verified_folder_existing_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() || absolute_session_path(session, path).exists() {
        return path.to_path_buf();
    }

    anchor_path_under_verified_folder(session, path)
        .filter(|candidate| absolute_session_path(session, candidate).exists())
        .unwrap_or_else(|| path.to_path_buf())
}

fn anchor_verified_folder_create_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() || absolute_session_path(session, path).exists() {
        return path.to_path_buf();
    }

    anchor_path_under_verified_folder(session, path)
        .filter(|candidate| {
            absolute_session_path(session, candidate)
                .parent()
                .is_some_and(Path::exists)
        })
        .unwrap_or_else(|| path.to_path_buf())
}

fn anchor_path_under_verified_folder(session: &Session, path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }
    let folder = latest_verified_folder_for_prompt(session)?;
    let current_target = absolute_session_path(session, path);
    if path_is_within(&current_target, &folder.path) {
        return None;
    }
    let anchored_target = normalize_path(folder.path.join(path));
    Some(cwd_relative_path(session, &anchored_target))
}

fn created_file_target_path(session: &Session, request: &ActionRequest) -> Option<PathBuf> {
    match request {
        ActionRequest::CreateFile(action) => {
            Some(absolute_session_path(session, &action.target_path))
        }
        _ => None,
    }
}

fn is_redundant_directory_action(
    session: &Session,
    request: &ActionRequest,
    file_targets: &[PathBuf],
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let directory = absolute_session_path(session, &action.target_path);
    file_targets
        .iter()
        .any(|file| path_is_within(file, &directory))
}
