use std::path::Path;

use serde_json::{json, Value};

use crate::{
    event::{FileActionVerification, VerifiedActionResult},
    plan_tree::{render_expected_path_tree, ExpectedPathTreeEntry},
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
    PlanStatus,
    Status,
    Memory,
    Summary,
}

impl VerifiedStateAnswerKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            VerifiedStateAnswerKind::LatestFolder => "latest_folder",
            VerifiedStateAnswerKind::LatestFile => "latest_file",
            VerifiedStateAnswerKind::CreatedSummary => "created_summary",
            VerifiedStateAnswerKind::RecentChanges => "recent_changes",
            VerifiedStateAnswerKind::LastBlock => "last_block",
            VerifiedStateAnswerKind::Pending => "pending",
            VerifiedStateAnswerKind::Plan => "plan",
            VerifiedStateAnswerKind::PlanDetails => "plan_details",
            VerifiedStateAnswerKind::ProjectFiles => "project_files",
            VerifiedStateAnswerKind::FirstCreated => "first_created",
            VerifiedStateAnswerKind::PlanStatus => "plan_status",
            VerifiedStateAnswerKind::Status => "status",
            VerifiedStateAnswerKind::Memory => "memory",
            VerifiedStateAnswerKind::Summary => "summary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedStateAnswerKind {
    pub(crate) requested_kind: VerifiedStateAnswerKind,
    pub(crate) resolved_kind: VerifiedStateAnswerKind,
    pub(crate) fallback_reason: Option<&'static str>,
}

impl ResolvedStateAnswerKind {
    fn unchanged(kind: VerifiedStateAnswerKind) -> Self {
        Self {
            requested_kind: kind,
            resolved_kind: kind,
            fallback_reason: None,
        }
    }

    fn changed(
        requested_kind: VerifiedStateAnswerKind,
        resolved_kind: VerifiedStateAnswerKind,
        fallback_reason: &'static str,
    ) -> Self {
        Self {
            requested_kind,
            resolved_kind,
            fallback_reason: Some(fallback_reason),
        }
    }
}

pub(crate) fn resolve_state_answer_kind(
    session: &Session,
    input: &str,
    requested_kind: VerifiedStateAnswerKind,
) -> ResolvedStateAnswerKind {
    if requested_kind == VerifiedStateAnswerKind::LatestFolder {
        if let Some(plan) = referenced_structured_plan(session, input) {
            if plan.expected_files_present_count() > 0 {
                return ResolvedStateAnswerKind::changed(
                    requested_kind,
                    VerifiedStateAnswerKind::ProjectFiles,
                    "requested_latest_folder_with_referenced_project_files",
                );
            }
        }
    }

    if state_answer_kind_has_data(session, requested_kind) {
        return ResolvedStateAnswerKind::unchanged(requested_kind);
    }

    if let Some(plan) = referenced_structured_plan(session, input)
        .or_else(|| session.project_memory().latest_structured_plan())
    {
        if plan.expected_files_present_count() > 0 {
            return ResolvedStateAnswerKind::changed(
                requested_kind,
                VerifiedStateAnswerKind::ProjectFiles,
                "requested_empty_kind_with_project_files",
            );
        }
        if plan.runtime_status() == StructuredProjectPlanStatus::Completed {
            return ResolvedStateAnswerKind::changed(
                requested_kind,
                VerifiedStateAnswerKind::PlanStatus,
                "requested_empty_kind_with_completed_plan",
            );
        }
    }

    if verified_artifact_count(session) > 0 {
        return ResolvedStateAnswerKind::changed(
            requested_kind,
            VerifiedStateAnswerKind::CreatedSummary,
            "requested_empty_kind_with_verified_artifacts",
        );
    }

    ResolvedStateAnswerKind::unchanged(requested_kind)
}

pub(crate) fn verified_session_state_answer(
    session: &Session,
    kind: VerifiedStateAnswerKind,
) -> String {
    match kind {
        VerifiedStateAnswerKind::LatestFolder => latest_verified_created_directory_path(session)
            .or_else(|| latest_verified_project_folder_path(session))
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
        VerifiedStateAnswerKind::PlanStatus => verified_plan_status_answer(session),
        VerifiedStateAnswerKind::Status => verified_status_answer(session),
        VerifiedStateAnswerKind::Memory => verified_memory_answer(session),
        VerifiedStateAnswerKind::Summary => verified_summary_answer(session),
    }
}

pub(crate) fn verified_state_answer_trace_metadata(
    session: &Session,
    kind: VerifiedStateAnswerKind,
) -> Value {
    let structured_plans = &session.project_memory().structured_plans;
    let mut project_roots = Vec::new();
    for plan in structured_plans {
        let root = display_agent_context_path(session, &plan.project_root);
        if !project_roots.iter().any(|seen| seen == &root) {
            project_roots.push(root);
        }
    }
    let selected_plan = structured_plans.last().map(|plan| {
        json!({
            "plan": display_agent_context_path(session, &plan.source_plan_path),
            "root": display_agent_context_path(session, &plan.project_root),
            "status": structured_plan_status_label(plan.runtime_status()),
            "expected_files": plan.expected_files.len(),
            "present_files": plan.expected_files_present_count(),
            "expected_directories": plan.expected_directories.len(),
            "present_directories": plan.expected_directories_present_count(),
        })
    });

    json!({
        "state_answer_kind": kind.as_str(),
        "answer_scope": state_answer_scope(kind),
        "plan_count": structured_plans.len(),
        "known_project_count": project_roots.len(),
        "selected_plan": selected_plan,
    })
}

pub(crate) fn resolved_state_answer_trace_metadata(
    session: &Session,
    resolution: ResolvedStateAnswerKind,
) -> Value {
    let mut metadata = verified_state_answer_trace_metadata(session, resolution.resolved_kind);
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "requested_state_answer_kind".to_string(),
            json!(resolution.requested_kind.as_str()),
        );
        object.insert(
            "resolved_state_answer_kind".to_string(),
            json!(resolution.resolved_kind.as_str()),
        );
        object.insert(
            "state_answer_fallback_reason".to_string(),
            resolution
                .fallback_reason
                .map_or(Value::Null, |reason| json!(reason)),
        );
        object.insert(
            "verified_artifact_count".to_string(),
            json!(verified_artifact_count(session)),
        );
    }
    metadata
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
        "plan_status" | "plan_execution_status" | "plans" | "all_plans" => {
            Some(VerifiedStateAnswerKind::PlanStatus)
        }
        "status" => Some(VerifiedStateAnswerKind::Status),
        "memory" => Some(VerifiedStateAnswerKind::Memory),
        "summary" => Some(VerifiedStateAnswerKind::Summary),
        _ => None,
    }
}

fn state_answer_kind_has_data(session: &Session, kind: VerifiedStateAnswerKind) -> bool {
    match kind {
        VerifiedStateAnswerKind::LatestFolder => {
            latest_verified_created_directory_path(session).is_some()
                || latest_verified_project_folder_path(session).is_some()
                || session.project_memory().latest_verified_folder().is_some()
        }
        VerifiedStateAnswerKind::LatestFile => latest_verified_file_path(session).is_some(),
        VerifiedStateAnswerKind::CreatedSummary => verified_artifact_count(session) > 0,
        VerifiedStateAnswerKind::RecentChanges => session
            .actions_in_latest_action_turn()
            .iter()
            .any(|record| record.verified_result.is_some()),
        VerifiedStateAnswerKind::LastBlock => session.latest_runtime_block().is_some(),
        VerifiedStateAnswerKind::Pending => !matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        ),
        VerifiedStateAnswerKind::Plan
        | VerifiedStateAnswerKind::PlanDetails
        | VerifiedStateAnswerKind::PlanStatus => {
            session.project_memory().latest_structured_plan().is_some()
                || session.project_memory().latest_verified_plan().is_some()
        }
        VerifiedStateAnswerKind::ProjectFiles => project_files_answer_has_data(session),
        VerifiedStateAnswerKind::FirstCreated => verified_artifact_count(session) > 0,
        VerifiedStateAnswerKind::Status | VerifiedStateAnswerKind::Summary => {
            verified_artifact_count(session) > 0
                || !matches!(
                    session.pending_action_selection(),
                    PendingActionSelection::None
                )
                || session.project_memory().latest_structured_plan().is_some()
                || session.project_memory().latest_verified_folder().is_some()
                || session.project_memory().latest_verified_plan().is_some()
        }
        VerifiedStateAnswerKind::Memory => {
            session.project_memory().latest_verified_folder().is_some()
                || session.project_memory().latest_verified_plan().is_some()
        }
    }
}

fn project_files_answer_has_data(session: &Session) -> bool {
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        return plan.expected_files_present_count() > 0;
    }

    session
        .project_memory()
        .latest_verified_folder()
        .is_some_and(|folder| {
            !verified_artifacts_under_folder(session, &folder.path, 1)
                .artifacts
                .is_empty()
        })
}

fn referenced_structured_plan<'a>(
    session: &'a Session,
    input: &str,
) -> Option<&'a crate::session::StructuredProjectPlan> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    session
        .project_memory()
        .structured_plans
        .iter()
        .rev()
        .find(|plan| {
            path_is_referenced_in_input(session, input, &plan.project_root)
                || path_is_referenced_in_input(session, input, &plan.source_plan_path)
        })
}

fn path_is_referenced_in_input(session: &Session, input: &str, path: &Path) -> bool {
    let display = display_agent_context_path(session, path);
    !display.is_empty() && display != "." && input.contains(&display)
}

fn verified_artifact_count(session: &Session) -> usize {
    session
        .actions()
        .iter()
        .filter(|record| record.verified_result.is_some())
        .count()
}

fn state_answer_scope(kind: VerifiedStateAnswerKind) -> &'static str {
    match kind {
        VerifiedStateAnswerKind::LatestFolder | VerifiedStateAnswerKind::LatestFile => "latest",
        VerifiedStateAnswerKind::CreatedSummary => "created_inventory",
        VerifiedStateAnswerKind::RecentChanges => "latest_action_turn",
        VerifiedStateAnswerKind::LastBlock => "runtime_block",
        VerifiedStateAnswerKind::Pending => "pending_actions",
        VerifiedStateAnswerKind::Plan | VerifiedStateAnswerKind::PlanDetails => "latest_plan",
        VerifiedStateAnswerKind::ProjectFiles => "latest_project",
        VerifiedStateAnswerKind::FirstCreated => "earliest_artifact",
        VerifiedStateAnswerKind::PlanStatus => "all_plans",
        VerifiedStateAnswerKind::Status | VerifiedStateAnswerKind::Summary => "session_status",
        VerifiedStateAnswerKind::Memory => "verified_memory",
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
            "status: {} · dirs {}/{} · files {}/{}",
            structured_plan_status_label(plan.runtime_status()),
            plan.expected_directories_present_count(),
            plan.expected_directories.len(),
            plan.expected_files_present_count(),
            plan.expected_files.len()
        ),
        format!(
            "plan: {}",
            display_agent_context_path(session, &plan.source_plan_path)
        ),
        format!(
            "root: {}",
            display_agent_context_path(session, &plan.project_root)
        ),
    ];

    render_structured_plan_expected_tree(&plan.project_root, plan, &mut lines);

    if include_contents {
        if let Ok(contents) = std::fs::read_to_string(&plan.source_plan_path) {
            let contents = contents.trim();
            if !contents.is_empty() {
                lines.push("contents:".to_string());
                lines.push(contents.to_string());
            }
        }
    }

    lines.join("\n")
}

fn render_structured_plan_expected_tree(
    root: &Path,
    plan: &crate::session::StructuredProjectPlan,
    lines: &mut Vec<String>,
) {
    if plan.expected_directories.is_empty() && plan.expected_files.is_empty() {
        lines.push("tree: (none listed)".to_string());
        return;
    }

    let mut entries = Vec::new();
    entries.extend(
        plan.expected_directories
            .iter()
            .map(|path| ExpectedPathTreeEntry::directory(path, directory_state_label(path))),
    );
    entries.extend(
        plan.expected_files
            .iter()
            .map(|path| ExpectedPathTreeEntry::file(path, file_state_label(path))),
    );

    lines.push("tree:".to_string());
    lines.extend(render_expected_path_tree(root, &entries));
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

fn verified_plan_status_answer(session: &Session) -> String {
    let lines = structured_plan_summary_lines(session);
    if lines.is_empty() {
        return "No verified plan recorded.".to_string();
    }

    let mut rendered = vec![format!("plans: {}", lines.len())];
    rendered.extend(lines.into_iter().map(|line| format!("- {line}")));
    rendered.join("\n")
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
    let plan_lines = structured_plan_summary_lines(session);
    if !plan_lines.is_empty() {
        lines.push("plans:".to_string());
        lines.extend(plan_lines.into_iter().map(|line| format!("- {line}")));
    }
    lines.join("\n")
}

fn structured_plan_summary_lines(session: &Session) -> Vec<String> {
    session
        .project_memory()
        .structured_plans
        .iter()
        .map(|plan| {
            format!(
                "{}: {} · dirs {}/{} · files {}/{} · plan {}",
                display_agent_context_path(session, &plan.project_root),
                structured_plan_status_label(plan.runtime_status()),
                plan.expected_directories_present_count(),
                plan.expected_directories.len(),
                plan.expected_files_present_count(),
                plan.expected_files.len(),
                display_agent_context_path(session, &plan.source_plan_path)
            )
        })
        .collect()
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

fn latest_verified_project_folder_path(session: &Session) -> Option<String> {
    session
        .project_memory()
        .latest_verified_folder()
        .map(|folder| display_agent_context_path(session, &folder.path))
        .or_else(|| {
            session
                .project_memory()
                .latest_structured_plan()
                .filter(|plan| plan.project_root.is_dir())
                .map(|plan| display_agent_context_path(session, &plan.project_root))
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

        assert!(
            answer.contains("status: verified · dirs 2/2 · files 2/3"),
            "got: {answer}"
        );
        assert!(answer.contains("plan: plan.md"), "got: {answer}");
        assert!(answer.contains("tree:"), "got: {answer}");
        assert!(answer.contains("[ok] src/"), "got: {answer}");
        assert!(answer.contains("  [ok] main.py"), "got: {answer}");
        assert!(answer.contains("[empty] requirements.txt"), "got: {answer}");
        assert!(answer.contains("  [missing] test_main.py"), "got: {answer}");
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
    fn plan_status_reports_all_structured_plans() {
        let root = std::env::temp_dir().join(format!(
            "elgar-state-answer-{}-plan-status",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let first = root.join("First");
        let second = root.join("Second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("plan.md"), "# First\n").unwrap();
        std::fs::write(second.join("plan.md"), "# Second\n").unwrap();
        std::fs::write(second.join("README.md"), "# Second\n").unwrap();

        let mut session = Session::new("session", &root, &root);
        session.record_structured_project_plan(structured_plan(
            &first,
            "action-first-plan",
            &[],
            &["README.md"],
        ));
        session.record_structured_project_plan(structured_plan(
            &second,
            "action-second-plan",
            &[],
            &["README.md"],
        ));

        let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::PlanStatus);

        assert!(answer.contains("plans: 2"), "got: {answer}");
        assert!(
            answer.contains("- First: verified · dirs 0/0 · files 0/1 · plan First/plan.md"),
            "got: {answer}"
        );
        assert!(
            answer.contains("- Second: completed · dirs 0/0 · files 1/1 · plan Second/plan.md"),
            "got: {answer}"
        );

        let metadata =
            verified_state_answer_trace_metadata(&session, VerifiedStateAnswerKind::PlanStatus);
        assert_eq!(
            metadata
                .get("state_answer_kind")
                .and_then(serde_json::Value::as_str),
            Some("plan_status")
        );
        assert_eq!(
            metadata
                .get("answer_scope")
                .and_then(serde_json::Value::as_str),
            Some("all_plans")
        );
        assert_eq!(
            metadata
                .get("plan_count")
                .and_then(serde_json::Value::as_u64),
            Some(2)
        );

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
        assert_eq!(
            parse_verified_state_answer_kind("plan_status"),
            Some(VerifiedStateAnswerKind::PlanStatus)
        );
        assert_eq!(
            parse_verified_state_answer_kind("all_plans"),
            Some(VerifiedStateAnswerKind::PlanStatus)
        );
    }
}
