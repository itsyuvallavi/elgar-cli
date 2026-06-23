//! Read-only directory context collection for the harness.
//!
//! This module summarizes one user-selected directory. It never reads file
//! contents and never sends a full directory dump to the model.

mod render;
mod types;
mod walk;

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub use types::{
    DirectoryEntry, DirectoryEntryKind, DirectoryError, DirectoryOmission, DirectoryOptions,
    DirectorySnapshot,
};
use walk::{collect_entry_samples, count_directory};

/// Collect a bounded summary for one user-selected directory.
pub fn collect_directory_summary(
    launch_cwd: impl AsRef<Path>,
    requested_path: &str,
    options: DirectoryOptions,
) -> Result<DirectorySnapshot, DirectoryError> {
    let root = launch_cwd
        .as_ref()
        .canonicalize()
        .map_err(|error| DirectoryError::RootUnreadable(error.to_string()))?;
    let directory = resolve_requested_path(&root, requested_path)?;
    let metadata = fs::symlink_metadata(&directory).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DirectoryError::PathNotFound(directory.clone())
        } else {
            DirectoryError::MetadataFailed(error.to_string())
        }
    })?;

    if metadata.file_type().is_symlink() {
        return Err(DirectoryError::SymlinkRejected);
    }
    if !metadata.is_dir() {
        return Err(DirectoryError::NotDirectory);
    }

    let mut snapshot = DirectorySnapshot {
        root,
        display_path: requested_path.trim().to_string(),
        total_files: 0,
        total_directories: 0,
        total_bytes: 0,
        entries: Vec::new(),
        omitted: Vec::new(),
        truncated: false,
        count_truncated: false,
        max_rendered_bytes: options.max_rendered_bytes,
    };

    collect_entry_samples(&directory, &directory, 0, &options, &mut snapshot)?;
    count_directory(&directory, &options, &mut snapshot)?;
    Ok(snapshot)
}

fn resolve_requested_path(root: &Path, path: &str) -> Result<PathBuf, DirectoryError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(DirectoryError::EmptyPath);
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(root.join(path))
}
