use std::path::{Path, PathBuf};

use crate::{
    action::{Action, ActionRequest},
    controller_reporting::verified_shell_expected_directories,
    event::VerifiedActionResult,
    session::{Session, VerifiedFolderReference, VerifiedPlanReference},
};

pub(crate) fn record_verified_project_memory(
    session: &mut Session,
    action: &Action,
    _result: &VerifiedActionResult,
) {
    let action_id = action.id.clone();
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            session.record_verified_folder_reference(VerifiedFolderReference {
                path: resolve_project_path(&session.project_root, &create_directory.target_path),
                source_action_id: action_id,
            });
        }
        ActionRequest::CreateFile(create_file) if is_markdown_path(&create_file.target_path) => {
            record_verified_plan_memory(session, &action_id, &create_file.target_path);
        }
        ActionRequest::OverwriteFile(overwrite_file)
            if is_markdown_path(&overwrite_file.target_path) =>
        {
            record_verified_plan_memory(session, &action_id, &overwrite_file.target_path);
        }
        ActionRequest::ShellCommand(shell_command) => {
            for path in verified_shell_expected_directories(shell_command) {
                session.record_verified_folder_reference(VerifiedFolderReference {
                    path,
                    source_action_id: action_id.clone(),
                });
            }

            if let Some(path) = shell_command
                .expected_file
                .as_ref()
                .filter(|path| is_markdown_path(path))
                .cloned()
                .or_else(|| {
                    shell_command
                        .expected_files
                        .iter()
                        .find(|path| is_markdown_path(path))
                        .cloned()
                })
            {
                session.record_verified_plan_reference(VerifiedPlanReference {
                    project_root: path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| session.project_root.clone()),
                    path,
                    source_action_id: action_id,
                });
            }
        }
        _ => {}
    }
}

fn record_verified_plan_memory(session: &mut Session, action_id: &str, target_path: &Path) {
    let path = resolve_project_path(&session.project_root, target_path);
    let project_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| session.project_root.clone());
    session.record_verified_plan_reference(VerifiedPlanReference {
        path,
        project_root,
        source_action_id: action_id.to_string(),
    });
}

fn resolve_project_path(project_root: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        project_root.join(target_path)
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}
