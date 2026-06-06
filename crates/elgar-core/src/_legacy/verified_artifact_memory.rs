use std::path::{Component, Path, PathBuf};

use crate::{
    event::{FileActionVerification, VerifiedActionResult},
    session::Session,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedArtifactFact {
    pub action_id: String,
    pub turn_index: u64,
    pub operation: &'static str,
    pub path: PathBuf,
    pub source_path: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CappedVerifiedArtifacts {
    pub artifacts: Vec<VerifiedArtifactFact>,
    pub omitted_count: usize,
}

pub(crate) fn verified_artifacts(session: &Session) -> Vec<VerifiedArtifactFact> {
    session
        .actions()
        .iter()
        .filter_map(|record| {
            let result = record.verified_result.as_ref()?;
            artifact_fact_from_result(session, result).map(|mut fact| {
                fact.action_id = record.action.id.clone();
                fact.turn_index = record.turn_index;
                fact
            })
        })
        .collect()
}

pub(crate) fn latest_action_turn_artifacts(
    session: &Session,
    limit: usize,
) -> CappedVerifiedArtifacts {
    let artifacts = verified_artifacts(session);
    let Some(latest_turn) = artifacts.iter().map(|artifact| artifact.turn_index).max() else {
        return CappedVerifiedArtifacts {
            artifacts: Vec::new(),
            omitted_count: 0,
        };
    };
    cap_artifacts(
        artifacts
            .into_iter()
            .filter(|artifact| artifact.turn_index == latest_turn),
        limit,
    )
}

pub(crate) fn latest_verified_artifacts(
    session: &Session,
    limit: usize,
) -> CappedVerifiedArtifacts {
    cap_artifacts(verified_artifacts(session).into_iter().rev(), limit)
}

pub(crate) fn earliest_verified_artifacts(
    session: &Session,
    limit: usize,
) -> CappedVerifiedArtifacts {
    cap_artifacts(verified_artifacts(session), limit)
}

pub(crate) fn verified_artifacts_under_folder(
    session: &Session,
    folder: &Path,
    limit: usize,
) -> CappedVerifiedArtifacts {
    let folder = normalize_path(folder);
    cap_artifacts(
        verified_artifacts(session)
            .into_iter()
            .filter(move |artifact| path_is_within(&artifact.path, &folder)),
        limit,
    )
}

fn cap_artifacts(
    artifacts: impl IntoIterator<Item = VerifiedArtifactFact>,
    limit: usize,
) -> CappedVerifiedArtifacts {
    let mut artifacts = artifacts.into_iter().collect::<Vec<_>>();
    let omitted_count = artifacts.len().saturating_sub(limit);
    artifacts.truncate(limit);
    CappedVerifiedArtifacts {
        artifacts,
        omitted_count,
    }
}

fn artifact_fact_from_result(
    session: &Session,
    result: &VerifiedActionResult,
) -> Option<VerifiedArtifactFact> {
    let (operation, path, source_path) = match result {
        VerifiedActionResult::FileWritten { path } => {
            ("wrote_file", absolute_session_path(session, path), None)
        }
        VerifiedActionResult::File(FileActionVerification::FileCreated { path }) => {
            ("created_file", absolute_session_path(session, path), None)
        }
        VerifiedActionResult::File(FileActionVerification::FilePatched { path }) => {
            ("patched_file", absolute_session_path(session, path), None)
        }
        VerifiedActionResult::File(FileActionVerification::FileOverwritten { path }) => {
            ("overwrote_file", absolute_session_path(session, path), None)
        }
        VerifiedActionResult::File(FileActionVerification::FileDeleted { path }) => {
            ("deleted_file", absolute_session_path(session, path), None)
        }
        VerifiedActionResult::File(FileActionVerification::FileMoved {
            source_path,
            target_path,
        }) => (
            "moved_file",
            absolute_session_path(session, target_path),
            Some(absolute_session_path(session, source_path)),
        ),
        VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => (
            "created_directory",
            absolute_session_path(session, path),
            None,
        ),
        VerifiedActionResult::Shell(_) => return None,
    };

    let project_root = infer_project_root(session, &path);
    Some(VerifiedArtifactFact {
        action_id: String::new(),
        turn_index: 0,
        operation,
        path,
        source_path,
        project_root,
    })
}

fn infer_project_root(session: &Session, path: &Path) -> Option<PathBuf> {
    session
        .project_memory()
        .verified_folders
        .iter()
        .map(|folder| folder.path.clone())
        .chain(
            session
                .project_memory()
                .structured_plans
                .iter()
                .map(|plan| plan.project_root.clone()),
        )
        .filter(|root| path_is_within(path, root))
        .max_by_key(|root| root.components().count())
}

fn absolute_session_path(session: &Session, path: impl AsRef<Path>) -> PathBuf {
    normalize_path(if path.as_ref().is_absolute() {
        path.as_ref().to_path_buf()
    } else {
        session.cwd.join(path)
    })
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    normalize_path(path).starts_with(normalize_path(root))
}

fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::{Action, ActionRequest, CreateFileAction},
        event::VerifiedActionResult,
        session::{ActionRecord, VerifiedFolderReference},
    };

    fn push_verified_file(session: &mut Session, action_id: &str, path: &str) {
        session.start_reasoning_trace(format!("turn {action_id}"));
        let action = Action::proposed(
            action_id,
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from(path),
                contents: String::new(),
            }),
            "create file",
        )
        .approve()
        .mark_applied();
        let mut record = ActionRecord::new(action);
        record.verified_result = Some(VerifiedActionResult::File(
            FileActionVerification::FileCreated {
                path: path.to_string(),
            },
        ));
        session.push_action(record);
    }

    #[test]
    fn derived_artifacts_include_only_verified_action_records() {
        let root = std::env::temp_dir().join(format!(
            "elgar-artifact-memory-{}-verified-only",
            std::process::id()
        ));
        let mut session = Session::new("session", &root, &root);
        push_verified_file(&mut session, "action-verified", "notes.txt");

        let pending = Action::proposed(
            "action-pending",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("pending.txt"),
                contents: String::new(),
            }),
            "pending",
        );
        session.push_action(ActionRecord::new(pending));

        let mut failed = ActionRecord::new(
            Action::proposed(
                "action-failed",
                ActionRequest::CreateFile(CreateFileAction {
                    target_path: PathBuf::from("failed.txt"),
                    contents: String::new(),
                }),
                "failed",
            )
            .mark_failed(),
        );
        failed.failure_reason = Some("failed".to_string());
        session.push_action(failed);

        let artifacts = verified_artifacts(&session);

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].action_id, "action-verified");
        assert!(artifacts[0].path.ends_with("notes.txt"));
    }

    #[test]
    fn derived_artifacts_support_latest_and_earliest_ordering() {
        let root = std::env::temp_dir().join(format!(
            "elgar-artifact-memory-{}-ordering",
            std::process::id()
        ));
        let mut session = Session::new("session", &root, &root);
        push_verified_file(&mut session, "action-first", "first.txt");
        push_verified_file(&mut session, "action-second", "second.txt");
        push_verified_file(&mut session, "action-third", "third.txt");

        let earliest = earliest_verified_artifacts(&session, 2);
        let latest = latest_verified_artifacts(&session, 2);

        assert_eq!(
            earliest
                .artifacts
                .iter()
                .map(|artifact| artifact.action_id.as_str())
                .collect::<Vec<_>>(),
            vec!["action-first", "action-second"]
        );
        assert_eq!(
            latest
                .artifacts
                .iter()
                .map(|artifact| artifact.action_id.as_str())
                .collect::<Vec<_>>(),
            vec!["action-third", "action-second"]
        );
        assert_eq!(earliest.omitted_count, 1);
        assert_eq!(latest.omitted_count, 1);
    }

    #[test]
    fn derived_artifacts_filter_under_folder() {
        let root = std::env::temp_dir().join(format!(
            "elgar-artifact-memory-{}-under-folder",
            std::process::id()
        ));
        let project = root.join("project");
        let mut session = Session::new("session", &root, &root);
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: project.clone(),
            source_action_id: "action-folder".to_string(),
        });
        push_verified_file(&mut session, "action-in", "project/notes.txt");
        push_verified_file(&mut session, "action-out", "other.txt");

        let under_project = verified_artifacts_under_folder(&session, &project, 10);

        assert_eq!(under_project.artifacts.len(), 1);
        assert_eq!(under_project.artifacts[0].action_id, "action-in");
        assert_eq!(under_project.artifacts[0].project_root, Some(project));
    }
}
