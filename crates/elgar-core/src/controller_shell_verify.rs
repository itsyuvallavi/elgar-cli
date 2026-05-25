use std::path::PathBuf;

use crate::{action::ShellCommandAction, event::VerifiedActionResult};

pub(crate) fn verify_expected_shell_effect(
    action: &ShellCommandAction,
    mut result: VerifiedActionResult,
) -> Result<VerifiedActionResult, String> {
    let mut expected_directories = Vec::new();
    if let Some(expected_directory) = action.expected_directory.as_ref() {
        expected_directories.push(expected_directory.clone());
    }
    expected_directories.extend(action.expected_directories.iter().cloned());
    let expected_directories = dedupe_paths(expected_directories);

    let mut expected_files = Vec::new();
    if let Some(expected_file) = action.expected_file.as_ref() {
        expected_files.push(expected_file.clone());
    }
    expected_files.extend(action.expected_files.iter().cloned());
    let expected_files = dedupe_paths(expected_files);

    if expected_directories.is_empty() && expected_files.is_empty() {
        return Ok(result);
    }

    let missing_directories = expected_directories
        .iter()
        .filter(|expected_directory| !expected_directory.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    if !missing_directories.is_empty() {
        return Err(format!(
            "expected directories were not created: {}",
            display_path_list(&missing_directories)
        ));
    }

    let missing_files = expected_files
        .iter()
        .filter(|expected_file| !expected_file.is_file())
        .cloned()
        .collect::<Vec<_>>();
    if !missing_files.is_empty() {
        return Err(format!(
            "expected files were not created: {}",
            display_path_list(&missing_files)
        ));
    }

    if let VerifiedActionResult::Shell(shell) = &mut result {
        let mut effects = Vec::new();
        if expected_directories.len() == 1 {
            effects.push(format!(
                "verified directory exists: {}",
                expected_directories[0].display()
            ));
        } else if !expected_directories.is_empty() {
            effects.push(format!(
                "verified directories exist: {}",
                display_path_list(&expected_directories)
            ));
        }
        if expected_files.len() == 1 {
            effects.push(format!(
                "verified file exists: {}",
                expected_files[0].display()
            ));
        } else if !expected_files.is_empty() {
            effects.push(format!(
                "verified files exist: {}",
                display_path_list(&expected_files)
            ));
        }
        shell.verified_effect = Some(effects.join("; "));
    }

    Ok(result)
}

fn display_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
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
