use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    action::{Action, ActionLifecycleState, ActionRequest},
    event::VerifiedActionResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filesystem;

impl Filesystem {
    pub fn apply_write_file(
        action: &Action,
        allowed_root: impl AsRef<Path>,
    ) -> Result<VerifiedActionResult, FilesystemError> {
        if action.state != ActionLifecycleState::Approved {
            return Err(FilesystemError::ActionNotApproved {
                state: action.state,
            });
        }

        let write_file = match &action.request {
            ActionRequest::WriteFile(write_file) => write_file,
        };
        let target_path = resolve_allowed_target(&write_file.target_path, allowed_root.as_ref())?;

        fs::write(&target_path, &write_file.contents).map_err(|source| {
            FilesystemError::WriteFailed {
                path: target_path.clone(),
                reason: source.to_string(),
            }
        })?;

        if target_path.exists() {
            Ok(VerifiedActionResult::FileWritten {
                path: target_path.display().to_string(),
            })
        } else {
            Err(FilesystemError::VerificationFailed { path: target_path })
        }
    }
}

fn resolve_allowed_target(
    target_path: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, FilesystemError> {
    if target_path.is_absolute() {
        return Err(FilesystemError::UnsafeTarget {
            path: target_path.to_path_buf(),
            reason: "absolute paths are not allowed".to_string(),
        });
    }

    for component in target_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(FilesystemError::UnsafeTarget {
                    path: target_path.to_path_buf(),
                    reason: "parent directory traversal is not allowed".to_string(),
                });
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(FilesystemError::UnsafeTarget {
                    path: target_path.to_path_buf(),
                    reason: "rooted paths are not allowed".to_string(),
                });
            }
        }
    }

    Ok(allowed_root.join(target_path))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemError {
    ActionNotApproved { state: ActionLifecycleState },
    UnsafeTarget { path: PathBuf, reason: String },
    WriteFailed { path: PathBuf, reason: String },
    VerificationFailed { path: PathBuf },
}

impl fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesystemError::ActionNotApproved { state } => {
                write!(formatter, "action is not approved: {state:?}")
            }
            FilesystemError::UnsafeTarget { path, reason } => {
                write!(
                    formatter,
                    "unsafe write target {}: {reason}",
                    path.display()
                )
            }
            FilesystemError::WriteFailed { path, reason } => {
                write!(formatter, "failed to write {}: {reason}", path.display())
            }
            FilesystemError::VerificationFailed { path } => {
                write!(formatter, "write was not verified at {}", path.display())
            }
        }
    }
}

impl std::error::Error for FilesystemError {}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::{
        action::{Action, ActionLifecycleState},
        event::VerifiedActionResult,
    };

    use super::{Filesystem, FilesystemError};

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("elgar-fs-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn proposed_write_file_does_not_apply() {
        let root = root("proposed");
        let path = root.join("proposed.txt");
        let action =
            Action::proposed_write_file("action-1", "proposed.txt", "contents", "write file");

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::ActionNotApproved {
                state: ActionLifecycleState::Proposed
            })
        );
        assert!(!path.exists());
    }

    #[test]
    fn rejected_write_file_does_not_apply() {
        let root = root("rejected");
        let path = root.join("rejected.txt");
        let action =
            Action::proposed_write_file("action-1", "rejected.txt", "contents", "write file")
                .reject();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::ActionNotApproved {
                state: ActionLifecycleState::Rejected
            })
        );
        assert!(!path.exists());
    }

    #[test]
    fn approved_relative_write_file_writes_inside_allowed_root() {
        let root = root("approved");
        let path = root.join("approved.txt");
        let action =
            Action::proposed_write_file("action-1", "approved.txt", "contents", "write file")
                .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Ok(VerifiedActionResult::FileWritten {
                path: path.display().to_string()
            })
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "contents");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_absolute_write_file_is_rejected_without_writing() {
        let root = root("absolute-root");
        let path =
            std::env::temp_dir().join(format!("elgar-fs-{}-absolute.txt", std::process::id()));
        let _ = fs::remove_file(&path);
        let action =
            Action::proposed_write_file("action-1", path.clone(), "contents", "write file")
                .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::UnsafeTarget {
                path: path.clone(),
                reason: "absolute paths are not allowed".to_string()
            })
        );
        assert!(!path.exists());
        assert!(!root.join(path.file_name().unwrap()).exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_parent_traversal_write_file_is_rejected_without_writing() {
        let root = root("traversal-root");
        let outside = root
            .parent()
            .unwrap()
            .join(format!("elgar-fs-{}-outside.txt", std::process::id()));
        let _ = fs::remove_file(&outside);
        let action = Action::proposed_write_file(
            "action-1",
            format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            "contents",
            "write file",
        )
        .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::UnsafeTarget {
                path: PathBuf::from(format!(
                    "../{}",
                    outside.file_name().unwrap().to_string_lossy()
                )),
                reason: "parent directory traversal is not allowed".to_string()
            })
        );
        assert!(!outside.exists());

        let _ = fs::remove_dir_all(&root);
    }
}
