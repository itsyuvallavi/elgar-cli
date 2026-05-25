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
        ActionRequest::CreateFile(create_file)
            if is_plan_path_or_contents(&create_file.target_path, &create_file.contents) =>
        {
            record_verified_plan_memory(session, &action_id, &create_file.target_path);
        }
        ActionRequest::OverwriteFile(overwrite_file)
            if is_plan_path_or_contents(&overwrite_file.target_path, &overwrite_file.contents) =>
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
                .filter(|path| is_plan_path(path))
                .cloned()
                .or_else(|| {
                    shell_command
                        .expected_files
                        .iter()
                        .find(|path| is_plan_path(path))
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

fn is_plan_path_or_contents(path: &Path, contents: &str) -> bool {
    is_plan_path(path) || contents_looks_like_plan(contents)
}

fn is_plan_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    (extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt"))
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.to_ascii_lowercase().contains("plan"))
}

fn contents_looks_like_plan(contents: &str) -> bool {
    let lower = contents.to_ascii_lowercase();
    lower.contains("project plan")
        || lower.contains("# plan")
        || (lower.contains("## directory structure") && lower.contains("key files"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        action::{Action, ActionRequest, CreateFileAction},
        event::{FileActionVerification, VerifiedActionResult},
        session::Session,
    };

    use super::*;

    #[test]
    fn records_plan_txt_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/plan.txt"),
                contents: "plan".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/plan.txt".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("plan.txt should be remembered");
        assert_eq!(plan.path, PathBuf::from("/repo/DesktopProject/plan.txt"));
        assert_eq!(plan.project_root, PathBuf::from("/repo/DesktopProject"));
    }

    #[test]
    fn does_not_record_arbitrary_txt_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/notes.txt"),
                contents: "notes".to_string(),
            }),
            "create notes",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/notes.txt".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert!(session.project_memory().latest_verified_plan().is_none());
    }

    #[test]
    fn does_not_record_readme_markdown_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/README.md"),
                contents: "# Demo".to_string(),
            }),
            "create readme",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/README.md".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert!(session.project_memory().latest_verified_plan().is_none());
    }

    #[test]
    fn records_readme_markdown_when_contents_are_a_plan() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/README.md"),
                contents: "# React TypeScript Project Plan\n\n- Create package.json.".to_string(),
            }),
            "create readme plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/README.md".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("README.md plan contents should be remembered");
        assert_eq!(plan.path, PathBuf::from("/repo/DesktopProject/README.md"));
    }
}
