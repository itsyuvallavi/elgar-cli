//! Read-only project file context collection for the harness.
//!
//! This module reads one bounded UTF-8 file selected by the user. Relative
//! paths are resolved from the folder where `elgar` was launched. Absolute
//! paths are allowed. Directories, symlinks, missing files, and binary content
//! are rejected with explicit errors.

use std::{
    error::Error,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
};

const DEFAULT_MAX_FILE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileOptions {
    pub max_bytes: usize,
}

impl Default for ProjectFileOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileSnapshot {
    pub root: PathBuf,
    pub display_path: String,
    pub contents: String,
    pub file_bytes: u64,
    pub rendered_bytes: usize,
    pub truncated: bool,
}

impl ProjectFileSnapshot {
    /// Render file evidence as text that can be sent to the model.
    pub fn render_for_model(&self) -> String {
        format!(
            "Read-only project file selected by Elgar harness.\nRoot: {}\nPath: {}\nBytes: {}\nRendered bytes: {}\nTruncated: {}\n\n```text\n{}\n```",
            self.root.display(),
            self.display_path,
            self.file_bytes,
            self.rendered_bytes,
            self.truncated,
            self.contents
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectFileError {
    EmptyPath,
    RootUnreadable(String),
    FileNotFound(PathBuf),
    MetadataFailed(String),
    SymlinkRejected,
    DirectoryRejected,
    NotFile,
    ReadFailed(String),
    BinaryRejected,
}

impl fmt::Display for ProjectFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(formatter, "file path is required"),
            Self::RootUnreadable(error) => write!(formatter, "project root unreadable: {error}"),
            Self::FileNotFound(path) => {
                write!(formatter, "file does not exist: {}", path.display())
            }
            Self::MetadataFailed(error) => write!(formatter, "file metadata failed: {error}"),
            Self::SymlinkRejected => write!(formatter, "symlink files are not allowed"),
            Self::DirectoryRejected => write!(formatter, "directories cannot be read as files"),
            Self::NotFile => write!(formatter, "path is not a regular file"),
            Self::ReadFailed(error) => write!(formatter, "file read failed: {error}"),
            Self::BinaryRejected => write!(formatter, "binary or non-UTF-8 file rejected"),
        }
    }
}

impl Error for ProjectFileError {}

/// Read one bounded UTF-8 file selected by the user.
pub fn collect_project_file(
    launch_cwd: impl AsRef<Path>,
    requested_path: &str,
    options: ProjectFileOptions,
) -> Result<ProjectFileSnapshot, ProjectFileError> {
    let root = launch_cwd
        .as_ref()
        .canonicalize()
        .map_err(|error| ProjectFileError::RootUnreadable(error.to_string()))?;
    let target = resolve_requested_path(&root, requested_path)?;
    let metadata = fs::symlink_metadata(&target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProjectFileError::FileNotFound(target.clone())
        } else {
            ProjectFileError::MetadataFailed(error.to_string())
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ProjectFileError::SymlinkRejected);
    }
    if metadata.is_dir() {
        return Err(ProjectFileError::DirectoryRejected);
    }
    if !metadata.is_file() {
        return Err(ProjectFileError::NotFile);
    }

    let (contents, rendered_bytes, truncated) = read_bounded_utf8(&target, options.max_bytes)?;

    Ok(ProjectFileSnapshot {
        root,
        display_path: requested_path.trim().to_string(),
        contents,
        file_bytes: metadata.len(),
        rendered_bytes,
        truncated,
    })
}

fn resolve_requested_path(root: &Path, path: &str) -> Result<PathBuf, ProjectFileError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ProjectFileError::EmptyPath);
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(root.join(path))
}

fn read_bounded_utf8(
    path: &Path,
    max_bytes: usize,
) -> Result<(String, usize, bool), ProjectFileError> {
    let file =
        fs::File::open(path).map_err(|error| ProjectFileError::ReadFailed(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProjectFileError::ReadFailed(error.to_string()))?;

    let truncated = bytes.len() > max_bytes;
    if truncated {
        bytes.truncate(max_bytes);
        while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
            bytes.pop();
        }
    }

    let rendered_bytes = bytes.len();
    let contents = String::from_utf8(bytes).map_err(|_error| ProjectFileError::BinaryRejected)?;
    Ok((contents, rendered_bytes, truncated))
}
