use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    action::{Action, ActionRequest, CreateFileAction, OverwriteFileAction},
    controller::TurnResult,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::verified_action_success_message,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage, VerifiedActionResult,
    },
    followup_action_paths::{explicit_request_base, followup_base_path_for_request},
    fs::Filesystem,
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ModelToolValidationErrorKind,
        RawModelToolCall, ValidatedModelGuidanceRequest, ValidatedModelToolAction,
        ValidatedModelToolOutput,
    },
    path_resolution::{allowed_root_for_action, resolve_agent_action_paths, AgentPathResolution},
    policy::{PermissionPolicyMode, PolicyDecision},
    provider::{ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction, ControllerProvider},
    provider_visible_text_from_text_only_output,
    router::Route,
    session::{
        ActionRecord, PendingActionSelection, ProviderPromptMemorySelectedFact,
        ProviderPromptMemorySelection, Session,
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
    run_agent_turn_with_policy(provider, session, input, PermissionPolicyMode::FullAccess)
}

pub fn run_agent_turn_with_policy<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    policy_mode: PermissionPolicyMode,
) -> TurnResult
where
    P: ControllerProvider,
{
    let start_index = session.events().len();
    session.push_event(Event::UserMessage(UserMessage::new(input)));

    if let Some(message) = repeated_plan_create_response(session, input) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            message,
            AssistantMessageSource::Controller,
        )));
        return TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        };
    }

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

        let path_resolution = agent_context.path_resolution(&session.project_root);
        let resolved_outputs = sanitize_plan_execution_outputs(
            session,
            input,
            resolve_agent_tool_outputs(outputs, &path_resolution),
        );
        let resolved_outputs = match resolved_outputs {
            PlanExecutionSanitization::Outputs(outputs) => {
                repair_directory_only_file_request(input, outputs)
            }
            PlanExecutionSanitization::Retry(message) => {
                for tool_call in tool_calls {
                    messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                }
                continue;
            }
        };

        if policy_mode == PermissionPolicyMode::ReviewAll {
            if let Some(action) = review_required_action_to_propose(&resolved_outputs, policy_mode)
            {
                apply_agent_action_with_policy(
                    session,
                    action.request.clone(),
                    action.summary.clone(),
                    policy_mode,
                );
                return TurnResult {
                    route: Route::AskModel,
                    events: session.events()[start_index..].to_vec(),
                };
            }
        }

        for output in resolved_outputs {
            match output {
                ResolvedAgentToolOutput::Guidance(guidance) => {
                    push_provider_message_if_visible(session, guidance.question.clone());
                    messages.push(ChatMessage::tool(guidance.tool_call_id, guidance.question));
                }
                ResolvedAgentToolOutput::Action(action) => {
                    let result = apply_agent_action_with_policy(
                        session,
                        action.request,
                        action.summary,
                        policy_mode,
                    );
                    messages.push(ChatMessage::tool(action.tool_call_id, result));
                    if !matches!(
                        session.pending_action_selection(),
                        PendingActionSelection::None
                    ) {
                        return TurnResult {
                            route: Route::AskModel,
                            events: session.events()[start_index..].to_vec(),
                        };
                    }
                }
            }
        }
    }

    TurnResult {
        route: Route::AskModel,
        events: session.events()[start_index..].to_vec(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedAgentToolOutput {
    Guidance(ValidatedModelGuidanceRequest),
    Action(ValidatedModelToolAction),
}

fn resolve_agent_tool_outputs(
    outputs: Vec<ValidatedModelToolOutput>,
    path_resolution: &AgentPathResolution,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ValidatedModelToolOutput::Guidance(guidance) => {
                ResolvedAgentToolOutput::Guidance(guidance)
            }
            ValidatedModelToolOutput::Action(action) => {
                ResolvedAgentToolOutput::Action(resolve_agent_action_paths(action, path_resolution))
            }
        })
        .collect()
}

enum PlanExecutionSanitization {
    Outputs(Vec<ResolvedAgentToolOutput>),
    Retry(String),
}

fn sanitize_plan_execution_outputs(
    session: &Session,
    input: &str,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> PlanExecutionSanitization {
    if !mentions_plan_execution_request(&input.to_ascii_lowercase()) {
        return PlanExecutionSanitization::Outputs(outputs);
    }

    let Some(plan) = session.project_memory().latest_verified_plan() else {
        return PlanExecutionSanitization::Outputs(outputs);
    };

    let filtered = outputs
        .into_iter()
        .filter(|output| match output {
            ResolvedAgentToolOutput::Action(action) => !is_redundant_plan_execution_action(
                session,
                &action.request,
                &plan.path,
                &plan.project_root,
            ),
            ResolvedAgentToolOutput::Guidance(_) => true,
        })
        .collect::<Vec<_>>();

    let has_project_file_action = filtered.iter().any(|output| {
        matches!(
            output,
            ResolvedAgentToolOutput::Action(action)
                if is_plan_execution_project_file_action(&action.request)
        )
    });
    if has_project_file_action {
        return PlanExecutionSanitization::Outputs(filtered);
    }

    PlanExecutionSanitization::Retry(format!(
        "The user asked to execute the verified plan. Directory creation alone is incomplete. Use the latest verified plan at `{}` as source and create the actual project files under `{}`. Do not create, patch, or overwrite the plan file again. Original user request: {}",
        plan.path.display(),
        plan.project_root.display(),
        input
    ))
}

fn is_redundant_plan_execution_action(
    session: &Session,
    request: &ActionRequest,
    plan_path: &Path,
    plan_root: &Path,
) -> bool {
    match request {
        ActionRequest::CreateFile(create_file) => {
            resolved_target_path_for_existing_check(session, &create_file.target_path) == plan_path
        }
        ActionRequest::OverwriteFile(overwrite_file) => {
            resolved_target_path_for_existing_check(session, &overwrite_file.target_path)
                == plan_path
        }
        ActionRequest::PatchFile(patch_file) => {
            resolved_target_path_for_existing_check(session, &patch_file.target_path) == plan_path
        }
        ActionRequest::CreateDirectory(create_directory) => {
            resolved_target_path_for_existing_check(session, &create_directory.target_path)
                == plan_root
        }
        _ => false,
    }
}

fn is_plan_execution_project_file_action(request: &ActionRequest) -> bool {
    matches!(
        request,
        ActionRequest::CreateFile(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_)
    )
}

fn repair_directory_only_file_request(
    input: &str,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    if outputs.len() != 1 {
        return outputs;
    }

    let Some(file_request) = parse_clear_file_create_request(input) else {
        return outputs;
    };

    let ResolvedAgentToolOutput::Action(action) = &outputs[0] else {
        return outputs;
    };
    let ActionRequest::CreateDirectory(create_directory) = &action.request else {
        return outputs;
    };

    if Some(create_directory.target_path.as_path()) != file_request.target_path.parent() {
        return outputs;
    }

    let request = ActionRequest::CreateFile(file_request);
    let target_label = request.approval_target();

    vec![ResolvedAgentToolOutput::Action(ValidatedModelToolAction {
        tool_call_id: action.tool_call_id.clone(),
        request,
        summary: "create requested file".to_string(),
        target_label,
    })]
}

fn parse_clear_file_create_request(input: &str) -> Option<CreateFileAction> {
    let trimmed = input.trim();
    let rest = [
        "create a file called ",
        "create file called ",
        "write a file called ",
        "write file called ",
        "create a file named ",
        "create file named ",
        "write a file named ",
        "write file named ",
    ]
    .into_iter()
    .find_map(|prefix| strip_ascii_case_prefix(trimmed, prefix))?;

    let (file_name, rest) = split_ascii_case_once(rest, " inside ")?;
    let (directory, contents) = split_ascii_case_once(rest, " with ")?;
    let file_name = clean_requested_value(file_name)?;
    let directory = clean_requested_value(directory)?;
    let contents = clean_requested_contents(contents)?;
    let target_path = expand_user_path(Path::new(&directory)).join(file_name);

    Some(CreateFileAction {
        target_path,
        contents,
    })
}

fn clean_requested_value(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | '"' | '\''));
    (!value.is_empty()).then(|| value.to_string())
}

fn clean_requested_contents(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | '"' | '\''));
    Some(value.to_string())
}

fn expand_user_path(path: &Path) -> PathBuf {
    let path_string = path.as_os_str().to_string_lossy();
    if path_string == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = path_string.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

fn split_ascii_case_once<'a>(input: &'a str, delimiter: &str) -> Option<(&'a str, &'a str)> {
    let index = input
        .as_bytes()
        .windows(delimiter.len())
        .position(|window| window.eq_ignore_ascii_case(delimiter.as_bytes()))?;
    Some((&input[..index], &input[index + delimiter.len()..]))
}

fn strip_ascii_case_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = input.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &input[prefix.len()..])
}

fn review_required_action_to_propose(
    outputs: &[ResolvedAgentToolOutput],
    policy_mode: PermissionPolicyMode,
) -> Option<&ValidatedModelToolAction> {
    let reviewed_actions = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if action_requires_review(policy_mode, action) =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    reviewed_actions
        .iter()
        .copied()
        .find(|action| !matches!(action.request, ActionRequest::CreateDirectory(_)))
        .or_else(|| reviewed_actions.first().copied())
}

fn action_requires_review(
    policy_mode: PermissionPolicyMode,
    action: &ValidatedModelToolAction,
) -> bool {
    let proposed = Action::proposed(
        "policy-preview",
        action.request.clone(),
        action.summary.clone(),
    );
    policy_decision_for_agent_action(policy_mode, &proposed).user_approval_required
}

fn apply_agent_action_with_policy(
    session: &mut Session,
    request: ActionRequest,
    summary: String,
    policy_mode: PermissionPolicyMode,
) -> String {
    let request = match reconcile_create_file_target(session, request) {
        CreateFileReconciliation::Request(request) => request,
        CreateFileReconciliation::AlreadySatisfied(message) => {
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                message.clone(),
                AssistantMessageSource::Controller,
            )));
            return message;
        }
    };
    let proposed = Action::proposed(next_action_id(session), request, summary);
    let policy_decision = policy_decision_for_agent_action(policy_mode, &proposed);

    if policy_decision.user_approval_required {
        return propose_agent_action_for_review(session, proposed, policy_decision);
    }

    let action = proposed.approve();
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
        _ => Filesystem::apply_file_action(&action, allowed_root_for_action(session, &action))
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

enum CreateFileReconciliation {
    Request(ActionRequest),
    AlreadySatisfied(String),
}

fn reconcile_create_file_target(
    session: &Session,
    request: ActionRequest,
) -> CreateFileReconciliation {
    let ActionRequest::CreateFile(create_file) = request else {
        return CreateFileReconciliation::Request(request);
    };

    let target_path = resolved_target_path_for_existing_check(session, &create_file.target_path);
    if !target_path.is_file() {
        return CreateFileReconciliation::Request(ActionRequest::CreateFile(create_file));
    }

    match fs::read_to_string(&target_path) {
        Ok(existing_contents) if existing_contents == create_file.contents => {
            CreateFileReconciliation::AlreadySatisfied(format!(
                "{} already exists with the requested content.",
                target_path.display()
            ))
        }
        Ok(_) => {
            CreateFileReconciliation::Request(ActionRequest::OverwriteFile(OverwriteFileAction {
                target_path: create_file.target_path,
                contents: create_file.contents,
            }))
        }
        Err(_) => CreateFileReconciliation::Request(ActionRequest::CreateFile(create_file)),
    }
}

fn resolved_target_path_for_existing_check(session: &Session, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        session.cwd.join(target_path)
    }
}

fn propose_agent_action_for_review(
    session: &mut Session,
    action: Action,
    policy_decision: PolicyDecision,
) -> String {
    match session.pending_action_selection() {
        PendingActionSelection::None => {}
        PendingActionSelection::Single(_) => {
            return "A proposed action is already waiting. Ask the user to approve or reject it before proposing another action.".to_string();
        }
        PendingActionSelection::Ambiguous => {
            return "Multiple proposed actions are already waiting. Ask the user to approve or reject pending work before proposing another action.".to_string();
        }
    }

    let target = action.request.approval_target();
    let mut record = ActionRecord::new(action.clone());
    record.policy_decision = Some(policy_decision);
    session.push_event(Event::ActionProposed(
        ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
            .with_target(target.clone()),
    ));
    session.push_action(record);

    format!(
        "Proposed {:?} for review at {target}. Wait for the user to approve or reject before treating it as done.",
        action.kind()
    )
}

fn policy_decision_for_agent_action(mode: PermissionPolicyMode, action: &Action) -> PolicyDecision {
    match (mode, &action.request) {
        (PermissionPolicyMode::FullAccess, _) => PolicyDecision::allow_apply(
            mode,
            "full_access policy validated and allowed the model tool call",
        ),
        (
            PermissionPolicyMode::AutoCreateReviewModify,
            ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "auto_create_review_modify allows validated safe create actions",
        ),
        (
            PermissionPolicyMode::WorkspaceWriteWithReview,
            ActionRequest::CreateFile(_)
            | ActionRequest::CreateDirectory(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "workspace_write_with_review allows validated workspace write actions",
        ),
        (PermissionPolicyMode::AutoCreateReviewModify, _) => PolicyDecision::require_review(
            mode,
            "auto_create_review_modify gates edits, deletes, moves, and shell commands",
        ),
        (PermissionPolicyMode::WorkspaceWriteWithReview, _) => PolicyDecision::require_review(
            mode,
            "workspace_write_with_review gates deletes, moves, and shell commands",
        ),
        (PermissionPolicyMode::ReviewAll, _) => {
            PolicyDecision::require_review(mode, "review_all requires user approval")
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

    fn path_resolution(&self, workspace_root: &Path) -> AgentPathResolution {
        AgentPathResolution::new(
            self.requested_project_base.clone(),
            self.followup_base.clone(),
            workspace_root,
        )
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
        || normalized.contains("implement it")
        || normalized.contains("execute plan")
        || normalized.contains("execute the plan")
        || normalized.contains("execute it")
        || normalized.contains("apply the plan")
        || normalized.contains("apply it")
        || normalized.contains("create project according")
        || normalized.contains("create the project")
}

fn mentions_plan_execution_request(normalized: &str) -> bool {
    normalized.contains("implement the plan")
        || normalized.contains("implement plan")
        || normalized.contains("implement it")
        || normalized.contains("execute the plan")
        || normalized.contains("execute plan")
        || normalized.contains("execute it")
        || normalized.contains("apply the plan")
        || normalized.contains("apply it")
}

fn repeated_plan_create_response(session: &Session, input: &str) -> Option<String> {
    let normalized = input.to_ascii_lowercase();
    let asks_to_create_plan = normalized.contains("create the plan")
        || normalized.contains("create a plan")
        || normalized.contains("make the plan")
        || normalized.contains("write the plan");
    let asks_to_replace = normalized.contains("replace")
        || normalized.contains("overwrite")
        || normalized.contains("update")
        || normalized.contains("change");

    if !asks_to_create_plan || asks_to_replace {
        return None;
    }

    let plan = session.project_memory().latest_verified_plan()?;
    Some(format!(
        "The plan already exists at {}. I will use that plan unless you ask me to update or replace it.",
        display_project_path(session, &plan.path)
    ))
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
    fn permissive_agent_execute_it_retries_when_model_recreates_plan_only() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-execute-plan-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ReactProject")).unwrap();
        let plan_path = root.join("ReactProject/plan.md");
        std::fs::write(
            &plan_path,
            "# React Project Plan\n\n- Create package.json.\n- Create src/App.tsx.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Recreating the plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "bad-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "ReactProject" }),
                    assistant_summary: Some("create ReactProject".to_string()),
                },
                RawModelToolCall {
                    id: "bad-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ReactProject/plan.md",
                        "contents": "# Replacement Plan\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating the project files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "good-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "package.json",
                        "contents": "{\"scripts\":{\"dev\":\"vite\"}}\n"
                    }),
                    assistant_summary: Some("create package.json".to_string()),
                },
                RawModelToolCall {
                    id: "good-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/App.tsx",
                        "contents": "export default function App() { return <h1>Hello</h1>; }\n"
                    }),
                    assistant_summary: Some("create app".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("ReactProject/plan.md"),
                contents: String::new(),
            }),
            "create project plan",
        )
        .approve()
        .mark_applied();
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileCreated {
                path: "ReactProject/plan.md".to_string(),
            },
        ));
        session.push_action(plan_record);
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "ReactProject/plan.md".to_string(),
            }),
        );

        run_permissive_agent_turn(&provider, &mut session, "okay execute it");

        assert_eq!(
            std::fs::read_to_string(&plan_path).unwrap(),
            "# React Project Plan\n\n- Create package.json.\n- Create src/App.tsx.\n"
        );
        assert!(root.join("ReactProject/package.json").is_file());
        assert!(root.join("ReactProject/src/App.tsx").is_file());
        assert!(!root.join("package.json").exists());
        let provider_requests = provider.messages.lock().unwrap();
        assert!(provider_requests.len() >= 2);
        assert!(provider_requests[1].iter().any(|message| {
            matches!(message.role, ChatRole::Tool)
                && message
                    .content
                    .contains("Directory creation alone is incomplete")
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_execute_it_retries_directory_only_plan_execution() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-execute-dir-only-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ReactProject2")).unwrap();
        let plan_path = root.join("ReactProject2/plan.md");
        std::fs::write(
            &plan_path,
            "# React TS Tailwind Plan\n\n- Create package.json.\n- Create src/main.tsx.\n- Create src/App.tsx.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating directories.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "dir-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "public" }),
                    assistant_summary: Some("create public".to_string()),
                },
                RawModelToolCall {
                    id: "dir-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "src" }),
                    assistant_summary: Some("create src".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "file-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "package.json",
                        "contents": "{\"scripts\":{\"dev\":\"vite\"}}\n"
                    }),
                    assistant_summary: Some("create package.json".to_string()),
                },
                RawModelToolCall {
                    id: "file-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.tsx",
                        "contents": "import './styles.css';\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
                RawModelToolCall {
                    id: "file-call-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/App.tsx",
                        "contents": "export default function App() { return <h1>Hello</h1>; }\n"
                    }),
                    assistant_summary: Some("create app".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("ReactProject2/plan.md"),
                contents: String::new(),
            }),
            "create project plan",
        )
        .approve()
        .mark_applied();
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "ReactProject2/plan.md".to_string(),
            });
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(result.clone());
        session.push_action(plan_record);
        record_verified_project_memory(&mut session, &plan_action, &result);

        run_permissive_agent_turn(&provider, &mut session, "okay execute it");

        assert!(!root.join("public").exists());
        assert!(!root.join("src").exists());
        assert!(root.join("ReactProject2/package.json").is_file());
        assert!(root.join("ReactProject2/src/main.tsx").is_file());
        assert!(root.join("ReactProject2/src/App.tsx").is_file());
        assert_eq!(
            std::fs::read_to_string(&plan_path).unwrap(),
            "# React TS Tailwind Plan\n\n- Create package.json.\n- Create src/main.tsx.\n- Create src/App.tsx.\n"
        );
        let provider_requests = provider.messages.lock().unwrap();
        assert!(provider_requests.len() >= 2);
        assert!(provider_requests[1].iter().any(|message| {
            matches!(message.role, ChatRole::Tool)
                && message
                    .content
                    .contains("Directory creation alone is incomplete")
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_repeated_create_plan_reports_existing_plan() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-repeat-plan",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ReactProject")).unwrap();
        std::fs::write(root.join("ReactProject/plan.md"), "# React Project Plan\n").unwrap();
        let provider = SequenceProvider::new(Vec::new());
        let mut session = Session::new("session", &root, &root);

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("ReactProject/plan.md"),
                contents: String::new(),
            }),
            "create project plan",
        )
        .approve()
        .mark_applied();
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "ReactProject/plan.md".to_string(),
            });
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(result.clone());
        session.push_action(plan_record);
        record_verified_project_memory(&mut session, &plan_action, &result);

        run_permissive_agent_turn(&provider, &mut session, "create the plan please");

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("The plan already exists at ReactProject/plan.md")
        )));
        assert!(provider.messages.lock().unwrap().is_empty());
        assert_eq!(session.actions().len(), 1);

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
