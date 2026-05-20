use std::{
    fmt, fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    action::{Action, ActionLifecycleState, ActionRequest, FileActionVerification},
    event::VerifiedActionResult,
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filesystem;

impl Filesystem {
    pub fn apply_write_file(
        action: &Action,
        allowed_root: impl AsRef<Path>,
    ) -> Result<VerifiedActionResult, FilesystemError> {
        Self::apply_file_action(action, allowed_root)
    }

    pub fn apply_file_action(
        action: &Action,
        allowed_root: impl AsRef<Path>,
    ) -> Result<VerifiedActionResult, FilesystemError> {
        if action.state != ActionLifecycleState::Approved {
            return Err(FilesystemError::ActionNotApproved {
                state: action.state,
            });
        }

        match &action.request {
            ActionRequest::CreateFile(create_file) => {
                apply_create_file(create_file, allowed_root.as_ref())
            }
            ActionRequest::PatchFile(patch_file) => {
                apply_patch_file(patch_file, allowed_root.as_ref())
            }
            ActionRequest::OverwriteFile(overwrite_file) => {
                apply_overwrite_file(overwrite_file, allowed_root.as_ref())
            }
            _ => Err(FilesystemError::UnsupportedAction {
                kind: action.kind(),
            }),
        }
    }
}

fn apply_create_file(
    create_file: &crate::action::CreateFileAction,
    allowed_root: &Path,
) -> Result<VerifiedActionResult, FilesystemError> {
    let target_path = resolve_allowed_target(&create_file.target_path, allowed_root)?;

    create_new_synced_file(&target_path, &create_file.contents)?;
    verify_file_contents(&target_path, &create_file.contents)?;

    Ok(VerifiedActionResult::FileWritten {
        path: target_path.display().to_string(),
    })
}

fn apply_patch_file(
    patch_file: &crate::action::PatchFileAction,
    allowed_root: &Path,
) -> Result<VerifiedActionResult, FilesystemError> {
    let target_path = resolve_existing_target(&patch_file.target_path, allowed_root)?;
    if patch_file.find.is_empty() {
        return Err(FilesystemError::PatchPatternMissing {
            path: target_path,
            pattern: patch_file.find.clone(),
        });
    }

    let original =
        fs::read_to_string(&target_path).map_err(|source| FilesystemError::WriteFailed {
            path: target_path.clone(),
            reason: source.to_string(),
        })?;
    if !original.contains(&patch_file.find) {
        return Err(FilesystemError::PatchPatternMissing {
            path: target_path,
            pattern: patch_file.find.clone(),
        });
    }

    let expected = original.replacen(&patch_file.find, &patch_file.replace, 1);
    atomic_write_file(&target_path, &expected, AtomicWriteMode::ReplaceExisting)?;
    verify_file_contents(&target_path, &expected)?;

    Ok(VerifiedActionResult::File(
        FileActionVerification::FilePatched {
            path: target_path.display().to_string(),
        },
    ))
}

fn apply_overwrite_file(
    overwrite_file: &crate::action::OverwriteFileAction,
    allowed_root: &Path,
) -> Result<VerifiedActionResult, FilesystemError> {
    let target_path = resolve_existing_target(&overwrite_file.target_path, allowed_root)?;

    atomic_write_file(
        &target_path,
        &overwrite_file.contents,
        AtomicWriteMode::ReplaceExisting,
    )?;
    verify_file_contents(&target_path, &overwrite_file.contents)?;

    Ok(VerifiedActionResult::File(
        FileActionVerification::FileOverwritten {
            path: target_path.display().to_string(),
        },
    ))
}

fn resolve_existing_target(
    target_path: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, FilesystemError> {
    let resolved_target = resolve_allowed_target(target_path, allowed_root)?;
    if !resolved_target.exists() {
        return Err(FilesystemError::TargetMissing {
            path: resolved_target,
        });
    }
    Ok(resolved_target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomicWriteMode {
    ReplaceExisting,
}

fn atomic_write_file(
    path: &Path,
    contents: &str,
    mode: AtomicWriteMode,
) -> Result<(), FilesystemError> {
    match mode {
        AtomicWriteMode::ReplaceExisting if !target_exists(path)? => {
            return Err(FilesystemError::TargetMissing {
                path: path.to_path_buf(),
            });
        }
        _ => {}
    }

    let parent = path.parent().ok_or_else(|| FilesystemError::WriteFailed {
        path: path.to_path_buf(),
        reason: "target parent could not be determined".to_string(),
    })?;
    let permissions = match mode {
        AtomicWriteMode::ReplaceExisting => Some(
            fs::metadata(path)
                .map_err(|source| FilesystemError::WriteFailed {
                    path: path.to_path_buf(),
                    reason: source.to_string(),
                })?
                .permissions(),
        ),
    };

    let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    for attempt in 0..100 {
        let temp_path = atomic_temp_path(parent, path, nonce, attempt);
        match write_synced_temp_file(&temp_path, contents, permissions.clone()) {
            Ok(()) => {
                let result = rename_temp_file(&temp_path, path);

                if result.is_err() {
                    cleanup_temp_file(&temp_path);
                }
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                cleanup_temp_file(&temp_path);
                return Err(FilesystemError::WriteFailed {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        }
    }

    Err(FilesystemError::WriteFailed {
        path: path.to_path_buf(),
        reason: "could not create atomic temporary file".to_string(),
    })
}

fn create_new_synced_file(path: &Path, contents: &str) -> Result<(), FilesystemError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                FilesystemError::TargetAlreadyExists {
                    path: path.to_path_buf(),
                }
            } else {
                FilesystemError::WriteFailed {
                    path: path.to_path_buf(),
                    reason: source.to_string(),
                }
            }
        })?;
    file.write_all(contents.as_bytes())
        .map_err(|source| FilesystemError::WriteFailed {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })?;
    file.flush()
        .map_err(|source| FilesystemError::WriteFailed {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })?;
    file.sync_all()
        .map_err(|source| FilesystemError::WriteFailed {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })
}

fn write_synced_temp_file(
    temp_path: &Path,
    contents: &str,
    permissions: Option<fs::Permissions>,
) -> Result<(), std::io::Error> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn atomic_temp_path(parent: &Path, target_path: &Path, nonce: u64, attempt: u32) -> PathBuf {
    let target_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("target");
    parent.join(format!(
        ".elgar-atomic-{}-{nonce}-{attempt}-{target_name}.tmp",
        std::process::id()
    ))
}

fn target_exists(path: &Path) -> Result<bool, FilesystemError> {
    path.try_exists()
        .map_err(|source| FilesystemError::WriteFailed {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })
}

fn cleanup_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

fn verify_file_contents(path: &Path, expected: &str) -> Result<(), FilesystemError> {
    let verified_contents =
        fs::read_to_string(path).map_err(|source| FilesystemError::WriteFailed {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })?;

    if verified_contents == expected {
        Ok(())
    } else {
        Err(FilesystemError::VerificationFailed {
            path: path.to_path_buf(),
            reason: "file contents did not match expected contents".to_string(),
        })
    }
}

fn rename_temp_file(temp_path: &Path, path: &Path) -> Result<(), FilesystemError> {
    fs::rename(temp_path, path).map_err(|source| FilesystemError::WriteFailed {
        path: path.to_path_buf(),
        reason: source.to_string(),
    })
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

    let canonical_root =
        allowed_root
            .canonicalize()
            .map_err(|source| FilesystemError::UnsafeRoot {
                path: allowed_root.to_path_buf(),
                reason: source.to_string(),
            })?;
    let resolved_target = allowed_root.join(target_path);
    let parent = resolved_target
        .parent()
        .ok_or_else(|| FilesystemError::UnsafeTarget {
            path: target_path.to_path_buf(),
            reason: "target parent could not be determined".to_string(),
        })?;
    let canonical_parent =
        parent
            .canonicalize()
            .map_err(|source| FilesystemError::WriteFailed {
                path: resolved_target.clone(),
                reason: source.to_string(),
            })?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err(FilesystemError::UnsafeTarget {
            path: target_path.to_path_buf(),
            reason: "target parent resolves outside the allowed root".to_string(),
        });
    }

    if resolved_target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(FilesystemError::UnsafeTarget {
            path: target_path.to_path_buf(),
            reason: "target file symlinks are not allowed".to_string(),
        });
    }

    Ok(resolved_target)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemError {
    ActionNotApproved { state: ActionLifecycleState },
    UnsupportedAction { kind: crate::action::ActionKind },
    UnsafeRoot { path: PathBuf, reason: String },
    UnsafeTarget { path: PathBuf, reason: String },
    TargetAlreadyExists { path: PathBuf },
    TargetMissing { path: PathBuf },
    PatchPatternMissing { path: PathBuf, pattern: String },
    WriteFailed { path: PathBuf, reason: String },
    VerificationFailed { path: PathBuf, reason: String },
}

impl fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesystemError::ActionNotApproved { state } => {
                write!(formatter, "action is not approved: {state:?}")
            }
            FilesystemError::UnsupportedAction { kind } => {
                write!(formatter, "unsupported filesystem action: {kind:?}")
            }
            FilesystemError::UnsafeRoot { path, reason } => {
                write!(formatter, "unsafe write root {}: {reason}", path.display())
            }
            FilesystemError::UnsafeTarget { path, reason } => {
                write!(
                    formatter,
                    "unsafe write target {}: {reason}",
                    path.display()
                )
            }
            FilesystemError::TargetAlreadyExists { path } => {
                write!(formatter, "write target already exists: {}", path.display())
            }
            FilesystemError::TargetMissing { path } => {
                write!(
                    formatter,
                    "file action target does not exist: {}",
                    path.display()
                )
            }
            FilesystemError::PatchPatternMissing { path, pattern } => {
                write!(
                    formatter,
                    "patch pattern {:?} was not found in {}",
                    pattern,
                    path.display()
                )
            }
            FilesystemError::WriteFailed { path, reason } => {
                write!(formatter, "failed to write {}: {reason}", path.display())
            }
            FilesystemError::VerificationFailed { path, reason } => {
                write!(
                    formatter,
                    "write was not verified at {}: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for FilesystemError {}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use crate::{
        action::{Action, ActionLifecycleState, FileActionVerification},
        event::VerifiedActionResult,
    };

    use super::{Filesystem, FilesystemError};

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("elgar-fs-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn atomic_temp_files(root: &Path) -> Vec<PathBuf> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".elgar-atomic-"))
            })
            .collect()
    }

    fn assert_no_atomic_temp_files(root: &Path) {
        assert_eq!(atomic_temp_files(root), Vec::<PathBuf>::new());
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
        assert_no_atomic_temp_files(&root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_write_file_fails_when_target_already_exists_without_overwriting() {
        let root = root("existing-target");
        let path = root.join("existing.txt");
        fs::write(&path, "original").unwrap();
        let action =
            Action::proposed_write_file("action-1", "existing.txt", "new contents", "write file")
                .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::TargetAlreadyExists { path: path.clone() })
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert_no_atomic_temp_files(&root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn proposed_patch_file_does_not_apply() {
        let root = root("proposed-patch");
        let path = root.join("notes.txt");
        fs::write(&path, "old contents").unwrap();
        let action =
            Action::proposed_patch_file("action-1", "notes.txt", "old", "new", "edit file");

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::ActionNotApproved {
                state: ActionLifecycleState::Proposed
            })
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old contents");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_patch_file_updates_existing_file_and_verifies_contents() {
        let root = root("approved-patch");
        let path = root.join("notes.txt");
        fs::write(&path, "old contents").unwrap();
        let action =
            Action::proposed_patch_file("action-1", "notes.txt", "old", "new", "edit file")
                .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Ok(VerifiedActionResult::File(
                FileActionVerification::FilePatched {
                    path: path.display().to_string()
                }
            ))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "new contents");
        assert_no_atomic_temp_files(&root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_patch_file_fails_when_pattern_is_missing() {
        let root = root("missing-patch-pattern");
        let path = root.join("notes.txt");
        fs::write(&path, "original").unwrap();
        let action =
            Action::proposed_patch_file("action-1", "notes.txt", "missing", "new", "edit file")
                .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::PatchPatternMissing {
                path: path.clone(),
                pattern: "missing".to_string()
            })
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_overwrite_file_replaces_existing_file_and_verifies_contents() {
        let root = root("approved-overwrite");
        let path = root.join("notes.txt");
        fs::write(&path, "original").unwrap();
        let action = Action::proposed_overwrite_file(
            "action-1",
            "notes.txt",
            "replacement",
            "overwrite file",
        )
        .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Ok(VerifiedActionResult::File(
                FileActionVerification::FileOverwritten {
                    path: path.display().to_string()
                }
            ))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        assert_no_atomic_temp_files(&root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_overwrite_file_cleans_up_temp_file_when_rename_fails() {
        let root = root("overwrite-rename-failure-cleanup");
        let path = root.join("directory-target");
        fs::create_dir(&path).unwrap();
        let action = Action::proposed_overwrite_file(
            "action-1",
            "directory-target",
            "replacement",
            "overwrite file",
        )
        .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        match result {
            Err(FilesystemError::WriteFailed { path: failed, .. }) => assert_eq!(failed, path),
            other => panic!("expected write failure, got {other:?}"),
        }
        assert!(path.is_dir());
        assert_no_atomic_temp_files(&root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_overwrite_file_fails_when_target_is_missing() {
        let root = root("missing-overwrite-target");
        let path = root.join("notes.txt");
        let action = Action::proposed_overwrite_file(
            "action-1",
            "notes.txt",
            "replacement",
            "overwrite file",
        )
        .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::TargetMissing { path: path.clone() })
        );
        assert!(!path.exists());

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

    #[cfg(unix)]
    #[test]
    fn approved_write_file_rejects_symlinked_parent_escape() {
        use std::os::unix::fs::symlink;

        let root = root("symlink-parent-root");
        let outside = root
            .parent()
            .unwrap()
            .join(format!("elgar-fs-{}-symlink-outside", std::process::id()));
        let outside_target = outside.join("escaped.txt");
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("link")).unwrap();

        let action =
            Action::proposed_write_file("action-1", "link/escaped.txt", "contents", "write file")
                .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::UnsafeTarget {
                path: PathBuf::from("link/escaped.txt"),
                reason: "target parent resolves outside the allowed root".to_string()
            })
        );
        assert!(!outside_target.exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn approved_write_file_rejects_existing_symlink_target_escape() {
        use std::os::unix::fs::symlink;

        let root = root("symlink-target-root");
        let outside = root.parent().unwrap().join(format!(
            "elgar-fs-{}-symlink-target.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&outside);
        fs::write(&outside, "original").unwrap();
        symlink(&outside, root.join("linked.txt")).unwrap();
        let action =
            Action::proposed_write_file("action-1", "linked.txt", "contents", "write file")
                .approve();

        let result = Filesystem::apply_write_file(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::UnsafeTarget {
                path: PathBuf::from("linked.txt"),
                reason: "target file symlinks are not allowed".to_string()
            })
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "original");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn approved_overwrite_file_rejects_existing_symlink_target_escape() {
        use std::os::unix::fs::symlink;

        let root = root("overwrite-symlink-target-root");
        let outside = root.parent().unwrap().join(format!(
            "elgar-fs-{}-overwrite-symlink-target.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&outside);
        fs::write(&outside, "original").unwrap();
        symlink(&outside, root.join("linked.txt")).unwrap();
        let action = Action::proposed_overwrite_file(
            "action-1",
            "linked.txt",
            "replacement",
            "overwrite file",
        )
        .approve();

        let result = Filesystem::apply_file_action(&action, &root);

        assert_eq!(
            result,
            Err(FilesystemError::UnsafeTarget {
                path: PathBuf::from("linked.txt"),
                reason: "target file symlinks are not allowed".to_string()
            })
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "original");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&outside);
    }
}
