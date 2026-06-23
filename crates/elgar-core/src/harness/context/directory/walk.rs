//! Directory traversal for bounded `ls` evidence collection.

use std::{collections::VecDeque, fs, path::Path};

use crate::harness::context::path::display_path;

use super::types::{
    DirectoryEntry, DirectoryEntryKind, DirectoryError, DirectoryOmission, DirectoryOptions,
    DirectorySnapshot,
};

pub(super) fn collect_entry_samples(
    root: &Path,
    directory: &Path,
    depth: usize,
    options: &DirectoryOptions,
    snapshot: &mut DirectorySnapshot,
) -> Result<(), DirectoryError> {
    if depth >= options.max_depth {
        snapshot.truncated = true;
        return Ok(());
    }

    let mut entries = fs::read_dir(directory)
        .map_err(|error| DirectoryError::ReadFailed(error.to_string()))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if snapshot.entries.len() >= options.max_entries {
            snapshot.truncated = true;
            return Ok(());
        }

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                snapshot.omitted.push(DirectoryOmission {
                    display_path: display_path(root, &path),
                    reason: format!("metadata failed: {error}"),
                });
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            snapshot.omitted.push(DirectoryOmission {
                display_path: display_path(root, &path),
                reason: "symlink skipped".to_string(),
            });
            continue;
        }

        if metadata.is_dir() {
            snapshot.entries.push(DirectoryEntry {
                display_path: display_path(root, &path),
                depth,
                kind: DirectoryEntryKind::Directory,
            });
            collect_entry_samples(root, &path, depth + 1, options, snapshot)?;
        } else if metadata.is_file() {
            snapshot.entries.push(DirectoryEntry {
                display_path: display_path(root, &path),
                depth,
                kind: DirectoryEntryKind::File,
            });
        }
    }

    Ok(())
}

pub(super) fn count_directory(
    root: &Path,
    options: &DirectoryOptions,
    snapshot: &mut DirectorySnapshot,
) -> Result<(), DirectoryError> {
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut counted_paths = 0usize;

    while let Some(directory) = queue.pop_front() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| DirectoryError::ReadFailed(error.to_string()))?;

        for entry in entries.filter_map(Result::ok) {
            counted_paths += 1;
            if counted_paths > options.max_counted_paths {
                snapshot.count_truncated = true;
                return Ok(());
            }

            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };

            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                snapshot.total_directories += 1;
                queue.push_back(path);
            } else if metadata.is_file() {
                snapshot.total_files += 1;
                snapshot.total_bytes = snapshot.total_bytes.saturating_add(metadata.len());
            }
        }
    }

    Ok(())
}
