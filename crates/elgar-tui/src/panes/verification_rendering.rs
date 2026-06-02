use std::path::{Path, PathBuf};

use elgar_core::event::{FileActionVerification, VerifiedActionResult};

use crate::shell_result::render_shell_execution_summary;

pub(super) fn render_verified_action_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("Wrote {}.", user_display_path(path))
        }
        VerifiedActionResult::File(file) => render_file_verification(file),
        VerifiedActionResult::Shell(shell) => {
            if let Some(effect) = &shell.verified_effect {
                if let Some(message) = render_shell_verified_effect(effect) {
                    return message;
                }
            }
            render_shell_execution_summary(shell)
        }
    }
}

fn render_shell_verified_effect(effect: &str) -> Option<String> {
    if let Some(path) = verified_effect_value(effect, "verified file exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified files exist: ") {
        return Some(format!("Created files: {}.", user_display_path_list(paths)));
    }

    if let Some(path) = verified_effect_value(effect, "verified directory exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified directories exist: ") {
        return Some(format!("Created {}.", user_display_path_list(paths)));
    }

    None
}

fn verified_effect_value<'a>(effect: &'a str, prefix: &str) -> Option<&'a str> {
    effect
        .split("; ")
        .find_map(|part| part.strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn render_file_verification(result: &FileActionVerification) -> String {
    match result {
        FileActionVerification::FileCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
        FileActionVerification::FilePatched { path } => {
            format!("Updated {}.", user_display_path(path))
        }
        FileActionVerification::FileOverwritten { path } => {
            format!("Overwrote {}.", user_display_path(path))
        }
        FileActionVerification::FileDeleted { path } => {
            format!("Deleted {}.", user_display_path(path))
        }
        FileActionVerification::FileMoved {
            source_path,
            target_path,
        } => format!(
            "Moved {} to {}.",
            user_display_path(source_path),
            user_display_path(target_path)
        ),
        FileActionVerification::DirectoryCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
    }
}

pub(super) fn user_display_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
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

fn user_display_path_list(paths: &str) -> String {
    paths
        .split(", ")
        .map(user_display_path)
        .collect::<Vec<_>>()
        .join(", ")
}
