//! Path validation for approved filesystem primitives.
//!
//! These helpers are used only after a pending approval has been explicitly
//! approved. They still reject empty paths and symlink targets/parents.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovedPathError {
    EmptyPath,
    RootUnreadable(String),
    ParentMissing,
    SymlinkRejected(PathBuf),
    DirectoryRejected(PathBuf),
    NotFile(PathBuf),
    MetadataFailed(String),
    CreateParentFailed(String),
}

impl fmt::Display for ApprovedPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(formatter, "path is required"),
            Self::RootUnreadable(error) => write!(formatter, "launch folder unreadable: {error}"),
            Self::ParentMissing => write!(formatter, "target path has no parent directory"),
            Self::SymlinkRejected(path) => {
                write!(formatter, "symlink path rejected: {}", path.display())
            }
            Self::DirectoryRejected(path) => {
                write!(formatter, "directory target rejected: {}", path.display())
            }
            Self::NotFile(path) => write!(
                formatter,
                "target is not a regular file: {}",
                path.display()
            ),
            Self::MetadataFailed(error) => write!(formatter, "path metadata failed: {error}"),
            Self::CreateParentFailed(error) => {
                write!(formatter, "parent directory creation failed: {error}")
            }
        }
    }
}

pub(in crate::harness::permissions) fn resolve_write_target(
    launch_cwd: &Path,
    requested_path: &str,
) -> Result<PathBuf, ApprovedPathError> {
    let root = canonical_root(launch_cwd)?;
    let target = resolve_requested_path(&root, requested_path)?;
    reject_existing_symlink_or_directory(&target)?;
    let parent = target.parent().ok_or(ApprovedPathError::ParentMissing)?;
    reject_symlink_ancestors(parent)?;
    fs::create_dir_all(parent)
        .map_err(|error| ApprovedPathError::CreateParentFailed(error.to_string()))?;
    reject_symlink_ancestors(parent)?;
    Ok(target)
}

pub(in crate::harness::permissions) fn resolve_existing_file_target(
    launch_cwd: &Path,
    requested_path: &str,
) -> Result<PathBuf, ApprovedPathError> {
    let root = canonical_root(launch_cwd)?;
    let target = resolve_requested_path(&root, requested_path)?;
    reject_symlink_ancestors(target.parent().ok_or(ApprovedPathError::ParentMissing)?)?;
    let metadata = fs::symlink_metadata(&target)
        .map_err(|error| ApprovedPathError::MetadataFailed(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(ApprovedPathError::SymlinkRejected(target));
    }
    if metadata.is_dir() {
        return Err(ApprovedPathError::DirectoryRejected(target));
    }
    if !metadata.is_file() {
        return Err(ApprovedPathError::NotFile(target));
    }
    Ok(target)
}

fn canonical_root(launch_cwd: &Path) -> Result<PathBuf, ApprovedPathError> {
    launch_cwd
        .canonicalize()
        .map_err(|error| ApprovedPathError::RootUnreadable(error.to_string()))
}

fn resolve_requested_path(root: &Path, path: &str) -> Result<PathBuf, ApprovedPathError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ApprovedPathError::EmptyPath);
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(root.join(path))
    }
}

fn reject_existing_symlink_or_directory(path: &Path) -> Result<(), ApprovedPathError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ApprovedPathError::MetadataFailed(error.to_string())),
    };
    if metadata.file_type().is_symlink() {
        return Err(ApprovedPathError::SymlinkRejected(path.to_path_buf()));
    }
    if metadata.is_dir() {
        return Err(ApprovedPathError::DirectoryRejected(path.to_path_buf()));
    }
    if !metadata.is_file() {
        return Err(ApprovedPathError::NotFile(path.to_path_buf()));
    }
    Ok(())
}

fn reject_symlink_ancestors(path: &Path) -> Result<(), ApprovedPathError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ApprovedPathError::MetadataFailed(error.to_string())),
        };
        if metadata.file_type().is_symlink() {
            return Err(ApprovedPathError::SymlinkRejected(current));
        }
    }
    Ok(())
}
