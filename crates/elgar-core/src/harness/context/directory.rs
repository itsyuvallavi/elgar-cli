//! Read-only directory context collection for the harness.
//!
//! This module summarizes one user-selected directory. It never reads file
//! contents and never sends a full directory dump to the model.

use std::{
    collections::VecDeque,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use super::path::display_path;

const DEFAULT_MAX_DEPTH: usize = 2;
const DEFAULT_MAX_ENTRIES: usize = 200;
const DEFAULT_MAX_COUNTED_PATHS: usize = 50_000;
const DEFAULT_MAX_RENDERED_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryOptions {
    pub max_depth: usize,
    pub max_entries: usize,
    pub max_counted_paths: usize,
    pub max_rendered_bytes: usize,
}

impl Default for DirectoryOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_counted_paths: DEFAULT_MAX_COUNTED_PATHS,
            max_rendered_bytes: DEFAULT_MAX_RENDERED_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySnapshot {
    pub root: PathBuf,
    pub display_path: String,
    pub total_files: usize,
    pub total_directories: usize,
    pub total_bytes: u64,
    pub entries: Vec<DirectoryEntry>,
    pub omitted: Vec<DirectoryOmission>,
    pub truncated: bool,
    pub count_truncated: bool,
    max_rendered_bytes: usize,
}

impl DirectorySnapshot {
    /// Render bounded directory evidence for the model.
    pub fn render_for_model(&self) -> String {
        let mut rendered = format!(
            "Read-only directory summary selected by Elgar harness.\nRoot: {}\nPath: {}\nFiles counted: {}\nDirectories counted: {}\nTotal bytes: {}\nCount truncated: {}\nNote: file contents were not read.\n\nEntries:\n",
            self.root.display(),
            self.display_path,
            self.total_files,
            self.total_directories,
            self.total_bytes,
            self.count_truncated
        );

        for entry in &self.entries {
            rendered.push_str(&"  ".repeat(entry.depth));
            rendered.push_str(entry.kind.prefix());
            rendered.push_str(&entry.display_path);
            rendered.push('\n');

            if rendered.len() >= self.max_rendered_bytes {
                rendered.push_str("\n[truncated: rendered directory exceeded byte limit]\n");
                return rendered;
            }
        }

        if !self.omitted.is_empty() {
            rendered.push_str("\nOmitted:\n");
            for omission in &self.omitted {
                rendered.push_str("- ");
                rendered.push_str(&omission.display_path);
                rendered.push_str(": ");
                rendered.push_str(&omission.reason);
                rendered.push('\n');
            }
        }

        if self.truncated {
            rendered.push_str("\n[truncated: directory entries exceeded entry or depth limits]\n");
        }

        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub display_path: String,
    pub depth: usize,
    pub kind: DirectoryEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryEntryKind {
    Directory,
    File,
}

impl DirectoryEntryKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Directory => "[dir] ",
            Self::File => "[file] ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryOmission {
    pub display_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryError {
    EmptyPath,
    RootUnreadable(String),
    PathNotFound(PathBuf),
    MetadataFailed(String),
    SymlinkRejected,
    NotDirectory,
    ReadFailed(String),
}

impl fmt::Display for DirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(formatter, "directory path is required"),
            Self::RootUnreadable(error) => write!(formatter, "project root unreadable: {error}"),
            Self::PathNotFound(path) => {
                write!(formatter, "directory does not exist: {}", path.display())
            }
            Self::MetadataFailed(error) => write!(formatter, "directory metadata failed: {error}"),
            Self::SymlinkRejected => write!(formatter, "symlink directories are not allowed"),
            Self::NotDirectory => write!(formatter, "path is not a directory"),
            Self::ReadFailed(error) => write!(formatter, "directory read failed: {error}"),
        }
    }
}

impl Error for DirectoryError {}

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

fn collect_entry_samples(
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

fn count_directory(
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
