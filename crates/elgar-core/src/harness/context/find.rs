//! Primitive `find` evidence collection for the harness.
//!
//! This module finds bounded file and directory paths by name. It does not read
//! file contents and skips noisy generated/cache/dependency folders.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

const DEFAULT_MAX_DEPTH: usize = 6;
const DEFAULT_MAX_RESULTS: usize = 200;
const DEFAULT_MAX_RENDERED_BYTES: usize = 16 * 1024;
const NOISY_DIRECTORIES: [&str; 7] = [
    ".git",
    ".elgar",
    ".next",
    "target",
    "node_modules",
    "dist",
    "build",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindOptions {
    pub max_depth: usize,
    pub max_results: usize,
    pub max_rendered_bytes: usize,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_results: DEFAULT_MAX_RESULTS,
            max_rendered_bytes: DEFAULT_MAX_RENDERED_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindSnapshot {
    pub root: PathBuf,
    pub display_path: String,
    pub pattern: String,
    pub matches: Vec<String>,
    pub omitted: Vec<String>,
    pub truncated: bool,
    max_rendered_bytes: usize,
}

impl FindSnapshot {
    /// Render bounded path matches for the model.
    pub fn render_for_model(&self) -> String {
        let mut rendered = format!(
            "Primitive find result selected by Elgar harness.\nRoot: {}\nPath: {}\nPattern: {}\nMatches: {}\nNote: file contents were not read.\n\n",
            self.root.display(),
            self.display_path,
            self.pattern,
            self.matches.len()
        );

        for path in &self.matches {
            rendered.push_str("- ");
            rendered.push_str(path);
            rendered.push('\n');
            if rendered.len() >= self.max_rendered_bytes {
                rendered.push_str("\n[truncated: rendered find output exceeded byte limit]\n");
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
            rendered.push_str("\n[truncated: find output exceeded result or depth limits]\n");
        }

        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindError {
    EmptyPattern,
    RootUnreadable(String),
    PathNotFound(PathBuf),
    MetadataFailed(String),
    SymlinkRejected,
    NotDirectory,
    ReadFailed(String),
}

impl fmt::Display for FindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => write!(formatter, "find pattern is required"),
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

impl Error for FindError {}

/// Find file and directory paths whose display path contains the pattern.
pub fn collect_find_matches(
    launch_cwd: impl AsRef<Path>,
    requested_path: &str,
    pattern: &str,
    options: FindOptions,
) -> Result<FindSnapshot, FindError> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(FindError::EmptyPattern);
    }

    let root = launch_cwd
        .as_ref()
        .canonicalize()
        .map_err(|error| FindError::RootUnreadable(error.to_string()))?;
    let directory = resolve_requested_path(&root, requested_path);
    let metadata = fs::symlink_metadata(&directory).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            FindError::PathNotFound(directory.clone())
        } else {
            FindError::MetadataFailed(error.to_string())
        }
    })?;

    if metadata.file_type().is_symlink() {
        return Err(FindError::SymlinkRejected);
    }
    if !metadata.is_dir() {
        return Err(FindError::NotDirectory);
    }

    let mut snapshot = FindSnapshot {
        root: root.clone(),
        display_path: requested_path.trim().to_string(),
        pattern: pattern.to_string(),
        matches: Vec::new(),
        omitted: Vec::new(),
        truncated: false,
        max_rendered_bytes: options.max_rendered_bytes,
    };
    collect_directory(&root, &directory, pattern, 0, &options, &mut snapshot)?;
    Ok(snapshot)
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    pattern: &str,
    depth: usize,
    options: &FindOptions,
    snapshot: &mut FindSnapshot,
) -> Result<(), FindError> {
    if depth > options.max_depth {
        snapshot.truncated = true;
        return Ok(());
    }

    let mut entries = fs::read_dir(directory)
        .map_err(|error| FindError::ReadFailed(error.to_string()))?
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

        let display_path = display_path(root, &path);
        if display_path.contains(pattern) {
            snapshot.matches.push(display_path.clone());
        }

        if metadata.is_dir() {
            if is_noisy_directory(&entry.file_name().to_string_lossy()) {
                snapshot
                    .omitted
                    .push(format!("{display_path}: noise directory skipped"));
                continue;
            }
            collect_directory(root, &path, pattern, depth + 1, options, snapshot)?;
        }
    }

    Ok(())
}

fn resolve_requested_path(root: &Path, path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return root.to_path_buf();
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn is_noisy_directory(name: &str) -> bool {
    NOISY_DIRECTORIES.contains(&name)
}
