use std::path::{Path, PathBuf};

use crate::{
    action::{Action, ActionRequest, FileActionVerification},
    controller_reporting::verified_shell_expected_directories,
    event::VerifiedActionResult,
    session::{Session, VerifiedFolderReference, VerifiedPlanReference},
};

pub(crate) fn record_verified_project_memory(
    session: &mut Session,
    action: &Action,
    result: &VerifiedActionResult,
) {
    let action_id = action.id.clone();
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            let path = verified_directory_path(session, result).unwrap_or_else(|| {
                resolve_session_path(&session.cwd, &create_directory.target_path)
            });
            session.record_verified_folder_reference(VerifiedFolderReference {
                path,
                source_action_id: action_id,
            });
        }
        ActionRequest::CreateFile(create_file)
            if is_plan_path_or_contents(&create_file.target_path, &create_file.contents) =>
        {
            let path = verified_file_write_path(session, result)
                .unwrap_or_else(|| resolve_session_path(&session.cwd, &create_file.target_path));
            record_verified_plan_memory(session, &action_id, path);
        }
        ActionRequest::OverwriteFile(overwrite_file)
            if is_plan_path_or_contents(&overwrite_file.target_path, &overwrite_file.contents) =>
        {
            let path = verified_file_write_path(session, result)
                .unwrap_or_else(|| resolve_session_path(&session.cwd, &overwrite_file.target_path));
            record_verified_plan_memory(session, &action_id, path);
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

fn record_verified_plan_memory(session: &mut Session, action_id: &str, path: PathBuf) {
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

fn verified_directory_path(session: &Session, result: &VerifiedActionResult) -> Option<PathBuf> {
    match result {
        VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
            Some(resolve_session_path(&session.cwd, path))
        }
        _ => None,
    }
}

fn verified_file_write_path(session: &Session, result: &VerifiedActionResult) -> Option<PathBuf> {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            Some(resolve_session_path(&session.cwd, path))
        }
        VerifiedActionResult::File(FileActionVerification::FileCreated { path })
        | VerifiedActionResult::File(FileActionVerification::FileOverwritten { path }) => {
            Some(resolve_session_path(&session.cwd, path))
        }
        _ => None,
    }
}

fn resolve_session_path(base: &Path, target_path: impl AsRef<Path>) -> PathBuf {
    let target_path = target_path.as_ref();
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        base.join(target_path)
    }
}

pub(crate) fn is_plan_path_or_contents(path: &Path, contents: &str) -> bool {
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
        action::{Action, ActionRequest, CreateDirectoryAction, CreateFileAction},
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
    fn records_verified_paths_relative_to_session_cwd() {
        let root = PathBuf::from("/repo");
        let cwd = root.join("playground");
        let mut session = Session::new("session", &root, &cwd);
        let folder_action = Action::proposed(
            "action-folder",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: PathBuf::from("WeatherApp"),
            }),
            "create folder",
        )
        .approve()
        .mark_applied();
        let folder_result = VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
            path: "WeatherApp".to_string(),
        });

        record_verified_project_memory(&mut session, &folder_action, &folder_result);

        assert_eq!(
            session
                .project_memory()
                .latest_verified_folder()
                .expect("folder should be remembered")
                .path,
            PathBuf::from("/repo/playground/WeatherApp")
        );

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("WeatherApp/project-plan.md"),
                contents: "# Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let plan_result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "WeatherApp/project-plan.md".to_string(),
        });

        record_verified_project_memory(&mut session, &plan_action, &plan_result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("plan should be remembered under cwd");
        assert_eq!(
            plan.path,
            PathBuf::from("/repo/playground/WeatherApp/project-plan.md")
        );
        assert_eq!(
            plan.project_root,
            PathBuf::from("/repo/playground/WeatherApp")
        );
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
