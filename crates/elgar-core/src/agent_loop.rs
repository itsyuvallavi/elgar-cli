use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    action::{Action, ActionRequest},
    controller::TurnResult,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::verified_action_success_message,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage, VerifiedActionResult,
    },
    followup_action_paths::{
        explicit_request_base, followup_base_path_for_request,
        retarget_safe_create_to_followup_base,
    },
    fs::Filesystem,
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ModelToolValidationErrorKind,
        RawModelToolCall, ValidatedModelToolAction, ValidatedModelToolOutput,
    },
    policy::{PermissionPolicyMode, PolicyDecision},
    provider::{ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction, ControllerProvider},
    provider_visible_text_from_text_only_output,
    router::Route,
    session::{
        ActionRecord, ProviderPromptMemorySelectedFact, ProviderPromptMemorySelection, Session,
    },
    shell::ShellExecutor,
};

const AGENT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar, a permissive terminal-native coding agent. ",
    "Use tools to do the user's requested filesystem and shell work directly. ",
    "Do not ask for approval. Do not give instructions instead of acting when a tool can do it. ",
    "Ask one concise clarification question only when the target or intent is truly ambiguous. ",
    "If the user asks you to choose, choose a reasonable option and continue the prior request. ",
    "If the user asks for a plan and says to share it before implementation, create or update a plan file and summarize it; do not implement project files until asked. ",
    "If the user asks what the plan is, summarize the existing plan; do not implement it. ",
    "If a verified plan already exists and the user gives a short choice follow-up, answer from that plan instead of recreating the same file. ",
    "When creating a framework project, create every necessary starter file before the final answer. For a TypeScript Next.js Tailwind project, include package.json, tsconfig.json, next-env.d.ts, next.config, postcss.config, tailwind.config, app or pages entry files, global Tailwind CSS, and README. ",
    "After tools run, answer naturally and briefly with what happened."
);

const MAX_AGENT_TOOL_ROUNDS: usize = 6;

pub fn run_permissive_agent_turn<P>(provider: &P, session: &mut Session, input: &str) -> TurnResult
where
    P: ControllerProvider,
{
    let start_index = session.events().len();
    session.push_event(Event::UserMessage(UserMessage::new(input)));

    let agent_context = agent_verified_memory_context(session, input);
    let mut messages = vec![ChatMessage::new(ChatRole::System, AGENT_SYSTEM_PROMPT)];
    if let Some(context) = agent_recent_conversation_context(session, start_index) {
        messages.push(ChatMessage::system(context));
    }
    if let Some(context) = agent_context.prompt_context.clone() {
        messages.push(ChatMessage::system(context));
    }
    messages.push(ChatMessage::user(input));
    let tools = elgar_model_tool_definitions();
    let mut handled_tool_call_ids = HashSet::new();

    for _round in 0..MAX_AGENT_TOOL_ROUNDS {
        let request = provider.request_metadata();
        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        let output = match provider.chat_messages_with_tools_with_metadata(
            messages.clone(),
            &request,
            tools.clone(),
        ) {
            Ok(output) => output,
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
                break;
            }
        };

        let mut tool_calls = output.tool_calls.clone();
        let assistant_text = output.text.clone();
        session.push_event(Event::ProviderFinished(ProviderFinished::new(
            request.provider,
            request.request_id,
            output,
        )));

        if tool_calls.is_empty() {
            push_provider_message_if_visible(session, assistant_text);
            break;
        }

        tool_calls.retain(|tool_call| handled_tool_call_ids.insert(tool_call.id.clone()));
        if tool_calls.is_empty() {
            push_provider_message_if_visible(session, assistant_text);
            break;
        }

        messages.push(chat_assistant_tool_call_message(
            assistant_text,
            &tool_calls,
        ));

        let outputs = match validate_model_tool_outputs(&tool_calls) {
            Ok(outputs) => outputs,
            Err(error) => {
                let active_project_base = agent_context.active_project_base();
                let message = recoverable_tool_validation_message(
                    &error.message,
                    &error,
                    active_project_base.as_deref(),
                );
                if !is_recoverable_edit_target_validation_error(&error)
                    || active_project_base.is_none()
                {
                    session.push_event(Event::Error(ErrorEvent::new(message.clone())));
                }
                for tool_call in tool_calls {
                    messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                }
                continue;
            }
        };

        for output in outputs {
            match output {
                ValidatedModelToolOutput::Guidance(guidance) => {
                    push_provider_message_if_visible(session, guidance.question.clone());
                    messages.push(ChatMessage::tool(guidance.tool_call_id, guidance.question));
                }
                ValidatedModelToolOutput::Action(action) => {
                    let action = retarget_agent_action(
                        action,
                        agent_context.requested_project_base.as_deref(),
                        agent_context.followup_base.as_deref(),
                        &session.project_root,
                    );
                    let result =
                        apply_permissive_agent_action(session, action.request, action.summary);
                    messages.push(ChatMessage::tool(action.tool_call_id, result));
                }
            }
        }
    }

    TurnResult {
        route: Route::AskModel,
        events: session.events()[start_index..].to_vec(),
    }
}

fn apply_permissive_agent_action(
    session: &mut Session,
    request: ActionRequest,
    summary: String,
) -> String {
    let action = Action::proposed(next_action_id(session), request, summary).approve();
    let policy_decision = PolicyDecision::allow_apply(
        PermissionPolicyMode::FullAccess,
        "permissive agent tool loop executed the model tool call directly",
    );
    let approval_source = policy_decision.approval_source.clone();
    let index = session.actions().len();
    let mut record = ActionRecord::new(action.clone());
    record.policy_decision = Some(policy_decision);
    session.push_action(record);

    let mut approved_event =
        ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
            .with_target(action.request.approval_target());
    if let Some(source) = approval_source {
        approved_event = approved_event.with_approval_source(source);
    }
    session.push_event(Event::ActionApproved(approved_event));

    let result: Result<VerifiedActionResult, String> = match &action.request {
        ActionRequest::ShellCommand(shell) => {
            ShellExecutor::execute(shell).map_err(|error| error.to_string())
        }
        _ => Filesystem::apply_file_action(&action, permissive_allowed_root(session, &action))
            .map_err(|error| error.to_string()),
    };

    match result {
        Ok(result) => record_agent_action_success(session, index, &action, result),
        Err(reason) => {
            let record = session
                .action_mut(index)
                .expect("agent action index must reference an action record");
            record.verified_result = None;
            record.failure_reason = Some(reason.clone());
            record.action = action.mark_failed();
            session.push_event(Event::ActionFailed(ActionFailed::new(
                action.id.clone(),
                action.kind(),
                reason.clone(),
            )));
            format!("Tool failed: {reason}")
        }
    }
}

fn record_agent_action_success(
    session: &mut Session,
    index: usize,
    action: &Action,
    result: VerifiedActionResult,
) -> String {
    let message = verified_action_success_message(session, action, &result);
    let record = session
        .action_mut(index)
        .expect("agent action index must reference an action record");
    record.verified_result = Some(result.clone());
    record.failure_reason = None;
    record.action = action.clone().mark_applied();
    record_verified_project_memory(session, action, &result);
    session.push_event(Event::ActionApplied(ActionApplied::new(
        action.id.clone(),
        action.kind(),
        result,
    )));
    message
}

fn permissive_allowed_root(session: &Session, action: &Action) -> PathBuf {
    let Some(target_path) = action_filesystem_target(action) else {
        return session.cwd.clone();
    };

    if target_path.is_absolute() {
        if let Some(home) = home_dir() {
            if target_path.starts_with(&home) {
                return home;
            }
        }
        return target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| session.cwd.clone());
    }

    session.cwd.clone()
}

fn action_filesystem_target(action: &Action) -> Option<&Path> {
    match &action.request {
        ActionRequest::CreateFile(create_file) => Some(&create_file.target_path),
        ActionRequest::CreateDirectory(create_directory) => Some(&create_directory.target_path),
        ActionRequest::PatchFile(patch_file) => Some(&patch_file.target_path),
        ActionRequest::OverwriteFile(overwrite_file) => Some(&overwrite_file.target_path),
        ActionRequest::DeleteFile(delete_file) => Some(&delete_file.target_path),
        ActionRequest::MoveFile(move_file) => Some(&move_file.target_path),
        ActionRequest::ShellCommand(_) => None,
    }
}

fn chat_assistant_tool_call_message(
    content: String,
    tool_calls: &[RawModelToolCall],
) -> ChatMessage {
    ChatMessage::assistant(content).with_tool_calls(
        tool_calls
            .iter()
            .map(|tool_call| ChatToolCall {
                id: tool_call.id.clone(),
                tool_type: "function".to_string(),
                function: ChatToolCallFunction {
                    name: tool_call.name.raw_label(),
                    arguments: arguments_json_string(&tool_call.arguments),
                },
            })
            .collect(),
    )
}

fn arguments_json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn push_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message = message.into();
    if let Some(message) = provider_visible_text_from_text_only_output(message) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            message,
            AssistantMessageSource::Provider,
        )));
    }
}

#[derive(Debug, Clone, Default)]
struct AgentVerifiedMemoryContext {
    prompt_context: Option<String>,
    followup_base: Option<PathBuf>,
    requested_project_base: Option<PathBuf>,
}

impl AgentVerifiedMemoryContext {
    fn active_project_base(&self) -> Option<PathBuf> {
        self.requested_project_base
            .clone()
            .or_else(|| self.followup_base.clone())
    }
}

fn recoverable_tool_validation_message(
    message: &str,
    error: &crate::model_runtime::ModelToolValidationError,
    active_project_base: Option<&Path>,
) -> String {
    if !is_recoverable_edit_target_validation_error(error) {
        return message.to_string();
    }

    let Some(base) = active_project_base else {
        return format!("{message}. Ask the user which project file to edit.");
    };

    format!(
        "{message}. Retry inside the active project root `{}` with a concrete project file path, or ask_guidance if the target is unclear.",
        base.display()
    )
}

fn is_recoverable_edit_target_validation_error(
    error: &crate::model_runtime::ModelToolValidationError,
) -> bool {
    matches!(
        error.kind,
        ModelToolValidationErrorKind::MissingArgument
            | ModelToolValidationErrorKind::MalformedArgument
    ) && error.argument.as_deref() == Some("target_path")
        && matches!(
            error.tool_name.as_deref(),
            Some("patch_file" | "overwrite_file" | "delete_file")
        )
}

fn agent_verified_memory_context(session: &mut Session, input: &str) -> AgentVerifiedMemoryContext {
    let normalized = input.to_ascii_lowercase();
    let short_followup = mentions_short_followup(&normalized);
    let repair_followup = mentions_project_repair_followup(&normalized);
    let folder_followup = mentions_followup_folder(&normalized);
    let plan_followup = mentions_followup_plan(&normalized);
    let needs_folder = short_followup || repair_followup || folder_followup;
    let needs_plan = short_followup || repair_followup || folder_followup || plan_followup;
    let explicit_base = explicit_request_base(input, home_dir());
    let requested_project_base = requested_project_base(input, explicit_base.as_deref());
    let followup_base =
        explicit_base.or_else(|| followup_base_path_for_request(session, needs_folder, needs_plan));

    let mut selected = Vec::new();
    let mut lines = Vec::new();
    if needs_folder || needs_plan {
        if let Some(folder) = session.project_memory().latest_verified_folder() {
            lines.push(format!(
                "- latest verified folder: {}",
                display_project_path(session, &folder.path)
            ));
            selected.push(ProviderPromptMemorySelectedFact::new(
                "verified_folder",
                folder.path.clone(),
                None,
                folder.source_action_id.clone(),
            ));
        }
        if let Some(plan) = session.project_memory().latest_verified_plan() {
            lines.push(format!(
                "- latest verified plan: {}",
                display_project_path(session, &plan.path)
            ));
            if let Some(excerpt) = verified_plan_excerpt(&plan.path) {
                lines.push(format!("- latest verified plan excerpt:\n{excerpt}"));
            }
            selected.push(ProviderPromptMemorySelectedFact::new(
                "verified_plan",
                plan.path.clone(),
                Some(plan.project_root.clone()),
                plan.source_action_id.clone(),
            ));
        }
    }

    if selected.is_empty() {
        session.set_latest_provider_prompt_memory_selection(None);
    } else {
        session.set_latest_provider_prompt_memory_selection(Some(
            ProviderPromptMemorySelection::new(selected, Vec::new()),
        ));
    }

    let prompt_context = if lines.is_empty() {
        None
    } else {
        let mut context = vec![
            "Verified filesystem context for this session:".to_string(),
            "Use these paths for references such as same folder, that folder, or the folder you created.".to_string(),
        ];
        context.extend(lines);
        Some(context.join("\n"))
    };

    AgentVerifiedMemoryContext {
        prompt_context,
        followup_base,
        requested_project_base,
    }
}

fn mentions_followup_folder(normalized: &str) -> bool {
    normalized.contains("same folder")
        || normalized.contains("last folder")
        || normalized.contains("latest folder")
        || normalized.contains("that folder")
        || normalized.contains("this folder")
        || normalized.contains("the folder")
        || normalized.contains("folder you created")
        || normalized.contains("folder you just created")
        || normalized.contains("folder we created")
        || normalized.contains("where did you put")
}

fn mentions_followup_plan(normalized: &str) -> bool {
    normalized.contains("the plan")
        || normalized.contains("that plan")
        || normalized.contains("same plan")
        || normalized.contains("last plan")
        || normalized.contains("latest plan")
        || normalized.contains("plan you")
        || normalized.contains("plan we")
        || normalized.contains("according to plan")
        || normalized.contains("according to the plan")
        || normalized.contains("implement plan")
        || normalized.contains("implement the plan")
        || normalized.contains("execute plan")
        || normalized.contains("execute the plan")
        || normalized.contains("create project according")
        || normalized.contains("create the project")
}

fn requested_project_base(input: &str, location_base: Option<&Path>) -> Option<PathBuf> {
    let normalized = input.to_ascii_lowercase();
    if !(normalized.contains("project") || normalized.contains("app")) {
        return None;
    }

    let name = requested_name_after(input, "call it")
        .or_else(|| requested_name_after(input, "call the folder"))
        .or_else(|| requested_name_after(input, "called"))
        .or_else(|| requested_name_after(input, "name it"))?;
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return Some(path);
    }

    Some(match location_base {
        Some(base) => base.join(path),
        None => path,
    })
}

fn requested_name_after(input: &str, marker: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let start = lower.find(marker)? + marker.len();
    let rest = input.get(start..)?.trim();
    let rest_lower = rest.to_ascii_lowercase();
    let mut end = rest.len();

    for delimiter in [
        " under ", " inside ", " in ", " on ", " at ", ",", ".", "?", "!",
    ] {
        if let Some(index) = rest_lower.find(delimiter) {
            end = end.min(index);
        }
    }

    let name = rest[..end]
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'))
        .trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        None
    } else {
        Some(name.to_string())
    }
}

fn mentions_short_followup(normalized: &str) -> bool {
    matches!(
        normalized.trim_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace()),
        "your choice"
            | "up to you"
            | "whatever you want"
            | "whatever you think"
            | "you choose"
            | "choose"
            | "go ahead"
            | "yes"
            | "ok"
            | "okay"
    )
}

fn mentions_project_repair_followup(normalized: &str) -> bool {
    let mentions_missing_work = normalized.contains("forgot")
        || normalized.contains("missing")
        || normalized.contains("rest of the")
        || normalized.contains("what about the rest")
        || normalized.contains("not complete")
        || normalized.contains("incomplete")
        || normalized.contains("finish it")
        || normalized.contains("finish the project")
        || normalized.contains("complete it")
        || normalized.contains("complete the project");
    let mentions_files_or_project = normalized.contains("file")
        || normalized.contains("project")
        || normalized.contains("scaffold")
        || normalized.contains("pages/")
        || normalized.contains("src/")
        || normalized.contains("tailwind")
        || normalized.contains("next");

    mentions_missing_work && mentions_files_or_project
}

fn agent_recent_conversation_context(session: &Session, end_index: usize) -> Option<String> {
    let mut lines = Vec::new();
    for event in session.events()[..end_index].iter().rev() {
        match event {
            Event::UserMessage(message) => {
                lines.push(format!("User: {}", compact_context_line(&message.content)));
            }
            Event::AssistantMessage(message) => {
                lines.push(format!("Elgar: {}", compact_context_line(&message.content)));
            }
            Event::ActionApplied(applied) => {
                lines.push(format!(
                    "Verified action: {}",
                    compact_context_line(&verified_result_context(&applied.result))
                ));
            }
            Event::ActionFailed(failed) => {
                lines.push(format!(
                    "Failed action: {}",
                    compact_context_line(&failed.reason)
                ));
            }
            _ => {}
        }

        if lines.len() >= 12 {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    Some(format!(
        "Recent conversation context. Use this to resolve short follow-ups like `your choice`, `same folder`, `last folder`, and `the plan`:\n{}",
        lines.join("\n")
    ))
}

fn verified_result_context(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => format!("wrote {path}"),
        VerifiedActionResult::File(verification) => match verification {
            crate::event::FileActionVerification::FileCreated { path } => {
                format!("created file {path}")
            }
            crate::event::FileActionVerification::FilePatched { path } => {
                format!("patched file {path}")
            }
            crate::event::FileActionVerification::FileOverwritten { path } => {
                format!("overwrote file {path}")
            }
            crate::event::FileActionVerification::FileDeleted { path } => {
                format!("deleted file {path}")
            }
            crate::event::FileActionVerification::FileMoved {
                source_path,
                target_path,
            } => format!("moved file {source_path} to {target_path}"),
            crate::event::FileActionVerification::DirectoryCreated { path } => {
                format!("created directory {path}")
            }
        },
        VerifiedActionResult::Shell(verification) => verification
            .verified_effect
            .clone()
            .unwrap_or_else(|| format!("shell command finished in {}", verification.cwd)),
    }
}

fn compact_context_line(value: &str) -> String {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 260;
    if line.len() <= LIMIT {
        line
    } else {
        format!("{}...", &line[..LIMIT])
    }
}

fn verified_plan_excerpt(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let contents = contents.trim();
    if contents.is_empty() {
        return None;
    }

    const LIMIT: usize = 1200;
    if contents.len() <= LIMIT {
        Some(contents.to_string())
    } else {
        Some(format!("{}...", &contents[..LIMIT]))
    }
}

fn display_project_path(session: &Session, path: &Path) -> String {
    path.strip_prefix(&session.project_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn retarget_agent_action(
    action: ValidatedModelToolAction,
    requested_project_base: Option<&Path>,
    followup_base: Option<&Path>,
    workspace_root: &Path,
) -> ValidatedModelToolAction {
    if let Some(base) = requested_project_base {
        return retarget_action_to_project_base(base, Some(workspace_root), action);
    }

    if let Some(base) = followup_base {
        return retarget_action_to_project_base(base, Some(workspace_root), action);
    }

    retarget_safe_create_to_followup_base(followup_base, action)
}

fn retarget_action_to_project_base(
    base: &Path,
    workspace_root: Option<&Path>,
    mut validated: ValidatedModelToolAction,
) -> ValidatedModelToolAction {
    match &mut validated.request {
        ActionRequest::CreateFile(create_file) => {
            if let Some(target_path) =
                project_base_target_path(&create_file.target_path, base, workspace_root)
            {
                create_file.target_path = target_path;
            }
        }
        ActionRequest::CreateDirectory(create_directory) => {
            if let Some(target_path) =
                project_base_target_path(&create_directory.target_path, base, workspace_root)
            {
                create_directory.target_path = target_path;
            }
        }
        ActionRequest::PatchFile(patch_file) => {
            if let Some(target_path) =
                project_base_target_path(&patch_file.target_path, base, workspace_root)
            {
                patch_file.target_path = target_path;
            }
        }
        ActionRequest::OverwriteFile(overwrite_file) => {
            if let Some(target_path) =
                project_base_target_path(&overwrite_file.target_path, base, workspace_root)
            {
                overwrite_file.target_path = target_path;
            }
        }
        ActionRequest::DeleteFile(delete_file) => {
            if let Some(target_path) =
                project_base_target_path(&delete_file.target_path, base, workspace_root)
            {
                delete_file.target_path = target_path;
            }
        }
        ActionRequest::MoveFile(move_file) => {
            if let Some(source_path) =
                project_base_target_path(&move_file.source_path, base, workspace_root)
            {
                move_file.source_path = source_path;
            }
            if let Some(target_path) =
                project_base_target_path(&move_file.target_path, base, workspace_root)
            {
                move_file.target_path = target_path;
            }
        }
        _ => return validated,
    }

    validated.target_label = validated.request.approval_target();
    validated
}

fn project_base_target_path(
    target_path: &Path,
    base: &Path,
    workspace_root: Option<&Path>,
) -> Option<PathBuf> {
    if target_path.starts_with(base) {
        return None;
    }

    if target_path.is_absolute() {
        if let Some(workspace_root) = workspace_root {
            if let Ok(relative) = target_path.strip_prefix(workspace_root) {
                return project_base_target_path(relative, base, None);
            }
        }
        if let Some(target_path) = sibling_project_target_path(target_path, base) {
            return Some(target_path);
        }
        return strip_base_suffix_prefix(target_path, base).map(|suffix| {
            if suffix.as_os_str().is_empty() {
                base.to_path_buf()
            } else {
                base.join(suffix)
            }
        });
    }

    if let Some(suffix) = strip_base_suffix_prefix(target_path, base) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    if let Some(suffix) = strip_repeated_base_name_prefix_for_agent(target_path, base) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    if let Some(suffix) = strip_generic_project_root_prefix(target_path) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    Some(base.join(target_path))
}

fn sibling_project_target_path(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let parent = base.parent()?;
    let relative = target_path.strip_prefix(parent).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    let std::path::Component::Normal(first) = first else {
        return None;
    };
    if Some(first) == base.file_name() || !is_generic_project_root_component(first) {
        return None;
    }
    let suffix = components.as_path();
    Some(if suffix.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(suffix)
    })
}

fn is_generic_project_root_component(value: &std::ffi::OsStr) -> bool {
    let value = value.to_string_lossy().to_ascii_lowercase();
    matches!(
        value.as_str(),
        "project"
            | "app"
            | "my-app"
            | "my-next-app"
            | "my-nextapp"
            | "my-nextjs-app"
            | "react-app"
            | "react-project"
            | "vite-project"
    )
}

fn strip_base_suffix_prefix(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let target_components = normal_path_components(target_path);
    let base_components = normal_path_components(base);
    for start in 0..base_components.len() {
        let base_suffix = &base_components[start..];
        if base_suffix.is_empty() || target_components.len() < base_suffix.len() {
            continue;
        }
        for target_start in 0..=target_components.len() - base_suffix.len() {
            let target_end = target_start + base_suffix.len();
            if target_components[target_start..target_end] == base_suffix[..] {
                return Some(target_components[target_end..].iter().collect());
            }
        }
    }
    None
}

fn normal_path_components(path: &Path) -> Vec<std::ffi::OsString> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect()
}

fn strip_repeated_base_name_prefix_for_agent(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let base_name = base.file_name()?;
    let suffix = target_path.strip_prefix(Path::new(base_name)).ok()?;
    Some(suffix.to_path_buf())
}

fn strip_generic_project_root_prefix(target_path: &Path) -> Option<PathBuf> {
    let mut components = target_path.components();
    let first = components.next()?;
    let first = first.as_os_str().to_string_lossy().to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "project"
            | "my-nextjs-app"
            | "nextjs-app"
            | "next-app"
            | "nextjs-project"
            | "next-tailwind-project"
            | "next-tailwind-ts-project"
            | "next-tailwind-app"
    ) {
        return Some(components.as_path().to_path_buf());
    }

    None
}

fn next_action_id(session: &Session) -> String {
    format!("action-{}", session.actions().len() + 1)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        model_runtime::{ModelToolName, RawModelToolName},
        provider::{ChatToolDefinition, ProviderError, ProviderRequestMetadata},
    };

    #[derive(Debug, Clone)]
    struct SequenceProvider {
        outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
        messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<ChatMessage>>>>,
    }

    impl SequenceProvider {
        fn new(outputs: Vec<crate::event::ProviderOutput>) -> Self {
            Self {
                outputs: std::sync::Arc::new(std::sync::Mutex::new(outputs)),
                messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    impl ControllerProvider for SequenceProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("sequence", None, "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Err(ProviderError::configuration("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.messages.lock().unwrap().push(messages);
            Ok(self.outputs.lock().unwrap().remove(0))
        }
    }

    #[test]
    fn permissive_agent_turn_executes_tool_call_and_continues() {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "demo" }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Created demo."),
        ]);
        let mut session = Session::new("session", &root, &root);

        let result = run_permissive_agent_turn(&provider, &mut session, "create a folder demo");

        assert_eq!(result.route, Route::AskModel);
        assert!(root.join("demo").is_dir());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Applied
        );
        let messages = provider.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[1]
            .iter()
            .any(|message| matches!(message.role, ChatRole::Tool)));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_tool_call_turn_does_not_render_tool_planning_text_as_chat() {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool-text", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new(
                "We need to create the folder and write files. Let's implement.",
            )
            .with_tool_calls(vec![RawModelToolCall {
                id: "call-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "demo" }),
                assistant_summary: None,
            }]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "create demo");

        assert!(root.join("demo").is_dir());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("We need to create the folder")
                    || message.content.contains("Let's implement")
        )));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message) if message.content == "Done."
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[derive(Debug, Clone)]
    struct HistoryAwarePlanProvider;

    impl ControllerProvider for HistoryAwarePlanProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("history-aware", None, "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Err(ProviderError::configuration("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, ChatRole::Tool))
            {
                return Ok(crate::event::ProviderOutput::new("Done."));
            }

            let latest_user = messages
                .iter()
                .rev()
                .find(|message| matches!(message.role, ChatRole::User))
                .map(|message| message.content.as_str())
                .unwrap_or_default();
            let joined_context = messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            if latest_user.contains("helloworld") {
                return Ok(crate::event::ProviderOutput::new("Creating helloworld.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "history-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: json!({ "target_path": "helloworld" }),
                        assistant_summary: Some("create helloworld".to_string()),
                    }]));
            }

            if latest_user.contains("python and ts project") {
                return Ok(crate::event::ProviderOutput::new(
                    "What type of Python and TypeScript project would you like?",
                ));
            }

            if latest_user == "your choice" {
                assert!(joined_context.contains(
                    "create a plan for a python and ts project in the last folder you created"
                ));
                assert!(joined_context.contains("latest verified folder: helloworld"));
                return Ok(crate::event::ProviderOutput::new("Creating the plan.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "history-call-2".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "python-ts-project-plan.md",
                            "contents": "# Python + TypeScript Project Plan\n\n- Build a FastAPI backend.\n- Build a TypeScript CLI client.\n- Share API types through JSON schema.\n"
                        }),
                        assistant_summary: Some("create Python + TypeScript project plan".to_string()),
                    }]));
            }

            if latest_user.contains("plan i asked") {
                assert!(joined_context
                    .contains("latest verified plan: helloworld/python-ts-project-plan.md"));
                assert!(joined_context.contains("FastAPI backend"));
                return Ok(crate::event::ProviderOutput::new(
                    "The plan is a FastAPI backend plus a TypeScript CLI client.",
                ));
            }

            Ok(crate::event::ProviderOutput::new("Unhandled."))
        }
    }

    #[test]
    fn permissive_agent_turn_carries_recent_plan_request_into_short_followup() {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-loop-{}-history", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = HistoryAwarePlanProvider;
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "create a folder called helloworld");
        assert!(root.join("helloworld").is_dir());

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a plan for a python and ts project in the last folder you created, it can be whatever you want as long as it uses for python and TS. make sure to share it with me before to implement the plan.",
        );
        assert!(!root.join("helloworld/python-ts-project-plan.md").exists());

        run_permissive_agent_turn(&provider, &mut session, "your choice");
        let plan_path = root.join("helloworld/python-ts-project-plan.md");
        assert!(plan_path.is_file());
        assert!(!root.join("python-ts-project-plan.md").exists());

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "whats the plan i asked you to create??",
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("FastAPI backend plus a TypeScript CLI client")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[derive(Debug, Clone)]
    struct RepairFollowupProvider;

    impl ControllerProvider for RepairFollowupProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("repair-followup", None, "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Err(ProviderError::configuration("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, ChatRole::Tool))
            {
                return Ok(crate::event::ProviderOutput::new("Done."));
            }

            let latest_user = messages
                .iter()
                .rev()
                .find(|message| matches!(message.role, ChatRole::User))
                .map(|message| message.content.as_str())
                .unwrap_or_default();
            let joined_context = messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            if latest_user.contains("forgot") {
                assert!(joined_context.contains("latest verified plan: app/project-plan.md"));
                return Ok(crate::event::ProviderOutput::new(
                    "We need create pages/index.tsx, styles/globals.css, tailwind config.",
                )
                .with_tool_calls(vec![
                    RawModelToolCall {
                        id: "repair-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "./pages/index.tsx",
                            "contents": "export default function Home() { return <main>Hello</main>; }\n"
                        }),
                        assistant_summary: Some("create missing homepage".to_string()),
                    },
                    RawModelToolCall {
                        id: "repair-call-2".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "./styles/globals.css",
                            "contents": "@tailwind base;\n@tailwind components;\n@tailwind utilities;\n"
                        }),
                        assistant_summary: Some("create missing global styles".to_string()),
                    },
                ]));
            }

            Ok(crate::event::ProviderOutput::new("Unhandled."))
        }
    }

    #[test]
    fn permissive_agent_repair_followup_targets_latest_project_folder() {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-loop-{}-repair", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let plan_path = root.join("app/project-plan.md");
        std::fs::write(
            &plan_path,
            "# Project Plan\n\n- Create pages/index.tsx.\n- Create styles/globals.css.\n",
        )
        .unwrap();
        let provider = RepairFollowupProvider;
        let mut session = Session::new("session", &root, &root);

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("app/project-plan.md"),
                contents: String::new(),
            }),
            "create project plan",
        )
        .approve()
        .mark_applied();
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileCreated {
                path: "app/project-plan.md".to_string(),
            },
        ));
        session.push_action(plan_record);
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "app/project-plan.md".to_string(),
            }),
        );

        run_permissive_agent_turn(&provider, &mut session, "i think you forgot some files");

        assert!(root.join("app/pages/index.tsx").is_file());
        assert!(root.join("app/styles/globals.css").is_file());
        assert!(!root.join("pages/index.tsx").exists());
        assert!(!root.join("styles/globals.css").exists());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message) if message.content.contains("We need create")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_new_project_request_does_not_use_stale_verified_plan_memory() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-fresh-project",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("old-project")).unwrap();
        std::fs::write(
            root.join("old-project/project-plan.md"),
            "# Old Project Plan\n\n- This should not affect unrelated new project requests.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating the requested project.").with_tool_calls(
                vec![
                    RawModelToolCall {
                        id: "fresh-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: json!({ "target_path": "project" }),
                        assistant_summary: None,
                    },
                    RawModelToolCall {
                        id: "fresh-call-2".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "project/package.json",
                            "contents": "{}\n"
                        }),
                        assistant_summary: None,
                    },
                ],
            ),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("old-project/project-plan.md"),
                contents: String::new(),
            }),
            "create old project plan",
        )
        .approve()
        .mark_applied();
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileCreated {
                path: "old-project/project-plan.md".to_string(),
            },
        ));
        session.push_action(plan_record);
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "old-project/project-plan.md".to_string(),
            }),
        );

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "can you create a nextjs project using tailwind and ts? call it FreshNextApp under this repo",
        );

        assert!(root.join("FreshNextApp/package.json").is_file());
        assert!(!root.join("project/package.json").exists());
        assert!(!root.join("old-project/FreshNextApp/package.json").exists());
        let messages = provider.messages.lock().unwrap();
        let first_request = messages
            .first()
            .expect("provider should receive the first request")
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!first_request.contains("latest verified plan"));
        assert!(!first_request.contains("Old Project Plan"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn requested_project_base_extracts_explicit_names_and_locations() {
        assert_eq!(
            requested_project_base(
                "can you create a nextjs project using tailwind and ts? call it FreshNextApp under this repo",
                None,
            ),
            Some(PathBuf::from("FreshNextApp"))
        );
        assert_eq!(
            requested_project_base(
                "create a TS app called Demo123 on the desktop",
                Some(Path::new("/Users/yuval/Desktop")),
            ),
            Some(PathBuf::from("/Users/yuval/Desktop/Demo123"))
        );
        assert_eq!(
            requested_project_base(
                "create a react project using tailwind and TS in the desktop, call the folder TEST",
                Some(Path::new("/Users/yuval/Desktop")),
            ),
            Some(PathBuf::from("/Users/yuval/Desktop/TEST"))
        );
        assert_eq!(
            requested_project_base("create a folder called notes on the desktop", None),
            None
        );
    }

    #[test]
    fn requested_project_target_path_retargets_generic_model_roots() {
        let base = Path::new("FreshNextApp");

        assert_eq!(
            project_base_target_path(Path::new("project"), base, None),
            Some(PathBuf::from("FreshNextApp"))
        );
        assert_eq!(
            project_base_target_path(Path::new("project/package.json"), base, None),
            Some(PathBuf::from("FreshNextApp/package.json"))
        );
        assert_eq!(
            project_base_target_path(Path::new("my-nextjs-app/tsconfig.json"), base, None),
            Some(PathBuf::from("FreshNextApp/tsconfig.json"))
        );
        assert_eq!(
            project_base_target_path(Path::new("app/page.tsx"), base, None),
            Some(PathBuf::from("FreshNextApp/app/page.tsx"))
        );
        assert_eq!(
            project_base_target_path(Path::new("FreshNextApp/package.json"), base, None),
            None
        );
    }

    #[test]
    fn requested_desktop_project_target_path_does_not_duplicate_desktop_or_folder() {
        let base = Path::new("/Users/yuval/Desktop/TEST");
        let workspace = Path::new("/Users/yuval/__git/elgar");

        assert_eq!(
            project_base_target_path(Path::new("Desktop/TEST"), base, Some(workspace)),
            Some(PathBuf::from("/Users/yuval/Desktop/TEST"))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("Desktop/TEST/package.json"),
                base,
                Some(workspace)
            ),
            Some(PathBuf::from("/Users/yuval/Desktop/TEST/package.json"))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/Desktop/Desktop/TEST/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/Desktop/project/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/__git/elgar/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
    }

    #[test]
    fn permissive_agent_exact_named_react_project_targets_verified_folder() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-named-react",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let repo_tailwind = repo.join("tailwind.config.js");
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating TEST.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "named-react-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "project" }),
                    assistant_summary: None,
                },
                RawModelToolCall {
                    id: "named-react-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "project/package.json",
                        "contents": "{\"name\":\"test\",\"devDependencies\":{\"tailwindcss\":\"latest\",\"typescript\":\"latest\"}}\n"
                    }),
                    assistant_summary: None,
                },
                RawModelToolCall {
                    id: "named-react-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": repo_tailwind.display().to_string(),
                        "contents": "module.exports = { content: [] };\n"
                    }),
                    assistant_summary: None,
                },
                RawModelToolCall {
                    id: "named-react-4".to_string(),
                    name: RawModelToolName::Known(ModelToolName::PatchFile),
                    arguments: json!({
                        "target_path": repo_tailwind.display().to_string(),
                        "find": "content: []",
                        "replace": "content: ['./index.html', './src/**/*.{ts,tsx}']"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &repo, &repo);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a react project using tailwind and TS, call the folder TEST",
        );

        let project = repo.join("TEST");
        assert!(project.is_dir());
        assert!(project.join("package.json").is_file());
        assert_eq!(
            std::fs::read_to_string(project.join("tailwind.config.js")).unwrap(),
            "module.exports = { content: ['./index.html', './src/**/*.{ts,tsx}'] };\n"
        );
        assert!(!repo.join("project").exists());
        assert!(!repo.join("tailwind.config.js").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recoverable_missing_patch_target_is_retried_without_error_event() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-patch-recovery",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Patching config.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-target-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::PatchFile),
                    arguments: json!({
                        "find": "content: []",
                        "replace": "content: ['./src/**/*.{ts,tsx}']"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Creating config instead.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-target-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "tailwind.config.js",
                        "contents": "module.exports = { content: ['./src/**/*.{ts,tsx}'] };\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &repo, &repo);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a react project using tailwind and TS, call the folder TEST",
        );

        assert!(repo.join("TEST/tailwind.config.js").is_file());
        assert!(!repo.join("tailwind.config.js").exists());
        assert!(!session
            .events()
            .iter()
            .any(|event| matches!(event, Event::Error(error) if error.message.contains("missing required argument"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[derive(Debug, Clone)]
    struct CapturingProvider {
        messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<ChatMessage>>>>,
    }

    impl CapturingProvider {
        fn new() -> Self {
            Self {
                messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    impl ControllerProvider for CapturingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("capture", None, "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Err(ProviderError::configuration("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.messages.lock().unwrap().push(messages);
            Ok(crate::event::ProviderOutput::new("I'll create it."))
        }
    }

    #[test]
    fn permissive_agent_prompt_names_complete_next_tailwind_scaffold_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-next-tailwind-prompt",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a TS Next.js and Tailwind project in ~/next-tailwind-ts-project",
        );

        let messages = provider.messages.lock().unwrap();
        let system_prompt = &messages[0][0].content;
        for expected in [
            "TypeScript Next.js Tailwind",
            "package.json",
            "tsconfig.json",
            "next-env.d.ts",
            "postcss.config",
            "tailwind.config",
            "global Tailwind CSS",
            "README",
        ] {
            assert!(system_prompt.contains(expected), "missing {expected}");
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
