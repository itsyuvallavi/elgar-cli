//! Primitive `grep` evidence collection for the harness.
//!
//! This module searches bounded UTF-8 files for text. It skips noisy generated
//! folders and returns path, line number, and a short line excerpt.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use super::{
    noise::is_noisy_directory,
    path::{display_path, resolve_optional_directory_path},
};

const DEFAULT_MAX_DEPTH: usize = 6;
const DEFAULT_MAX_FILE_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_RESULTS: usize = 200;
const DEFAULT_MAX_RENDERED_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepOptions {
    pub max_depth: usize,
    pub max_file_bytes: usize,
    pub max_results: usize,
    pub max_rendered_bytes: usize,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_results: DEFAULT_MAX_RESULTS,
            max_rendered_bytes: DEFAULT_MAX_RENDERED_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepSnapshot {
    pub root: PathBuf,
    pub display_path: String,
    pub query: String,
    pub matches: Vec<GrepMatch>,
    pub omitted: Vec<String>,
    pub truncated: bool,
    max_rendered_bytes: usize,
}

impl GrepSnapshot {
    /// Render bounded text search results for the model.
    pub fn render_for_model(&self) -> String {
        let mut rendered = format!(
            "Primitive grep result selected by Elgar harness.\nRoot: {}\nPath: {}\nQuery: {}\nMatches: {}\n\n",
            self.root.display(),
            self.display_path,
            self.query,
            self.matches.len()
        );

        for item in &self.matches {
            rendered.push_str("- ");
            rendered.push_str(&item.path);
            rendered.push(':');
            rendered.push_str(&item.line_number.to_string());
            rendered.push_str(": ");
            rendered.push_str(&item.line);
            rendered.push('\n');
            if rendered.len() >= self.max_rendered_bytes {
                rendered.push_str("\n[truncated: rendered grep output exceeded byte limit]\n");
                return rendered;
            }
        }

        if !self.omitted.is_empty() {
            rendered.push_str("\nOmitted:\n");
            for omission in &self.omitted {
                rendered.push_str("- ");
                rendered.push_str(omission);
                rendered.push('\n');
            }
        }

        if self.truncated {
            rendered.push_str("\n[truncated: grep output exceeded result or depth limits]\n");
        }

        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrepError {
    EmptyQuery,
    RootUnreadable(String),
    PathNotFound(PathBuf),
    MetadataFailed(String),
    SymlinkRejected,
    NotDirectory,
    ReadFailed(String),
}

impl fmt::Display for GrepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuery => write!(formatter, "grep query is required"),
            Self::RootUnreadable(error) => write!(formatter, "project root unreadable: {error}"),
            Self::PathNotFound(path) => {
                write!(formatter, "path does not exist: {}", path.display())
            }
            Self::MetadataFailed(error) => write!(formatter, "path metadata failed: {error}"),
            Self::SymlinkRejected => write!(formatter, "symlink directories are not allowed"),
            Self::NotDirectory => write!(formatter, "path is not a directory"),
            Self::ReadFailed(error) => write!(formatter, "directory read failed: {error}"),
        }
    }
}

impl Error for GrepError {}

/// Search bounded UTF-8 files under one directory for the query text.
pub fn collect_grep_matches(
    launch_cwd: impl AsRef<Path>,
    requested_path: &str,
    query: &str,
    options: GrepOptions,
) -> Result<GrepSnapshot, GrepError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(GrepError::EmptyQuery);
    }

    let root = launch_cwd
        .as_ref()
        .canonicalize()
        .map_err(|error| GrepError::RootUnreadable(error.to_string()))?;
    let directory = resolve_optional_directory_path(&root, requested_path);
    let metadata = fs::symlink_metadata(&directory).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            GrepError::PathNotFound(directory.clone())
        } else {
            GrepError::MetadataFailed(error.to_string())
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(GrepError::SymlinkRejected);
    }
    if !metadata.is_dir() {
        return Err(GrepError::NotDirectory);
    }

    let mut snapshot = GrepSnapshot {
        root: root.clone(),
        display_path: requested_path.trim().to_string(),
        query: query.to_string(),
        matches: Vec::new(),
        omitted: Vec::new(),
        truncated: false,
        max_rendered_bytes: options.max_rendered_bytes,
    };
    search_directory(&root, &directory, query, 0, &options, &mut snapshot)?;
    Ok(snapshot)
}

fn search_directory(
    root: &Path,
    directory: &Path,
    query: &str,
    depth: usize,
    options: &GrepOptions,
    snapshot: &mut GrepSnapshot,
) -> Result<(), GrepError> {
    if depth > options.max_depth {
        snapshot.truncated = true;
        return Ok(());
    }

    let mut entries = fs::read_dir(directory)
        .map_err(|error| GrepError::ReadFailed(error.to_string()))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if snapshot.matches.len() >= options.max_results {
            snapshot.truncated = true;
            return Ok(());
        }

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                snapshot.omitted.push(format!(
                    "{}: metadata failed: {error}",
                    display_path(root, &path)
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            snapshot
                .omitted
                .push(format!("{}: symlink skipped", display_path(root, &path)));
            continue;
        }
        if metadata.is_dir() {
            let display_path = display_path(root, &path);
            if is_noisy_directory(&entry.file_name().to_string_lossy()) {
                snapshot
                    .omitted
                    .push(format!("{display_path}: noise directory skipped"));
                continue;
            }
            search_directory(root, &path, query, depth + 1, options, snapshot)?;
        } else if metadata.is_file() {
            search_file(root, &path, query, options, snapshot);
        }
    }

    Ok(())
}

fn search_file(
    root: &Path,
    path: &Path,
    query: &str,
    options: &GrepOptions,
    snapshot: &mut GrepSnapshot,
) {
    if snapshot.matches.len() >= options.max_results {
        snapshot.truncated = true;
        return;
    }

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            snapshot.omitted.push(format!(
                "{}: metadata failed: {error}",
                display_path(root, path)
            ));
            return;
        }
    };
    if metadata.len() > options.max_file_bytes as u64 {
        snapshot.omitted.push(format!(
            "{}: file too large for grep",
            display_path(root, path)
        ));
        return;
    }

    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => {
            snapshot.omitted.push(format!(
                "{}: binary or non-UTF-8 file skipped",
                display_path(root, path)
            ));
            return;
        }
    };

    for (index, line) in contents.lines().enumerate() {
        if snapshot.matches.len() >= options.max_results {
            snapshot.truncated = true;
            return;
        }
        if line.contains(query) {
            snapshot.matches.push(GrepMatch {
                path: display_path(root, path),
                line_number: index + 1,
                line: line.trim().chars().take(240).collect(),
            });
        }
    }
}
