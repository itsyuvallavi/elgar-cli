use elgar_core::{
    action::{ActionKind, ActionLifecycleState},
    event::{Event, FileActionVerification, ProviderMetrics, VerifiedActionResult},
    plan_contract::{
        PlanContract, PlanContractDraftIssue, PlanContractDraftIssueKind, PlanContractStatus,
    },
    session::{
        PendingActionSelection, ProjectMemory, ProviderPromptMemoryOmittedFact,
        ProviderPromptMemorySelectedFact, ProviderPromptMemorySelection, Session,
        StructuredProjectPlan, StructuredProjectPlanStatus,
    },
    token_accounting::ContextWindowSource,
};
use std::path::Path;

pub fn render_session_memory(session: &Session) -> String {
    render_memory(
        session.project_memory(),
        session.latest_provider_prompt_memory_selection(),
    )
}

pub fn render_session_plan_preview(session: &Session) -> String {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return "Plan Preview\n(none)".to_string();
    };

    render_structured_plan_preview(session, plan)
}

pub fn render_session_status(session: &Session) -> String {
    let mut lines = vec!["Status".to_string()];
    lines.push(format!("actions: {}", session.actions().len()));
    lines.push(format!("pending: {}", pending_action_summary_line(session)));
    lines.push(format!(
        "applied: {}",
        session
            .actions()
            .iter()
            .filter(|record| record.action.state == ActionLifecycleState::Applied)
            .count()
    ));
    lines.push(format!(
        "failed: {}",
        session
            .actions()
            .iter()
            .filter(|record| record.action.state == ActionLifecycleState::Failed)
            .count()
    ));
    lines.push(format!(
        "rejected: {}",
        session
            .actions()
            .iter()
            .filter(|record| record.action.state == ActionLifecycleState::Rejected)
            .count()
    ));

    if let Some(folder) = session.project_memory().latest_verified_folder() {
        lines.push(format!(
            "latest folder: {}",
            display_session_path(session, folder.path.as_path())
        ));
    }
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        lines.push(format!(
            "latest plan: {}",
            display_session_path(session, plan.path.as_path())
        ));
    }

    lines.join("\n")
}

pub fn render_session_tokens(session: &Session) -> String {
    let snapshot = session.latest_context_window_snapshot();
    let totals = session.session_token_totals();
    let mut lines = vec!["Tokens".to_string()];
    lines.push(format!(
        "current context: {}",
        render_context_snapshot(&snapshot)
    ));
    if let Some(last) = session.latest_turn_token_usage() {
        lines.push(format!(
            "last turn: {} input + {} output = {} total [{}]",
            render_optional_tokens(last.input_tokens),
            render_optional_tokens(last.output_tokens),
            render_optional_tokens(last.total_tokens),
            source_label(last.source)
        ));
        lines.push(format!("last request: {}", last.request_id));
    } else {
        lines.push("last turn: unknown".to_string());
    }
    let request_summaries = latest_turn_provider_request_summaries(session);
    if !request_summaries.is_empty() {
        lines.push("last turn requests:".to_string());
        lines.extend(
            request_summaries
                .into_iter()
                .map(|summary| format!("- {}", render_provider_request_summary(summary))),
        );
    }
    lines.push(format!(
        "session total: {} input + {} output = {} total",
        format_tokens(totals.input_tokens),
        format_tokens(totals.output_tokens),
        format_tokens(totals.total_tokens)
    ));
    lines.push(format!(
        "reasoning/cache: {} reasoning, {} cache read, {} cache write",
        format_tokens(totals.reasoning_tokens),
        format_tokens(totals.cache_read_tokens),
        format_tokens(totals.cache_write_tokens)
    ));
    lines.push(format!(
        "local context: {}",
        session
            .context_accounting()
            .estimated_tokens
            .map(|tokens| format!("~{} estimated tokens", format_tokens(tokens)))
            .unwrap_or_else(|| "unknown".to_string())
    ));
    lines.push(format!("window source: {}", source_label(snapshot.source)));
    lines.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderRequestSummary {
    request_id: String,
    mode: String,
    tool_count: Option<usize>,
    metrics: Option<ProviderMetrics>,
    emitted_tool_calls: Option<bool>,
}

fn latest_turn_provider_request_summaries(session: &Session) -> Vec<ProviderRequestSummary> {
    let events = session.events();
    let start = events
        .iter()
        .rposition(|event| matches!(event, Event::UserMessage(_)))
        .unwrap_or(0);
    let mut summaries = Vec::new();

    for event in &events[start..] {
        match event {
            Event::ProviderStarted(started) => {
                summaries.push(ProviderRequestSummary {
                    request_id: started.request_id.clone(),
                    mode: started
                        .request_mode
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    tool_count: started.tool_count,
                    metrics: None,
                    emitted_tool_calls: None,
                });
            }
            Event::ProviderFinished(finished) => {
                if let Some(summary) = summaries
                    .iter_mut()
                    .rev()
                    .find(|summary| summary.request_id == finished.request_id)
                {
                    summary.metrics = finished.output.metrics.clone();
                    summary.emitted_tool_calls = Some(!finished.output.tool_calls.is_empty());
                }
            }
            _ => {}
        }
    }

    summaries
}

fn render_provider_request_summary(summary: ProviderRequestSummary) -> String {
    let mut parts = vec![format!("{} {}", summary.mode, summary.request_id)];
    if let Some(metrics) = summary.metrics {
        if let Some(usage) = metrics.usage {
            parts.push(format!(
                "{} input + {} output = {} total",
                render_optional_tokens(usage.prompt_tokens),
                render_optional_tokens(usage.completion_tokens),
                render_optional_tokens(usage.total_tokens.or_else(|| {
                    usage
                        .prompt_tokens
                        .unwrap_or_default()
                        .checked_add(usage.completion_tokens.unwrap_or_default())
                }))
            ));
        } else {
            parts.push("tokens unknown".to_string());
        }
        if let Some(duration) = metrics.total_duration_millis {
            parts.push(format!("{duration} ms"));
        }
        parts.push(format!("messages {}", metrics.message_count));
        parts.push(format!("bytes {}", metrics.serialized_request_bytes));
    } else {
        parts.push("metrics unknown".to_string());
    }
    if let Some(tool_count) = summary.tool_count {
        parts.push(format!("tools {tool_count}"));
    }
    if let Some(emitted) = summary.emitted_tool_calls {
        parts.push(format!("tool calls {}", if emitted { "yes" } else { "no" }));
    }
    parts.join(" · ")
}

pub fn render_session_state_snapshot(session: &Session) -> String {
    let mut lines = vec!["State".to_string()];
    lines.push(format!("pending: {}", pending_action_summary_line(session)));
    lines.push(format!(
        "applied actions: {}",
        session
            .actions()
            .iter()
            .filter(|record| record.action.state == ActionLifecycleState::Applied)
            .count()
    ));

    let created = session
        .actions()
        .iter()
        .filter_map(|record| record.verified_result.as_ref())
        .filter_map(|result| verified_creation_line(session, result))
        .collect::<Vec<_>>();
    if created.is_empty() {
        lines.push("created: (none)".to_string());
    } else {
        lines.push("created:".to_string());
        for line in created {
            lines.push(format!("- {line}"));
        }
    }

    let memory = session.project_memory();
    if memory.verified_folders.is_empty()
        && memory.verified_plans.is_empty()
        && memory.structured_plans.is_empty()
    {
        lines.push("memory: (none)".to_string());
        return lines.join("\n");
    }

    lines.push("memory:".to_string());
    if !memory.verified_folders.is_empty() {
        lines.push("verified folders:".to_string());
        for reference in memory.verified_folders.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::Directory),
                display_session_path(session, &reference.path),
                reference.source_action_id
            ));
        }
    }
    if !memory.verified_plans.is_empty() {
        lines.push("verified plans:".to_string());
        for reference in memory.verified_plans.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::File),
                display_session_path(session, &reference.path),
                reference.source_action_id
            ));
            lines.push(format!(
                "  root {} {}",
                path_state(&reference.project_root, PathKind::Directory),
                display_session_path(session, &reference.project_root)
            ));
        }
    }
    if let Some(plan) = memory.latest_structured_plan() {
        lines.push("latest structured plan:".to_string());
        lines.push(format!(
            "- {} {}",
            structured_status(plan.runtime_status()),
            display_session_path(session, &plan.source_plan_path)
        ));
        lines.push(format!(
            "  root {} {}",
            path_state(&plan.project_root, PathKind::Directory),
            display_session_path(session, &plan.project_root)
        ));
        lines.push(format!(
            "  dirs {}",
            path_count(&plan.expected_directories, PathKind::Directory)
        ));
        lines.push(format!(
            "  files {}",
            path_count(&plan.expected_files, PathKind::File)
        ));
    }

    lines.join("\n")
}

fn render_structured_plan_preview(session: &Session, plan: &StructuredProjectPlan) -> String {
    let mut lines = vec!["Plan Preview".to_string()];
    lines.push(format!(
        "status: {}",
        structured_status(plan.runtime_status())
    ));
    lines.push(format!("stage: {}", plan.stage));
    lines.push(format!(
        "source action: {}",
        plan.source_action_id.as_deref().unwrap_or("unknown-action")
    ));
    lines.push(format!(
        "plan: {}",
        display_session_path(session, &plan.source_plan_path)
    ));
    lines.push(format!(
        "root: {}",
        display_session_path(session, &plan.project_root)
    ));

    if plan.expected_directories.is_empty() {
        lines.push("directories: (none listed)".to_string());
    } else {
        lines.push(format!(
            "directories: {}/{} present",
            plan.expected_directories_present_count(),
            plan.expected_directories.len()
        ));
        for path in &plan.expected_directories {
            lines.push(format!(
                "- {} {}",
                path_state(path, PathKind::Directory),
                display_session_path(session, path)
            ));
        }
    }

    if plan.expected_files.is_empty() {
        lines.push("files: (none listed)".to_string());
    } else {
        lines.push(format!(
            "files: {}/{} present",
            plan.expected_files_present_count(),
            plan.expected_files.len()
        ));
        for path in &plan.expected_files {
            lines.push(format!(
                "- {} {}",
                path_state(path, PathKind::File),
                display_session_path(session, path)
            ));
        }
    }

    if let Some(contract) = session
        .latest_plan_contract()
        .filter(|contract| contract.source_plan_path == plan.source_plan_path)
    {
        render_plan_contract_review(session, contract, &mut lines);
    }

    lines.join("\n")
}

fn render_plan_contract_review(
    session: &Session,
    contract: &PlanContract,
    lines: &mut Vec<String>,
) {
    let review = contract.review_draft();
    lines.push("contract review:".to_string());
    lines.push(format!(
        "- status: {}",
        plan_contract_status(contract.runtime_status())
    ));
    lines.push(format!(
        "- approvable: {}",
        if review.is_approvable() { "yes" } else { "no" }
    ));

    if review.issues.is_empty() {
        lines.push("- blocking issues: none".to_string());
    } else {
        lines.push("- blocking issues:".to_string());
        for issue in &review.issues {
            lines.push(format!("  - {}", draft_issue_line(session, issue)));
        }
    }

    if contract.scope.verification_steps.is_empty() {
        lines.push("- verification: missing".to_string());
    } else {
        lines.push("- verification:".to_string());
        for step in &contract.scope.verification_steps {
            lines.push(format!("  - {step}"));
        }
    }

    if contract.scope.acceptance_criteria.is_empty() {
        lines.push("- acceptance criteria: missing".to_string());
    } else {
        lines.push("- acceptance criteria:".to_string());
        for criterion in &contract.scope.acceptance_criteria {
            lines.push(format!("  - {criterion}"));
        }
    }
}

fn draft_issue_line(session: &Session, issue: &PlanContractDraftIssue) -> String {
    let path = issue
        .path
        .as_ref()
        .map(|path| format!(": {}", display_session_path(session, path)))
        .unwrap_or_default();

    match &issue.kind {
        PlanContractDraftIssueKind::ContractNotDraft { status } => {
            format!("contract is not draft ({})", plan_contract_status(*status))
        }
        PlanContractDraftIssueKind::MissingSourcePlan => format!("missing source plan{path}"),
        PlanContractDraftIssueKind::MissingProjectRoot => format!("missing project root{path}"),
        PlanContractDraftIssueKind::SourcePlanOutsideProjectRoot => {
            format!("source plan outside project root{path}")
        }
        PlanContractDraftIssueKind::EmptyExecutableScope => {
            "missing concrete file tree or expected path list".to_string()
        }
        PlanContractDraftIssueKind::PathOutsideProjectRoot => {
            format!("planned path outside project root{path}")
        }
        PlanContractDraftIssueKind::MalformedScopePath => {
            format!("malformed planned path{path}")
        }
        PlanContractDraftIssueKind::ReferencedPathMissingFromScope => {
            format!("referenced path missing from plan scope{path}")
        }
        PlanContractDraftIssueKind::InvalidPythonModuleReference { module } => {
            format!("invalid Python module reference: {module}")
        }
        PlanContractDraftIssueKind::DuplicateScopePath => {
            format!("duplicate planned path{path}")
        }
        PlanContractDraftIssueKind::MissingVerificationSteps => {
            "missing Verification section".to_string()
        }
        PlanContractDraftIssueKind::MissingAcceptanceCriteria => {
            "missing Acceptance Criteria section".to_string()
        }
    }
}

pub fn render_session_pending_action(session: &Session) -> String {
    format!("Pending\n{}", pending_action_summary_line(session))
}

pub fn render_session_created_actions(session: &Session) -> String {
    let lines = session
        .actions()
        .iter()
        .filter_map(|record| record.verified_result.as_ref())
        .filter_map(|result| verified_creation_line(session, result))
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return "Created\n(none)".to_string();
    }

    format!("Created\n- {}", lines.join("\n- "))
}

fn pending_action_summary_line(session: &Session) -> String {
    match session.pending_action_selection() {
        PendingActionSelection::None => "none".to_string(),
        PendingActionSelection::Ambiguous => {
            "multiple actions waiting; use /approve or /reject after resolving the queue"
                .to_string()
        }
        PendingActionSelection::Single(index) => {
            let Some(record) = session.actions().get(index) else {
                return "none".to_string();
            };
            format!(
                "{} {} at {}; {}",
                action_kind_label(record.action.kind()),
                record.action.id,
                record.action.request.approval_target(),
                record.action.summary
            )
        }
    }
}

fn verified_creation_line(session: &Session, result: &VerifiedActionResult) -> Option<String> {
    match result {
        VerifiedActionResult::FileWritten { path } => Some(format!(
            "file {}",
            display_session_path(session, Path::new(path))
        )),
        VerifiedActionResult::File(verification) => match verification {
            FileActionVerification::FileCreated { path } => Some(format!(
                "file {}",
                display_session_path(session, Path::new(path))
            )),
            FileActionVerification::DirectoryCreated { path } => Some(format!(
                "directory {}",
                display_session_path(session, Path::new(path))
            )),
            FileActionVerification::FilePatched { .. }
            | FileActionVerification::FileOverwritten { .. }
            | FileActionVerification::FileDeleted { .. }
            | FileActionVerification::FileMoved { .. } => None,
        },
        VerifiedActionResult::Shell(_) => None,
    }
}

fn action_kind_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::CreateFile => "create_file",
        ActionKind::PatchFile => "patch_file",
        ActionKind::OverwriteFile => "overwrite_file",
        ActionKind::DeleteFile => "delete_file",
        ActionKind::MoveFile => "move_file",
        ActionKind::CreateDirectory => "create_directory",
        ActionKind::ShellCommand => "shell_command",
    }
}

fn display_session_path(session: &Session, path: &Path) -> String {
    path.strip_prefix(&session.project_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn render_context_snapshot(
    snapshot: &elgar_core::token_accounting::ContextWindowSnapshot,
) -> String {
    let current = render_optional_context_tokens(snapshot.current_tokens, snapshot.source);
    let window = snapshot
        .context_window_tokens
        .map(format_tokens)
        .unwrap_or_else(|| "?".to_string());
    let percent = snapshot
        .used_percent
        .map(|percent| format!("{percent}%"))
        .unwrap_or_else(|| "?%".to_string());
    format!(
        "{current} / {window} ({percent}) [{}]",
        source_label(snapshot.source)
    )
}

fn render_optional_context_tokens(tokens: Option<u64>, source: ContextWindowSource) -> String {
    match (tokens, source) {
        (Some(tokens), ContextWindowSource::Estimate) => format!("~{}", format_tokens(tokens)),
        (Some(tokens), _) => format_tokens(tokens),
        (None, _) => "?".to_string(),
    }
}

fn render_optional_tokens(tokens: Option<u64>) -> String {
    tokens.map(format_tokens).unwrap_or_else(|| "?".to_string())
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn source_label(source: ContextWindowSource) -> &'static str {
    match source {
        ContextWindowSource::Provider => "provider",
        ContextWindowSource::Estimate => "estimate",
        ContextWindowSource::Unknown => "unknown",
    }
}

fn render_memory(
    memory: &ProjectMemory,
    provider_selection: Option<&ProviderPromptMemorySelection>,
) -> String {
    let has_provider_selection = provider_selection
        .is_some_and(|selection| !selection.selected.is_empty() || !selection.omitted.is_empty());
    if memory.verified_folders.is_empty()
        && memory.verified_plans.is_empty()
        && memory.structured_plans.is_empty()
        && !has_provider_selection
    {
        return "Memory\n(empty)".to_string();
    }

    let mut lines = vec!["Memory".to_string()];

    if !memory.verified_folders.is_empty() {
        lines.push("folders".to_string());
        for reference in memory.verified_folders.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::Directory),
                reference.path.display(),
                reference.source_action_id
            ));
        }
    }

    if !memory.verified_plans.is_empty() {
        lines.push("plans".to_string());
        for reference in memory.verified_plans.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::File),
                reference.path.display(),
                reference.source_action_id
            ));
            lines.push(format!(
                "  root {} {}",
                path_state(&reference.project_root, PathKind::Directory),
                reference.project_root.display()
            ));
        }
    }

    if !memory.structured_plans.is_empty() {
        lines.push("structured plans".to_string());
        for plan in memory.structured_plans.iter().rev() {
            let action = plan.source_action_id.as_deref().unwrap_or("unknown-action");
            lines.push(format!(
                "- {} {} ({})",
                structured_status(plan.runtime_status()),
                plan.stage,
                action
            ));
            lines.push(format!(
                "  plan {} {}",
                path_state(&plan.source_plan_path, PathKind::File),
                plan.source_plan_path.display()
            ));
            lines.push(format!(
                "  root {} {}",
                path_state(&plan.project_root, PathKind::Directory),
                plan.project_root.display()
            ));
            if !plan.expected_directories.is_empty() {
                lines.push(format!(
                    "  dirs {}",
                    path_count(&plan.expected_directories, PathKind::Directory)
                ));
            }
            if !plan.expected_files.is_empty() {
                lines.push(format!(
                    "  files {}",
                    path_count(&plan.expected_files, PathKind::File)
                ));
            }
        }
    }

    if let Some(selection) = provider_selection {
        render_provider_prompt_memory_selection(selection, &mut lines);
    }

    lines.join("\n")
}

#[derive(Debug, Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

fn path_state(path: &Path, kind: PathKind) -> &'static str {
    match kind {
        PathKind::Directory if path.is_dir() => "ok",
        PathKind::File
            if path.is_file() && path.metadata().is_ok_and(|metadata| metadata.len() == 0) =>
        {
            "empty"
        }
        PathKind::File if path.is_file() => "ok",
        _ => "missing",
    }
}

fn path_count(paths: &[std::path::PathBuf], kind: PathKind) -> String {
    let present = paths
        .iter()
        .filter(|path| match kind {
            PathKind::Directory => path.is_dir(),
            PathKind::File => path.is_file(),
        })
        .count();
    format!("{present}/{}", paths.len())
}

fn structured_status(status: StructuredProjectPlanStatus) -> &'static str {
    match status {
        StructuredProjectPlanStatus::Draft => "draft",
        StructuredProjectPlanStatus::Verified => "verified",
        StructuredProjectPlanStatus::Executing => "executing",
        StructuredProjectPlanStatus::Completed => "completed",
        StructuredProjectPlanStatus::Stale => "stale",
    }
}

fn plan_contract_status(status: PlanContractStatus) -> &'static str {
    match status {
        PlanContractStatus::Draft => "draft",
        PlanContractStatus::Approved => "approved",
        PlanContractStatus::Executing => "executing",
        PlanContractStatus::NeedsRevision => "needs_revision",
        PlanContractStatus::Completed => "completed",
        PlanContractStatus::Rejected => "rejected",
        PlanContractStatus::Stale => "stale",
    }
}

fn render_provider_prompt_memory_selection(
    selection: &ProviderPromptMemorySelection,
    lines: &mut Vec<String>,
) {
    if selection.selected.is_empty() && selection.omitted.is_empty() {
        return;
    }

    lines.push("provider prompt memory".to_string());
    if !selection.selected.is_empty() {
        lines.push("selected".to_string());
        for fact in &selection.selected {
            render_selected_provider_prompt_memory_fact(fact, lines);
        }
    }
    if !selection.omitted.is_empty() {
        lines.push("omitted".to_string());
        for fact in &selection.omitted {
            render_omitted_provider_prompt_memory_fact(fact, lines);
        }
    }
}

fn render_selected_provider_prompt_memory_fact(
    fact: &ProviderPromptMemorySelectedFact,
    lines: &mut Vec<String>,
) {
    lines.push(format!(
        "- {} {} {} ({})",
        provider_memory_kind_label(&fact.kind),
        provider_memory_path_state(&fact.kind, &fact.path),
        fact.path.display(),
        fact.source_action_id
    ));
    if let Some(project_root) = fact.project_root.as_ref() {
        lines.push(format!(
            "  root {} {}",
            path_state(project_root, PathKind::Directory),
            project_root.display()
        ));
    }
}

fn render_omitted_provider_prompt_memory_fact(
    fact: &ProviderPromptMemoryOmittedFact,
    lines: &mut Vec<String>,
) {
    lines.push(format!(
        "- {} {} {} ({}; {})",
        provider_memory_kind_label(&fact.kind),
        provider_memory_path_state(&fact.kind, &fact.path),
        fact.path.display(),
        fact.source_action_id,
        fact.reason
    ));
    if let Some(project_root) = fact.project_root.as_ref() {
        lines.push(format!(
            "  root {} {}",
            path_state(project_root, PathKind::Directory),
            project_root.display()
        ));
    }
}

fn provider_memory_kind_label(kind: &str) -> String {
    match kind {
        "verified_folder" => "verified folder".to_string(),
        "verified_plan" => "verified plan".to_string(),
        "structured_plan" => "structured plan".to_string(),
        other => other.replace('_', " "),
    }
}

fn provider_memory_path_state(kind: &str, path: &Path) -> &'static str {
    let path_kind = match kind {
        "verified_folder" => PathKind::Directory,
        _ => PathKind::File,
    };
    path_state(path, path_kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgar_core::{
        agent_runtime::AgentRuntime,
        controller::Controller,
        event::{ProviderMetrics, ProviderOutput, ProviderTokenUsage},
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
        policy::PermissionPolicyMode,
        provider::{
            ChatMessage, ChatRole, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata,
        },
    };
    use std::fs;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("elgar-memory-render-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn renders_tokens_with_current_window_and_session_totals_separated() {
        let root = temp_root("tokens-provider");
        fs::write(root.join("AGENTS.md"), "local context").unwrap();
        let mut session = Session::new("memory-session", &root, &root);
        let runtime = AgentRuntime::default();
        runtime.refresh_context_accounting(&mut session, Some(128_000));
        Controller::new(TokenUsageProvider).model_turn(&mut session, "hello");

        let rendered = render_session_tokens(&session);

        assert!(rendered.contains("current context: 42.0k / 128.0k (32%) [provider]"));
        assert!(rendered.contains("last turn: 40.0k input + 2.0k output = 42.0k total [provider]"));
        assert!(rendered.contains("last turn requests:"));
        assert!(rendered.contains("- plain request-usage · 40.0k input + 2.0k output = 42.0k total · messages 2 · bytes 64 · tools 0 · tool calls no"));
        assert!(rendered.contains("session total: 40.0k input + 2.0k output = 42.0k total"));
        assert!(rendered.contains("local context: ~"));

        let _ = fs::remove_dir_all(root);
    }

    #[derive(Debug, Clone)]
    struct TokenUsageProvider;

    impl ControllerProvider for TokenUsageProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "usage-provider",
                Some("model-a".to_string()),
                "request-usage",
            )
        }

        fn chat_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<ProviderOutput, ProviderError> {
            self.chat("hello")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            let mut metrics =
                ProviderMetrics::new("request-usage", Some("model-a".to_string()), false, 2, 64);
            metrics.usage = Some(ProviderTokenUsage {
                prompt_tokens: Some(40_000),
                completion_tokens: Some(2_000),
                total_tokens: Some(42_000),
            });
            Ok(ProviderOutput::new("measured").with_metrics(metrics))
        }
    }

    #[test]
    fn renders_empty_memory_compactly() {
        let session = Session::new("memory-empty", "/repo", "/repo");

        assert_eq!(render_session_memory(&session), "Memory\n(empty)");
        assert!(!render_session_memory(&session).contains("provider prompt memory"));
    }

    #[test]
    fn renders_verified_and_stale_memory_without_provider_calls() {
        let root = temp_root("verified-stale");
        let folder = root.join("project");
        let mut session = Session::new("memory-session", &root, &root);

        tool_runtime(
            ModelToolName::CreateDirectory,
            serde_json::json!({"target_path": "project"}),
        )
        .tool_turn(
            &mut session,
            "create folder project",
            PermissionPolicyMode::FullAccess,
        );
        tool_runtime(
            ModelToolName::CreateFile,
            serde_json::json!({
                "target_path": "project/small-python-project-plan.md",
                "contents": "# Project Plan\n",
            }),
        )
        .tool_turn(
            &mut session,
            "create project plan",
            PermissionPolicyMode::FullAccess,
        );

        let plan_path = folder.join("small-python-project-plan.md");
        assert!(plan_path.is_file());

        let rendered = render_session_memory(&session);
        assert!(rendered.contains("folders\n- ok "));
        assert!(rendered.contains("plans\n- ok "));

        fs::remove_file(&plan_path).unwrap();
        let rendered = render_session_memory(&session);
        assert!(rendered.contains("- missing "));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_plan_preview_lifecycle_from_verified_paths() {
        let root = temp_root("plan-preview-lifecycle");
        let project = root.join("DemoApp");
        fs::create_dir_all(&project).unwrap();
        let mut session = Session::new("memory-session", &root, &root);
        let plan_contents = "# Project Plan\n\n```text\nsrc/\n└─ main.py\nrequirements.txt\n```\n";

        tool_runtime(
            ModelToolName::CreateFile,
            serde_json::json!({
                "target_path": "DemoApp/plan.md",
                "contents": plan_contents,
            }),
        )
        .tool_turn(
            &mut session,
            "create project plan",
            PermissionPolicyMode::FullAccess,
        );

        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("status: verified"));
        assert!(rendered.contains("stage: verified-plan"));
        assert!(rendered.contains("source action: action-1"));
        assert!(rendered.contains("plan: DemoApp/plan.md"));
        assert!(rendered.contains("root: DemoApp"));
        assert!(rendered.contains("directories: 0/1 present"));
        assert!(rendered.contains("files: 0/2 present"));
        assert!(rendered.contains("contract review:"));
        assert!(rendered.contains("- status: draft"));
        assert!(rendered.contains("- approvable: no"));
        assert!(rendered.contains("missing Verification section"));
        assert!(rendered.contains("missing Acceptance Criteria section"));

        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/main.py"), "print('hello')\n").unwrap();
        fs::write(project.join("requirements.txt"), "").unwrap();
        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("status: completed"));
        assert!(rendered.contains("directories: 1/1 present"));
        assert!(rendered.contains("files: 2/2 present"));
        assert!(rendered.contains("- empty DemoApp/requirements.txt"));

        fs::remove_file(project.join("plan.md")).unwrap();
        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("status: stale"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_plan_contract_review_when_plan_is_approvable() {
        let root = temp_root("plan-contract-review-approvable");
        let project = root.join("DemoApp");
        fs::create_dir_all(&project).unwrap();
        let mut session = Session::new("memory-session", &root, &root);
        let plan_contents = "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Run the CLI smoke test.\n\n## Acceptance Criteria\n- The expected files exist.\n";

        tool_runtime(
            ModelToolName::CreateFile,
            serde_json::json!({
                "target_path": "DemoApp/plan.md",
                "contents": plan_contents,
            }),
        )
        .tool_turn(
            &mut session,
            "create project plan",
            PermissionPolicyMode::FullAccess,
        );

        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("contract review:"));
        assert!(rendered.contains("- status: draft"));
        assert!(rendered.contains("- approvable: yes"));
        assert!(rendered.contains("- blocking issues: none"));
        assert!(rendered.contains("- verification:\n  - Run the CLI smoke test."));
        assert!(rendered.contains("- acceptance criteria:\n  - The expected files exist."));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_plan_preview_with_cleaned_markdown_list_file_tree_paths() {
        let root = temp_root("plan-contract-review-cleaned-list-tree");
        let project = root.join("plan-review-copy-test");
        fs::create_dir_all(&project).unwrap();
        let mut session = Session::new("memory-session", &root, &root);
        let plan_contents = "# Project Plan\n\n```text\n  - app.py\n  - __init__.py\n  - cli.py\n  - README.md\n  - requirements.txt\n  - tests\n    - test_app.py\n```\n\n## Verification\n- Ensure all listed files exist and contain minimal placeholder content.\n\n## Acceptance Criteria\n- The project directory exists with the specified structure.\n";

        tool_runtime(
            ModelToolName::CreateFile,
            serde_json::json!({
                "target_path": "plan-review-copy-test/PLAN.md",
                "contents": plan_contents,
            }),
        )
        .tool_turn(
            &mut session,
            "create project plan",
            PermissionPolicyMode::FullAccess,
        );

        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("root: plan-review-copy-test"));
        assert!(rendered.contains("- missing plan-review-copy-test/app.py"));
        assert!(rendered.contains("- missing plan-review-copy-test/tests"));
        assert!(rendered.contains("- missing plan-review-copy-test/tests/test_app.py"));
        assert!(!rendered.contains("/- "));
        assert!(!rendered.contains("plan-review-copy-test/- "));
        assert!(rendered.contains("- approvable: yes"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_plan_contract_review_for_incoherent_referenced_paths() {
        let root = temp_root("plan-contract-review-incoherent-paths");
        let project = root.join("plan-review-copy-test");
        fs::create_dir_all(&project).unwrap();
        let mut session = Session::new("memory-session", &root, &root);
        let plan_contents = "# Project Plan\n\n```text\nREADME.md\n__init__.py\ntests/\n```\n\n## Verification\n- Verify that `cli.py` can be executed with `python -m plan-review-copy-test.cli` and displays help.\n- Run `pytest tests/test_cli.py` to ensure all unit tests pass.\n\n## Acceptance Criteria\n- The project contains a clear `README.md` with usage instructions.\n";

        tool_runtime(
            ModelToolName::CreateFile,
            serde_json::json!({
                "target_path": "plan-review-copy-test/PROJECT_PLAN.md",
                "contents": plan_contents,
            }),
        )
        .tool_turn(
            &mut session,
            "create project plan",
            PermissionPolicyMode::FullAccess,
        );

        let rendered = render_session_plan_preview(&session);
        assert!(rendered.contains("- approvable: no"));
        assert!(rendered
            .contains("referenced path missing from plan scope: plan-review-copy-test/cli.py"));
        assert!(rendered.contains(
            "referenced path missing from plan scope: plan-review-copy-test/tests/test_cli.py"
        ));
        assert!(rendered.contains("invalid Python module reference: plan-review-copy-test.cli"));

        let _ = fs::remove_dir_all(root);
    }

    fn tool_runtime(
        name: ModelToolName,
        arguments: serde_json::Value,
    ) -> AgentRuntime<ScriptedToolProvider> {
        AgentRuntime::new(ScriptedToolProvider {
            output: ProviderOutput::new("tool output").with_tool_calls(vec![RawModelToolCall {
                id: "call-tool".to_string(),
                name: RawModelToolName::Known(name),
                arguments,
                assistant_summary: Some("tool action".to_string()),
            }]),
        })
    }

    #[derive(Debug, Clone)]
    struct ScriptedToolProvider {
        output: ProviderOutput,
    }

    impl ControllerProvider for ScriptedToolProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "tool-provider",
                Some("tool-model".to_string()),
                "request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("plain response"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, ChatRole::Tool))
            {
                return Ok(ProviderOutput::new("Done."));
            }

            Ok(self.output.clone())
        }
    }
}
