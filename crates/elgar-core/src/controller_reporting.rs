use std::path::{Path, PathBuf};

use crate::{
    action::{Action, ActionRequest, FileActionVerification, ShellCommandAction},
    event::VerifiedActionResult,
    session::Session,
};

pub(crate) fn truth_guard_visible_message(session: &Session, message: String) -> String {
    let normalized = message.to_ascii_lowercase();
    if denies_verified_folder_create(&normalized) {
        if let Some(path) = latest_verified_created_directory(session) {
            return format!(
                "Filesystem truth: {} was created and verified.",
                user_display_path(&path)
            );
        }
    }

    if denies_verified_file_create(&normalized) {
        if let Some(path) = latest_verified_created_file(session) {
            return format!(
                "Filesystem truth: {} was created and verified.",
                user_display_path(&path)
            );
        }
    }

    message
}

pub(crate) fn create_directory_proposal_message(target_paths: &[PathBuf]) -> String {
    if target_paths.len() == 1 {
        return format!(
            "I can create {}. Approve to create it.",
            user_display_path(&target_paths[0])
        );
    }

    format!(
        "I can create these directories: {}. Approve to create them.",
        display_user_path_list(target_paths)
    )
}

pub(crate) fn verified_action_success_message(
    session: &Session,
    action: &Action,
    result: &VerifiedActionResult,
) -> String {
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            let path = resolve_project_path(&session.project_root, &create_directory.target_path);
            format!("Created {}.", user_display_path(&path))
        }
        ActionRequest::ShellCommand(shell_command) => {
            let directories = verified_shell_expected_directories(shell_command);
            if directories.len() == 1 {
                return format!("Created {}.", user_display_path(&directories[0]));
            }
            if !directories.is_empty() {
                return format!("Created {}.", display_user_path_list(&directories));
            }
            "Executed approved shell command and recorded the verified result.".to_string()
        }
        _ => verified_file_action_success_message(result),
    }
}

pub(crate) fn verified_shell_expected_directories(
    shell_command: &ShellCommandAction,
) -> Vec<PathBuf> {
    let mut expected_directories = Vec::new();
    if let Some(path) = shell_command.expected_directory.clone() {
        expected_directories.push(path);
    }
    expected_directories.extend(shell_command.expected_directories.iter().cloned());
    dedupe_paths(expected_directories)
}

fn denies_verified_folder_create(normalized: &str) -> bool {
    (normalized.contains("no folder") || normalized.contains("no directory"))
        && (normalized.contains("was created")
            || normalized.contains("were created")
            || normalized.contains("has been created"))
}

fn denies_verified_file_create(normalized: &str) -> bool {
    normalized.contains("no file")
        && (normalized.contains("was created")
            || normalized.contains("were created")
            || normalized.contains("has been created"))
}

fn latest_verified_created_directory(session: &Session) -> Option<PathBuf> {
    session.actions().iter().rev().find_map(|record| {
        let verified = record.verified_result.as_ref()?;
        match verified {
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
                Some(PathBuf::from(path))
            }
            VerifiedActionResult::Shell(shell) => shell
                .verified_effect
                .as_deref()
                .and_then(|effect| {
                    verified_effect_value(effect, "verified directory exists: ")
                        .or_else(|| verified_effect_value(effect, "verified directories exist: "))
                })
                .and_then(first_verified_effect_path),
            _ => None,
        }
    })
}

fn latest_verified_created_file(session: &Session) -> Option<PathBuf> {
    session.actions().iter().rev().find_map(|record| {
        let verified = record.verified_result.as_ref()?;
        match verified {
            VerifiedActionResult::FileWritten { path }
            | VerifiedActionResult::File(FileActionVerification::FileCreated { path }) => {
                Some(PathBuf::from(path))
            }
            VerifiedActionResult::Shell(shell) => shell
                .verified_effect
                .as_deref()
                .and_then(|effect| {
                    verified_effect_value(effect, "verified file exists: ")
                        .or_else(|| verified_effect_value(effect, "verified files exist: "))
                })
                .and_then(first_verified_effect_path),
            _ => None,
        }
    })
}

fn first_verified_effect_path(value: &str) -> Option<PathBuf> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn verified_effect_value<'a>(effect: &'a str, prefix: &str) -> Option<&'a str> {
    effect
        .split("; ")
        .find_map(|part| part.strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn verified_file_action_success_message(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("Wrote {}.", user_display_path(Path::new(path)))
        }
        VerifiedActionResult::File(file) => match file {
            FileActionVerification::FileCreated { path } => {
                format!("Created {}.", user_display_path(Path::new(path)))
            }
            FileActionVerification::FilePatched { path } => {
                format!("Updated {}.", user_display_path(Path::new(path)))
            }
            FileActionVerification::FileOverwritten { path } => {
                format!("Overwrote {}.", user_display_path(Path::new(path)))
            }
            FileActionVerification::FileDeleted { path } => {
                format!("Deleted {}.", user_display_path(Path::new(path)))
            }
            FileActionVerification::FileMoved {
                source_path,
                target_path,
            } => format!(
                "Moved {} to {}.",
                user_display_path(Path::new(source_path)),
                user_display_path(Path::new(target_path))
            ),
            FileActionVerification::DirectoryCreated { path } => {
                format!("Created {}.", user_display_path(Path::new(path)))
            }
        },
        VerifiedActionResult::Shell(_) => {
            "Applied approved action and recorded the verified result.".to_string()
        }
    }
}

fn display_user_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| user_display_path(path))
        .collect::<Vec<_>>()
        .join(", ")
}

fn user_display_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        let desktop = home.join("Desktop");
        if path == desktop {
            return "Desktop".to_string();
        }
        if let Ok(relative) = path.strip_prefix(&desktop) {
            return PathBuf::from("Desktop")
                .join(relative)
                .display()
                .to_string();
        }
    }

    path.display().to_string()
}

fn resolve_project_path(project_root: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        project_root.join(target_path)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::{create_directory_proposal_message, truth_guard_visible_message};
    use crate::{
        action::{Action, ActionRequest, CreateDirectoryAction, FileActionVerification},
        event::VerifiedActionResult,
        session::{ActionRecord, Session},
    };

    #[test]
    fn reporting_formats_create_directory_proposal_without_mutating_session() {
        let message = create_directory_proposal_message(&["alpha".into(), "beta".into()]);

        assert_eq!(
            message,
            "I can create these directories: alpha, beta. Approve to create them."
        );
    }

    #[test]
    fn truth_guard_replaces_false_folder_claim_with_verified_truth() {
        let mut session = Session::new("session-1", ".", ".");
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: "demo".into(),
            }),
            "create directory demo",
        );
        let mut record = ActionRecord::new(action);
        record.verified_result = Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated {
                path: "demo".to_string(),
            },
        ));
        session.push_action(record);

        assert_eq!(
            truth_guard_visible_message(&session, "No folder was created.".to_string()),
            "Filesystem truth: demo was created and verified."
        );
    }
}
