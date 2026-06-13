//! Directory collector data types.

use std::{error::Error, fmt, path::PathBuf};

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
    pub(super) max_rendered_bytes: usize,
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
    pub(super) fn prefix(self) -> &'static str {
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
