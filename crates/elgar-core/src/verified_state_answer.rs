use std::path::Path;

use crate::{
    event::{FileActionVerification, VerifiedActionResult},
    session::{PendingActionSelection, Session, StructuredProjectPlanStatus},
    verified_artifact_memory::{earliest_verified_artifacts, verified_artifacts_under_folder},
};

const STATE_ARTIFACT_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifiedStateAnswerKind {
    LatestFolder,
    LatestFile,
    CreatedSummary,
    RecentChanges,
    LastBlock,
    Pending,
    Plan,
    PlanDetails,
    ProjectFiles,
    FirstCreated,
    Status,
    Memory,
    Summary,
}

pub(crate) fn verified_session_state_answer(
    session: &Session,
    kind: VerifiedStateAnswerKind,
) -> String {
    match kind {
        VerifiedStateAnswerKind::LatestFolder => latest_verified_created_directory_path(session)
            .unwrap_or_else(|| "No verified folder creation recorded.".to_string()),
        VerifiedStateAnswerKind::LatestFile => latest_verified_created_file_path(session)
            .or_else(|| latest_verified_file_path(session))
            .unwrap_or_else(|| "No verified file change recorded.".to_string()),
        VerifiedStateAnswerKind::CreatedSummary => verified_created_summary(session),
        VerifiedStateAnswerKind::RecentChanges => verified_recent_changes_answer(session),
        VerifiedStateAnswerKind::LastBlock => verified_last_block_answer(session),
        VerifiedStateAnswerKind::Pending => verified_pending_answer(session),
        VerifiedStateAnswerKind::Plan => verified_plan_answer(session),
        VerifiedStateAnswerKind::PlanDetails => verified_plan_details_answer(session),
        VerifiedStateAnswerKind::ProjectFiles => verified_project_files_answer(session),
        VerifiedStateAnswerKind::FirstCreated => verified_first_created_answer(session),
        VerifiedStateAnswerKind::Status => verified_status_answer(session),
        VerifiedStateAnswerKind::Memory => verified_memory_answer(session),
        VerifiedStateAnswerKind::Summary => verified_summary_answer(session),
    }
}

pub(crate) fn parse_verified_state_answer_kind(kind: &str) -> Option<VerifiedStateAnswerKind> {
    match kind {
        "none" => None,
        "latest_folder" => Some(VerifiedStateAnswerKind::LatestFolder),
        "latest_file" => Some(VerifiedStateAnswerKind::LatestFile),
        "created_summary" => Some(VerifiedStateAnswerKind::CreatedSummary),
        "recent_changes" => Some(VerifiedStateAnswerKind::RecentChanges),
        "last_block" | "last_outcome" => Some(VerifiedStateAnswerKind::LastBlock),
        "pending" => Some(VerifiedStateAnswerKind::Pending),
        "plan" => Some(VerifiedStateAnswerKind::Plan),
        "plan_details" => Some(VerifiedStateAnswerKind::PlanDetails),
        "project_files" | "created_project_files" | "latest_project_files" => {
            Some(VerifiedStateAnswerKind::ProjectFiles)
        }
        "first_created" | "earliest_created" | "first_file" => {
            Some(VerifiedStateAnswerKind::FirstCreated)
        }
        "status" => Some(VerifiedStateAnswerKind::Status),
        "memory" => Some(VerifiedStateAnswerKind::Memory),
        "summary" => Some(VerifiedStateAnswerKind::Summary),
        _ => None,
    }
}

fn verified_last_block_answer(session: &Session) -> String {
    session
        .latest_runtime_block()
        .map(|block| block.message.clone())
        .unwrap_or_else(|| "No runtime block recorded.".to_string())
}

fn verified_summary_answer(session: &Session) -> String {
    let pending = match session.pending_action_selection() {
        PendingActionSelection::None => None,
        PendingActionSelection::Single(_) => Some("Pending: one action.".to_string()),
        PendingActionSelection::Ambiguous => Some("Pending: multiple actions.".to_string()),
    };

    let latest_folder = latest_verified_created_directory_path(session).or_else(|| {
        session
            .project_memory()
            .latest_verified_folder()
            .map(|folder| display_agent_context_path(session, &folder.path))
    });
    let latest_file = latest_verified_file_path(session);

    let mut lines = Vec::new();
    if let Some(folder) = latest_folder {
        lines.push(("latest folder", folder));
    }
    if let Some(file) = latest_file {
        lines.push(("latest file", file));
    }

    match (lines.as_slice(), pending.as_ref()) {
        ([], None) => "No verified filesystem changes recorded.".to_string(),
        ([], Some(pending)) => pending.clone(),
        ([(.., value)], None) => value.clone(),
        _ => {
            let mut rendered = lines
                .into_iter()
                .map(|(label, value)| format!("{label}: {value}"))
                .collect::<Vec<_>>();
            if let Some(pending) = pending {
                rendered.push(pending);
            }
            rendered.join("\n")
        }
    }
}

fn verified_created_summary(session: &Session) -> String {
    let entries = verified_created_entries(session);
    if entries.is_empty() {
        "No verified creations recorded.".to_string()
    } else {
        entries.join("\n")
    }
}

fn verified_recent_changes_answer(session: &Session) -> String {
    let entries = session
        .actions_in_latest_action_turn()
        .into_iter()
        .filter_map(|record| record.verified_result.as_ref())
        .filter_map(|result| recent_change_entry(session, result))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        "No verified changes recorded in the latest action.".to_string()
    } else {
        entries.join("\n")
    }
}

fn recent_change_entry(session: &Session, result: &VerifiedActionResult) -> Option<String> {
    match result {
        VerifiedActionResult::FileWritten { path } => Some(format!(
            "wrote {}",
            display_agent_context_path(session, Path::new(path))
        )),
        VerifiedActionResult::File(file) => match file {
            FileActionVerification::FileCreated { path } => Some(format!(
                "created {}",
                display_agent_context_path(session, Path::new(path))
            )),
            FileActionVerification::DirectoryCreated { path } => Some(format!(
                "created directory {}",
                display_agent_context_path(session, Path::new(path))
            )),
            FileActionVerification::FilePatched { path } => Some(format!(
                "patched {}",
                display_agent_context_path(session, Path::new(path))
            )),
            FileActionVerification::FileOverwritten { path } => Some(format!(
                "overwrote {}",
                display_agent_context_path(session, Path::new(path))
            )),
            FileActionVerification::FileDeleted { path } => Some(format!(
                "deleted {}",
                display_agent_context_path(session, Path::new(path))
            )),
            FileActionVerification::FileMoved { target_path, .. } => Some(format!(
                "moved to {}",
                display_agent_context_path(session, Path::new(target_path))
            )),
        },
        VerifiedActionResult::Shell(shell) => shell
            .verified_effect
            .as_ref()
            .map(|effect| format!("ran command: {effect}"))
            .or_else(|| Some("ran command".to_string())),
    }
}

fn verified_pending_answer(session: &Session) -> String {
    match session.pending_action_selection() {
        PendingActionSelection::None => "No pending action.".to_string(),
        PendingActionSelection::Single(index) => session
            .actions()
            .get(index)
            .map(|record| {
                format!(
                    "pending: {} at {}",
                    record.action.id,
                    record.action.request.approval_target()
                )
            })
            .unwrap_or_else(|| "No pending action.".to_string()),
        PendingActionSelection::Ambiguous => "Multiple pending actions.".to_string(),
    }
}

fn verified_plan_answer(session: &Session) -> String {
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        return structured_plan_state_answer(session, plan, false);
    }
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        return format!(
            "plan: {}\nroot: {}",
            display_agent_context_path(session, &plan.path),
            display_agent_context_path(session, &plan.project_root)
        );
    }

    "No verified plan recorded.".to_string()
}

fn verified_plan_details_answer(session: &Session) -> String {
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        return structured_plan_state_answer(session, plan, true);
    }

    let Some(plan) = session.project_memory().latest_verified_plan() else {
        return "No verified plan recorded.".to_string();
    };

    let header = format!(
        "plan: {}\nroot: {}",
        display_agent_context_path(session, &plan.path),
        display_agent_context_path(session, &plan.project_root)
    );

    match std::fs::read_to_string(&plan.path) {
        Ok(contents) if !contents.trim().is_empty() => {
            format!("{header}\n\n{}", contents.trim())
        }
        _ => header,
    }
}

fn structured_plan_state_answer(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    include_contents: bool,
) -> String {
    let mut lines = vec![
        format!(
            "plan: {}",
            display_agent_context_path(session, &plan.source_plan_path)
        ),
        format!(
            "root: {}",
            display_agent_context_path(session, &plan.project_root)
        ),
        format!(
            "status: {}",
            structured_plan_status_label(plan.runtime_status())
        ),
    ];

    lines.push(format!(
        "directories: {}/{} present",
        plan.expected_directories_present_count(),
        plan.expected_directories.len()
    ));
    for path in &plan.expected_directories {
        lines.push(format!(
            "- {} {}",
            directory_state_label(path),
            display_agent_context_path(session, path)
        ));
    }

    lines.push(format!(
        "files: {}/{} present",
        plan.expected_files_present_count(),
        plan.expected_files.len()
    ));
    for path in &plan.expected_files {
        lines.push(format!(
            "- {} {}",
            file_state_label(path),
            display_agent_context_path(session, path)
        ));
    }

    if include_contents {
        if let Ok(contents) = std::fs::read_to_string(&plan.source_plan_path) {
            let contents = contents.trim();
            if !contents.is_empty() {
                lines.push("plan contents:".to_string());
                lines.push(contents.to_string());
            }
        }
    }

    lines.join("\n")
}

fn verified_project_files_answer(session: &Session) -> String {
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        let mut lines = vec![
            format!(
                "project: {}",
                display_agent_context_path(session, &plan.project_root)
            ),
            format!(
                "plan: {}",
                display_agent_context_path(session, &plan.source_plan_path)
            ),
            format!(
                "status: {}",
                structured_plan_status_label(plan.runtime_status())
            ),
            format!(
                "files: {}/{} present",
                plan.expected_files_present_count(),
                plan.expected_files.len()
            ),
        ];
        for path in &plan.expected_files {
            lines.push(format!(
                "- {} {}",
                file_state_label(path),
                display_agent_context_path(session, path)
            ));
        }
        return lines.join("\n");
    }

    let Some(folder) = session.project_memory().latest_verified_folder() else {
        return "No verified project files recorded.".to_string();
    };

    let capped = verified_artifacts_under_folder(session, &folder.path, STATE_ARTIFACT_LIMIT);
    let mut lines = vec![format!(
        "project: {}",
        display_agent_context_path(session, &folder.path)
    )];
    lines.extend(capped.artifacts.iter().map(|artifact| {
        format!(
            "- {} {}",
            artifact.operation,
            display_agent_context_path(session, &artifact.path)
        )
    }));
    if capped.omitted_count > 0 {
        lines.push(format!("... {} more omitted", capped.omitted_count));
    }
    if lines.len() == 1 {
        "No verified project files recorded.".to_string()
    } else {
        lines.join("\n")
    }
}

fn verified_first_created_answer(session: &Session) -> String {
    let capped = earliest_verified_artifacts(session, 1);
    let Some(artifact) = capped.artifacts.first() else {
        return "No verified created file recorded.".to_string();
    };

    format!(
        "first created: {}\naction: {}\nturn: {}\nkind: {}",
        display_agent_context_path(session, &artifact.path),
        artifact.action_id,
        artifact.turn_index,
        artifact.operation
    )
}

fn verified_status_answer(session: &Session) -> String {
    let applied = session
        .actions()
        .iter()
        .filter(|record| record.action.state == crate::action::ActionLifecycleState::Applied)
        .count();
    let mut lines = vec![
        format!("pending: {}", verified_pending_summary_value(session)),
        format!("applied actions: {applied}"),
    ];
    if let Some(folder) = latest_verified_created_directory_path(session) {
        lines.push(format!("latest folder: {folder}"));
    }
    if let Some(file) = latest_verified_file_path(session) {
        lines.push(format!("latest file: {file}"));
    }
    lines.join("\n")
}

fn verified_pending_summary_value(session: &Session) -> String {
    match session.pending_action_selection() {
        PendingActionSelection::None => "none".to_string(),
        PendingActionSelection::Single(index) => session
            .actions()
            .get(index)
            .map(|record| {
                format!(
                    "{} at {}",
                    record.action.id,
                    record.action.request.approval_target()
                )
            })
            .unwrap_or_else(|| "none".to_string()),
        PendingActionSelection::Ambiguous => "multiple".to_string(),
    }
}

fn structured_plan_status_label(status: StructuredProjectPlanStatus) -> &'static str {
    match status {
        StructuredProjectPlanStatus::Draft => "draft",
        StructuredProjectPlanStatus::Verified => "verified",
        StructuredProjectPlanStatus::Executing => "executing",
        StructuredProjectPlanStatus::Completed => "completed",
        StructuredProjectPlanStatus::Stale => "stale",
    }
}

fn file_state_label(path: &Path) -> &'static str {
    if !path.is_file() {
        return "missing";
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() == 0 => "empty",
        Ok(_) => "ok",
        Err(_) => "ok",
    }
}

fn directory_state_label(path: &Path) -> &'static str {
    if path.is_dir() {
        "ok"
    } else {
        "missing"
    }
}

fn verified_memory_answer(session: &Session) -> String {
    let mut lines = Vec::new();
    if let Some(folder) = session.project_memory().latest_verified_folder() {
        lines.push(format!(
            "latest folder: {}",
            display_agent_context_path(session, &folder.path)
        ));
    }
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        lines.push(format!(
            "latest plan: {}",
            display_agent_context_path(session, &plan.path)
        ));
    }
    if lines.is_empty() {
        "No verified memory recorded.".to_string()
    } else {
        lines.join("\n")
    }
}

fn verified_created_entries(session: &Session) -> Vec<String> {
    session
        .actions()
        .iter()
        .filter_map(|record| record.verified_result.as_ref())
        .filter_map(|result| match result {
            VerifiedActionResult::FileWritten { path } => Some(format!(
                "file {}",
                display_agent_context_path(session, Path::new(path))
            )),
            VerifiedActionResult::File(FileActionVerification::FileCreated { path }) => {
                Some(format!(
                    "file {}",
                    display_agent_context_path(session, Path::new(path))
                ))
            }
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
                Some(format!(
                    "directory {}",
                    display_agent_context_path(session, Path::new(path))
                ))
            }
            VerifiedActionResult::File(
                FileActionVerification::FilePatched { .. }
                | FileActionVerification::FileOverwritten { .. }
                | FileActionVerification::FileDeleted { .. }
                | FileActionVerification::FileMoved { .. },
            )
            | VerifiedActionResult::Shell(_) => None,
        })
        .collect()
}

fn latest_verified_created_directory_path(session: &Session) -> Option<String> {
    session.actions().iter().rev().find_map(|record| {
        let result = record.verified_result.as_ref()?;
        match result {
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
                Some(display_agent_context_path(session, Path::new(path)))
            }
            _ => None,
        }
    })
}

fn latest_verified_created_file_path(session: &Session) -> Option<String> {
    session.actions().iter().rev().find_map(|record| {
        let result = record.verified_result.as_ref()?;
        match result {
            VerifiedActionResult::FileWritten { path }
            | VerifiedActionResult::File(FileActionVerification::FileCreated { path }) => {
                Some(display_agent_context_path(session, Path::new(path)))
            }
            _ => None,
        }
    })
}

fn latest_verified_file_path(session: &Session) -> Option<String> {
    session.actions().iter().rev().find_map(|record| {
        let result = record.verified_result.as_ref()?;
        match result {
            VerifiedActionResult::FileWritten { path } => {
                Some(display_agent_context_path(session, Path::new(path)))
            }
            VerifiedActionResult::File(file) => match file {
                FileActionVerification::FileCreated { path }
                | FileActionVerification::FilePatched { path }
                | FileActionVerification::FileOverwritten { path }
                | FileActionVerification::FileDeleted { path } => {
                    Some(display_agent_context_path(session, Path::new(path)))
                }
                FileActionVerification::FileMoved { target_path, .. } => {
                    Some(display_agent_context_path(session, Path::new(target_path)))
                }
                FileActionVerification::DirectoryCreated { .. } => None,
            },
            VerifiedActionResult::Shell(_) => None,
        }
    })
}

fn display_agent_context_path(session: &Session, path: &Path) -> String {
    let display_path = path
        .strip_prefix(&session.cwd)
        .or_else(|_| path.strip_prefix(&session.project_root))
        .unwrap_or(path);
    if display_path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        display_path.display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::Action,
        event::FileActionVerification,
        session::{ActionRecord, StructuredProjectPlan, VerifiedPlanReference},
    };

    fn applied_create(session: &mut Session, id: &str, path: &str) {
        let mut record = ActionRecord::new(
            Action::proposed_create_file(id, path, "", "create")
                .approve()
                .mark_applied(),
        );
        record.verified_result = Some(VerifiedActionResult::File(
            FileActionVerification::FileCreated {
                path: path.to_string(),
            },
        ));
        session.push_action(record);
    }

    fn structured_plan(
        root: &Path,
        action_id: &str,
        expected_directories: &[&str],
        expected_files: &[&str],
    ) -> StructuredProjectPlan {
        StructuredProjectPlan {
            source_action_id: Some(action_id.to_string()),
            source_plan_path: root.join("plan.md"),
            project_root: root.to_path_buf(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: expected_directories
                .iter()
                .map(|path| root.join(path))
                .collect(),
            expected_files: expected_files.iter().map(|path| root.join(path)).collect(),
        }
    }

    #[test]
    fn plan_details_reports_expected_dirs_and_files_from_structured_plan() {
        let root = std::env::temp_dir().join(format!(
            "elgar-state-answer-{}-plan-details",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("plan.md"),
            "# Project Plan\n\n## Verification\n- Run tests.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        std::fs::write(root.join("src/main.py"), "print('hello')\n").unwrap();
        std::fs::write(root.join("requirements.txt"), "").unwrap();
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: root.join("plan.md"),
            project_root: root.clone(),
            source_action_id: "action-plan".to_string(),
        });
        session.record_structured_project_plan(structured_plan(
            &root,
            "action-plan",
            &["src", "tests"],
            &["src/main.py", "requirements.txt", "tests/test_main.py"],
        ));

        let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::PlanDetails);

        assert!(answer.contains("plan: plan.md"), "got: {answer}");
        assert!(answer.contains("directories: 2/2 present"), "got: {answer}");
        assert!(answer.contains("- ok src"), "got: {answer}");
        assert!(answer.contains("files: 2/3 present"), "got: {answer}");
        assert!(answer.contains("- ok src/main.py"), "got: {answer}");
        assert!(answer.contains("- empty requirements.txt"), "got: {answer}");
        assert!(
            answer.contains("- missing tests/test_main.py"),
            "got: {answer}"
        );
        assert!(answer.contains("## Verification"), "got: {answer}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_files_reports_latest_project_without_older_or_unrelated_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-state-answer-{}-project-files",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let first = root.join("FirstProject");
        let second = root.join("SecondProject");
        std::fs::create_dir_all(first.join("src")).unwrap();
        std::fs::create_dir_all(second.join("budget")).unwrap();
        std::fs::write(first.join("plan.md"), "# First Plan\n").unwrap();
        std::fs::write(first.join("src/main.py"), "print('first')\n").unwrap();
        std::fs::write(second.join("plan.md"), "# Second Plan\n").unwrap();
        std::fs::write(second.join("README.md"), "# Second\n").unwrap();
        std::fs::write(second.join("budget/cli.py"), "def main(): pass\n").unwrap();
        std::fs::write(root.join("extra.txt"), "outside\n").unwrap();

        let mut session = Session::new("session", &root, &root);
        session.record_structured_project_plan(structured_plan(
            &first,
            "action-first-plan",
            &["src"],
            &["src/main.py"],
        ));
        session.record_structured_project_plan(structured_plan(
            &second,
            "action-second-plan",
            &["budget"],
            &["README.md", "budget/cli.py"],
        ));

        let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::ProjectFiles);

        assert!(answer.contains("project: SecondProject"), "got: {answer}");
        assert!(answer.contains("files: 2/2 present"), "got: {answer}");
        assert!(
            answer.contains("- ok SecondProject/README.md"),
            "got: {answer}"
        );
        assert!(
            answer.contains("- ok SecondProject/budget/cli.py"),
            "got: {answer}"
        );
        assert!(
            !answer.contains("FirstProject/src/main.py"),
            "got: {answer}"
        );
        assert!(!answer.contains("extra.txt"), "got: {answer}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn first_created_reports_earliest_verified_artifact() {
        let root = std::env::temp_dir().join(format!(
            "elgar-state-answer-{}-first-created",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("session", &root, &root);

        session.start_reasoning_trace("create plan");
        applied_create(&mut session, "action-plan", "Project/plan.md");
        session.start_reasoning_trace("create implementation");
        applied_create(&mut session, "action-readme", "Project/README.md");

        let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::FirstCreated);

        assert!(
            answer.contains("first created: Project/plan.md"),
            "got: {answer}"
        );
        assert!(answer.contains("action: action-plan"), "got: {answer}");
        assert!(answer.contains("turn: 1"), "got: {answer}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recent_changes_reports_only_the_latest_action_turn() {
        let root = std::env::temp_dir();
        let mut session = Session::new("session", &root, &root);

        // Turn 1: scaffold several files.
        session.start_reasoning_trace("scaffold the app");
        applied_create(&mut session, "a1", "app/page.tsx");
        applied_create(&mut session, "a2", "app/layout.tsx");

        // Turn 2: one targeted fix.
        session.start_reasoning_trace("fix the config");
        applied_create(&mut session, "b1", "next.config.js");

        // Turn 3: the question is asked in its own action-free turn.
        session.start_reasoning_trace("what did you just do?");

        let recent =
            verified_session_state_answer(&session, VerifiedStateAnswerKind::RecentChanges);
        assert!(recent.contains("next.config.js"), "got: {recent}");
        assert!(!recent.contains("page.tsx"), "got: {recent}");
        assert!(!recent.contains("layout.tsx"), "got: {recent}");

        // created_summary still reflects the whole session inventory.
        let inventory =
            verified_session_state_answer(&session, VerifiedStateAnswerKind::CreatedSummary);
        assert!(inventory.contains("page.tsx"), "got: {inventory}");
        assert!(inventory.contains("next.config.js"), "got: {inventory}");
    }

    #[test]
    fn recent_changes_reports_nothing_without_verified_actions() {
        let root = std::env::temp_dir();
        let mut session = Session::new("session", &root, &root);
        session.start_reasoning_trace("hello");

        assert_eq!(
            verified_session_state_answer(&session, VerifiedStateAnswerKind::RecentChanges),
            "No verified changes recorded in the latest action."
        );
    }

    #[test]
    fn parses_recent_changes_answer_kind() {
        assert_eq!(
            parse_verified_state_answer_kind("recent_changes"),
            Some(VerifiedStateAnswerKind::RecentChanges)
        );
    }

    #[test]
    fn parses_last_block_answer_kind() {
        assert_eq!(
            parse_verified_state_answer_kind("last_block"),
            Some(VerifiedStateAnswerKind::LastBlock)
        );
        assert_eq!(
            parse_verified_state_answer_kind("last_outcome"),
            Some(VerifiedStateAnswerKind::LastBlock)
        );
    }

    #[test]
    fn parses_project_files_and_first_created_answer_kinds() {
        assert_eq!(
            parse_verified_state_answer_kind("project_files"),
            Some(VerifiedStateAnswerKind::ProjectFiles)
        );
        assert_eq!(
            parse_verified_state_answer_kind("first_created"),
            Some(VerifiedStateAnswerKind::FirstCreated)
        );
        assert_eq!(
            parse_verified_state_answer_kind("first_file"),
            Some(VerifiedStateAnswerKind::FirstCreated)
        );
    }
}
