use std::path::Path;

use serde_json::Value;

use crate::{
    event::{FileActionVerification, VerifiedActionResult},
    session::{PendingActionSelection, Session, StructuredProjectPlanStatus},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifiedStateAnswerKind {
    LatestFolder,
    LatestFile,
    CreatedSummary,
    Pending,
    Plan,
    Status,
    Memory,
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct VerifiedStateClassification {
    pub answer_kind: Option<VerifiedStateAnswerKind>,
    pub needs_runtime_context: bool,
}

pub(crate) fn parse_verified_state_classification_output(
    message: &str,
) -> VerifiedStateClassification {
    let Some(value) = parse_json_value(message) else {
        return VerifiedStateClassification::default();
    };
    let needs_runtime_context = value
        .get("needs_runtime_context")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(kind) = value.get("answer_kind").and_then(Value::as_str) {
        return VerifiedStateClassification {
            answer_kind: parse_verified_state_answer_kind(kind),
            needs_runtime_context,
        };
    }

    let answer_kind = match value.get("needs_verified_state").and_then(Value::as_bool) {
        Some(true) => Some(VerifiedStateAnswerKind::Summary),
        _ => None,
    };
    VerifiedStateClassification {
        answer_kind,
        needs_runtime_context,
    }
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
        VerifiedStateAnswerKind::Pending => verified_pending_answer(session),
        VerifiedStateAnswerKind::Plan => verified_plan_answer(session),
        VerifiedStateAnswerKind::Status => verified_status_answer(session),
        VerifiedStateAnswerKind::Memory => verified_memory_answer(session),
        VerifiedStateAnswerKind::Summary => verified_summary_answer(session),
    }
}

fn parse_json_value(message: &str) -> Option<Value> {
    serde_json::from_str::<Value>(message.trim())
        .ok()
        .or_else(|| {
            let start = message.find('{')?;
            let end = message.rfind('}')?;
            (start < end)
                .then(|| serde_json::from_str::<Value>(&message[start..=end]).ok())
                .flatten()
        })
}

fn parse_verified_state_answer_kind(kind: &str) -> Option<VerifiedStateAnswerKind> {
    match kind {
        "none" => None,
        "latest_folder" => Some(VerifiedStateAnswerKind::LatestFolder),
        "latest_file" => Some(VerifiedStateAnswerKind::LatestFile),
        "created_summary" => Some(VerifiedStateAnswerKind::CreatedSummary),
        "pending" => Some(VerifiedStateAnswerKind::Pending),
        "plan" => Some(VerifiedStateAnswerKind::Plan),
        "status" => Some(VerifiedStateAnswerKind::Status),
        "memory" => Some(VerifiedStateAnswerKind::Memory),
        "summary" => Some(VerifiedStateAnswerKind::Summary),
        _ => None,
    }
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
        return format!(
            "plan: {}\nroot: {}\nstatus: {}",
            display_agent_context_path(session, &plan.source_plan_path),
            display_agent_context_path(session, &plan.project_root),
            structured_plan_status_label(plan.runtime_status())
        );
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
