use elgar_core::{
    action::{ActionKind, ActionLifecycleState},
    event::{FileActionVerification, VerifiedActionResult},
    session::{
        PendingActionSelection, ProjectMemory, ProviderPromptMemoryOmittedFact,
        ProviderPromptMemorySelectedFact, ProviderPromptMemorySelection, Session,
        StructuredProjectPlan, StructuredProjectPlanStatus,
    },
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

    lines.join("\n")
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
        event::ProviderOutput,
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
