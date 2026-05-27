use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

use crate::{
    action::{Action, ActionRequest, CreateFileAction, OverwriteFileAction},
    context::ContextBundle,
    controller::TurnResult,
    controller_project_memory::record_verified_project_memory,
    controller_reporting::verified_action_success_message,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage, VerifiedActionResult,
    },
    fs::Filesystem,
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ModelToolValidationErrorKind,
        RawModelToolCall, ValidatedModelGuidanceRequest, ValidatedModelToolAction,
        ValidatedModelToolOutput,
    },
    path_resolution::{allowed_root_for_action, resolve_agent_action_paths, AgentPathResolution},
    policy::{PermissionPolicyMode, PolicyDecision},
    provider::{
        ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction, ControllerProvider,
        ProviderErrorKind,
    },
    provider_visible_text_from_text_only_output,
    router::Route,
    session::{
        ActionRecord, PendingActionSelection, ProviderPromptMemorySelectedFact,
        ProviderPromptMemorySelection, Session, VerifiedPlanReference,
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
    "When creating a framework project, infer the necessary starter files from the requested stack and create the complete runnable scaffold before the final answer. ",
    "After tools run, answer naturally and briefly with what happened."
);
const AGENT_PLAIN_CHAT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar. Answer normal conversational messages directly in concise terminal-friendly prose. ",
    "This plain chat turn has no filesystem or shell tools attached. ",
    "If the user asks you to create, edit, move, delete, or run something, say you need a tool-enabled turn instead of claiming it was done. ",
    "Do not output tool-call markup. Do not claim filesystem or shell changes unless they were already reported by verified tool results."
);
const AGENT_PLAIN_FALLBACK_SYSTEM_PROMPT: &str = concat!(
    "The previous tool-enabled provider request returned no usable response. ",
    "Answer normally in prose. Do not claim filesystem or shell changes unless they were already reported by verified tool results."
);

const MAX_AGENT_TOOL_ROUNDS: usize = 6;
const TOOL_COMMAND_PREFIX: &str = "/tool";

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

    let Some(tool_input) = explicit_tool_command_input(input) else {
        run_plain_agent_chat(provider, session, input);
        return TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        };
    };

    if tool_input.is_empty() {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "Usage: /tool <request>",
            AssistantMessageSource::Controller,
        )));
        return TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        };
    }

    run_agent_tool_chat(provider, session, tool_input, policy_mode, start_index)
}

pub fn run_agent_tool_turn_with_policy<P>(
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
    run_agent_tool_chat(provider, session, input, policy_mode, start_index)
}

fn run_agent_tool_chat<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    policy_mode: PermissionPolicyMode,
    start_index: usize,
) -> TurnResult
where
    P: ControllerProvider,
{
    let agent_context = agent_verified_memory_context(session);
    let mut messages = vec![ChatMessage::new(ChatRole::System, AGENT_SYSTEM_PROMPT)];
    if let Some(context) = agent_local_runtime_context(session) {
        messages.push(ChatMessage::system(context));
    }
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
        session.push_event(Event::ProviderStarted(
            ProviderStarted::new(request.provider.clone(), request.request_id.clone())
                .with_request_details(request.model.clone(), "tool_enabled", tools.len()),
        ));

        let output = match provider.chat_messages_with_tools_with_metadata(
            messages.clone(),
            &request,
            tools.clone(),
        ) {
            Ok(output) => output,
            Err(error) => {
                if error.kind == ProviderErrorKind::EmptyResponse {
                    let fallback_request = provider.request_metadata();
                    session.push_event(Event::ProviderStarted(
                        ProviderStarted::new(
                            fallback_request.provider.clone(),
                            fallback_request.request_id.clone(),
                        )
                        .with_request_details(
                            fallback_request.model.clone(),
                            "plain_fallback",
                            0,
                        ),
                    ));
                    let mut fallback_messages = messages.clone();
                    fallback_messages.push(ChatMessage::system(AGENT_PLAIN_FALLBACK_SYSTEM_PROMPT));
                    match provider.chat_messages_with_metadata(fallback_messages, &fallback_request)
                    {
                        Ok(output) => {
                            let assistant_text = output.text.clone();
                            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                                fallback_request.provider,
                                fallback_request.request_id,
                                output,
                            )));
                            push_plain_provider_message_if_visible(session, assistant_text);
                            break;
                        }
                        Err(fallback_error) => {
                            session.push_event(Event::Error(ErrorEvent::new(format!(
                                "{} provider request {} failed: {error}; plain fallback request {} failed: {fallback_error}",
                                request.provider,
                                request.request_id,
                                fallback_request.request_id
                            ))));
                            break;
                        }
                    }
                }

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
            Err(error) => match tool_validation_recovery(&error) {
                ToolValidationRecovery::AskUser(message) => {
                    session.push_event(Event::AssistantMessage(AssistantMessage::new(
                        message,
                        AssistantMessageSource::Controller,
                    )));
                    break;
                }
                ToolValidationRecovery::Error(message) => {
                    session.push_event(Event::Error(ErrorEvent::new(message.clone())));
                    for tool_call in tool_calls {
                        messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                    }
                    continue;
                }
            },
        };

        let path_resolution = AgentPathResolution::new(None, None, &session.project_root);
        let resolved_outputs = resolve_agent_tool_outputs(outputs, &path_resolution);

        if let Err(message) = preflight_verified_plan_tool_outputs(session, &resolved_outputs) {
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                message,
                AssistantMessageSource::Controller,
            )));
            break;
        }

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

fn explicit_tool_command_input(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if trimmed == TOOL_COMMAND_PREFIX {
        return Some("");
    }

    let rest = trimmed.strip_prefix(TOOL_COMMAND_PREFIX)?;
    rest.strip_prefix(' ')
        .or_else(|| rest.strip_prefix('\t'))
        .map(str::trim)
}

fn run_plain_agent_chat<P>(provider: &P, session: &mut Session, input: &str)
where
    P: ControllerProvider,
{
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_chat", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_PLAIN_CHAT_SYSTEM_PROMPT),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                request.provider,
                request.request_id,
                output,
            )));
            push_plain_provider_message_if_visible(session, assistant_text);
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }
}

fn agent_local_runtime_context(session: &mut Session) -> Option<String> {
    let project_root = session.project_root.clone();
    let cwd = session.cwd.clone();
    let max_window_tokens = session.context_accounting().max_window_tokens;
    let bundle = ContextBundle::from_default_local_files(project_root, cwd, max_window_tokens);
    session.set_context_accounting(bundle.accounting.clone());
    bundle.system_context()
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

fn preflight_verified_plan_tool_outputs(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Result<(), String> {
    let Some(plan) = session.project_memory().latest_verified_plan() else {
        return Ok(());
    };

    for target_path in outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => Some(action),
            ResolvedAgentToolOutput::Guidance(_) => None,
        })
        .flat_map(|action| plan_preflight_paths(&action.request))
    {
        let target_path = absolute_session_path(session, target_path);
        if !path_is_within(&target_path, &plan.project_root) {
            return Err(plan_preflight_outside_root_message(
                session,
                plan,
                &target_path,
            ));
        }
    }

    Ok(())
}

fn plan_preflight_paths(request: &ActionRequest) -> Vec<&Path> {
    match request {
        ActionRequest::CreateFile(action) => vec![&action.target_path],
        ActionRequest::CreateDirectory(action) => vec![&action.target_path],
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(action) => vec![&action.target_path],
        ActionRequest::MoveFile(action) => vec![&action.source_path, &action.target_path],
        ActionRequest::ShellCommand(_) => Vec::new(),
    }
}

fn absolute_session_path(session: &Session, path: &Path) -> PathBuf {
    normalize_path(if path.is_absolute() {
        path.to_path_buf()
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
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn plan_preflight_outside_root_message(
    session: &Session,
    plan: &VerifiedPlanReference,
    target_path: &Path,
) -> String {
    format!(
        "The verified plan is rooted at {}, but the tool call targets {} outside that project. No filesystem action was applied.",
        display_agent_context_path(session, &plan.project_root),
        display_agent_context_path(session, target_path)
    )
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
    let create_file = match request {
        ActionRequest::CreateFile(create_file) => create_file,
        ActionRequest::OverwriteFile(overwrite_file) => {
            let target_path =
                resolved_target_path_for_existing_check(session, &overwrite_file.target_path);
            if target_path.is_file() {
                return CreateFileReconciliation::Request(ActionRequest::OverwriteFile(
                    overwrite_file,
                ));
            }
            CreateFileAction {
                target_path: overwrite_file.target_path,
                contents: overwrite_file.contents,
            }
        }
        _ => return CreateFileReconciliation::Request(request),
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

fn push_plain_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message = message.into();
    if looks_like_raw_tool_protocol(&message) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "That was a plain chat turn, so no filesystem action was executed. Use `/tool <request>` to allow filesystem tools.",
            AssistantMessageSource::Controller,
        )));
        return;
    }

    push_provider_message_if_visible(session, message);
}

fn looks_like_raw_tool_protocol(message: &str) -> bool {
    [
        "<|channel|>",
        "<|message|>",
        "to=filesystem.",
        "filesystem.create",
        "filesystem.write",
        "filesystem.patch",
        "filesystem.move",
        "filesystem.delete",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

#[derive(Debug, Clone, Default)]
struct AgentVerifiedMemoryContext {
    prompt_context: Option<String>,
}

enum ToolValidationRecovery {
    AskUser(String),
    Error(String),
}

fn tool_validation_recovery(
    error: &crate::model_runtime::ModelToolValidationError,
) -> ToolValidationRecovery {
    if let Some(message) = tool_validation_guidance_message(error) {
        return ToolValidationRecovery::AskUser(message);
    }

    ToolValidationRecovery::Error(format!(
        "{} No filesystem action was applied.",
        friendly_tool_validation_error(error)
    ))
}

fn tool_validation_guidance_message(
    error: &crate::model_runtime::ModelToolValidationError,
) -> Option<String> {
    if !is_missing_or_malformed_tool_argument(error) {
        return None;
    }

    let tool = error.tool_name.as_deref().unwrap_or("tool");
    match error.argument.as_deref()? {
        "target_path" => Some(format!(
            "I need a concrete target path before I can {}. Which file or folder should I use?",
            tool_action_phrase(tool)
        )),
        "source_path" => Some(format!(
            "I need the source path before I can {}. Which existing file should I use?",
            tool_action_phrase(tool)
        )),
        "cwd" => Some(format!(
            "I need a working directory before I can {}. Which folder should I run it in?",
            tool_action_phrase(tool)
        )),
        "command" => {
            Some("I need the shell command before I can run it. What command should I run?".into())
        }
        "contents" if is_file_contents_tool(error) => Some(format!(
            "I need file contents before I can {}. What should I write?",
            tool_action_phrase(tool)
        )),
        "find" => Some(
            "I need the exact text to replace before I can edit the file. What text should I replace?"
                .into(),
        ),
        "replace" => Some(
            "I need the replacement text before I can edit the file. What should I replace it with?"
                .into(),
        ),
        _ => None,
    }
}

fn is_missing_or_malformed_tool_argument(
    error: &crate::model_runtime::ModelToolValidationError,
) -> bool {
    matches!(
        error.kind,
        ModelToolValidationErrorKind::MissingArgument
            | ModelToolValidationErrorKind::MalformedArgument
    )
}

fn is_file_contents_tool(error: &crate::model_runtime::ModelToolValidationError) -> bool {
    matches!(
        error.tool_name.as_deref(),
        Some("create_file" | "overwrite_file")
    )
}

fn friendly_tool_validation_error(
    error: &crate::model_runtime::ModelToolValidationError,
) -> String {
    match (error.tool_name.as_deref(), error.argument.as_deref()) {
        (Some(tool), Some(argument)) => {
            format!("The `{tool}` tool call is incomplete or malformed for `{argument}`.")
        }
        (Some(tool), None) => format!("The `{tool}` tool call is incomplete or malformed."),
        (None, _) => "The model returned an incomplete or malformed tool call.".to_string(),
    }
}

fn tool_action_phrase(tool_name: &str) -> &'static str {
    match tool_name {
        "create_file" => "create the file",
        "create_directory" => "create the folder",
        "overwrite_file" => "overwrite the file",
        "patch_file" => "edit the file",
        "delete_file" => "delete the file",
        "move_file" => "move the file",
        "shell_command" => "run the command",
        _ => "use the tool",
    }
}

fn agent_verified_memory_context(session: &mut Session) -> AgentVerifiedMemoryContext {
    let mut selected = Vec::new();
    let mut lines = Vec::new();
    if let Some(folder) = session.project_memory().latest_verified_folder() {
        lines.push(format!(
            "- latest verified folder: {}",
            display_agent_context_path(session, &folder.path)
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
            display_agent_context_path(session, &plan.path)
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
            "Use these verified paths only when the explicit tool request refers to prior work."
                .to_string(),
            "Displayed paths are relative to the current working directory when possible."
                .to_string(),
        ];
        context.extend(lines);
        Some(context.join("\n"))
    };

    AgentVerifiedMemoryContext { prompt_context }
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
        "Recent conversation context for the explicit tool request:\n{}",
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
    truncate_utf8(&line, LIMIT)
}

fn verified_plan_excerpt(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let contents = contents.trim();
    if contents.is_empty() {
        return None;
    }

    const LIMIT: usize = 1200;
    Some(truncate_utf8(contents, LIMIT))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let suffix = "...";
    let max_content = max_bytes.saturating_sub(suffix.len());
    let mut end = max_content.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], suffix)
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

fn next_action_id(session: &Session) -> String {
    format!("action-{}", session.actions().len() + 1)
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

        let result = run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a folder demo",
            PermissionPolicyMode::FullAccess,
        );

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

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create demo",
            PermissionPolicyMode::FullAccess,
        );

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

    #[test]
    fn permissive_agent_plan_only_request_does_not_retry_as_implementation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-only-no-implementation",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating folder and plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-only-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "ReactPlanOnly" }),
                    assistant_summary: Some("create folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-only-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ReactPlanOnly/plan.md",
                        "contents": "# React TS Tailwind Plan\n\n- Create package.json.\n- Create src/main.tsx.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created. I have not implemented it yet."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a folder called ReactPlanOnly, then create a plan for a simple React TypeScript Tailwind project inside it. The plan should include all necessary files, but do not implement yet.",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("ReactPlanOnly/plan.md").is_file());
        assert!(!root.join("ReactPlanOnly/package.json").exists());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::Error(error)
                if error
                    .message
                    .contains("Provider did not return the required filesystem tool calls")
        )));
        assert_eq!(provider.messages.lock().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_create_file_target_asks_user_without_raw_tool_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-create-target",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "Creating the file.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "missing-create-target-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({ "contents": "# Plan\n" }),
            assistant_summary: None,
        }])]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create an md file with a plan for projects",
            PermissionPolicyMode::FullAccess,
        );

        assert!(session.actions().is_empty());
        assert_eq!(provider.messages.lock().unwrap().len(), 1);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message
                        .content
                        .contains("I need a concrete target path before I can create the file")
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::Error(error)
                if error.message.contains("model tool")
                    || error.message.contains("missing required argument")
                    || error.message.contains("Tool error")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_preflight_blocks_file_actions_outside_plan_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-preflight-outside",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("demo")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "Creating missing files.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "outside-plan-root-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "other/index.tsx",
                "contents": "export default function Home() {}\n"
            }),
            assistant_summary: None,
        }])]);
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: root.join("demo/project-plan.md"),
            project_root: root.join("demo"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue from the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!root.join("other/index.tsx").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("verified plan is rooted at demo")
                    && message.content.contains("outside that project")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_preflight_allows_file_actions_inside_plan_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-preflight-inside",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "inside-plan-root-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "demo/index.tsx",
                        "contents": "export default function Home() {}\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: cwd.join("demo/project-plan.md"),
            project_root: cwd.join("demo"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue from the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/index.tsx")).unwrap(),
            "export default function Home() {}\n"
        );
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_context_uses_cwd_relative_paths_for_tool_turns() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-context-cwd-relative",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("tui-state-test")).unwrap();
        std::fs::write(
            cwd.join("tui-state-test/PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "I found the verified plan.",
        )]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: cwd.join("tui-state-test"),
            source_action_id: "action-folder".to_string(),
        });
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: cwd.join("tui-state-test/PLAN.md"),
            project_root: cwd.join("tui-state-test"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the plan",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("- latest verified folder: tui-state-test"));
        assert!(verified_context
            .content
            .contains("- latest verified plan: tui-state-test/PLAN.md"));
        assert!(!verified_context
            .content
            .contains("playground/tui-state-test"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_preflight_blocks_duplicated_cwd_prefix_target() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-preflight-duplicate-prefix",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "Creating missing files.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "duplicate-prefix-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "playground/demo/index.tsx",
                "contents": "export default function Home() {}\n"
            }),
            assistant_summary: None,
        }])]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: cwd.join("demo/project-plan.md"),
            project_root: cwd.join("demo"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue from the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!cwd.join("playground/demo/index.tsx").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("verified plan is rooted at demo")
                    && message.content.contains("targets playground/demo/index.tsx")
                    && message.content.contains("outside that project")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_move_source_path_asks_user_without_raw_tool_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-move-source",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            SequenceProvider::new(vec![crate::event::ProviderOutput::new("Moving the file.")
                .with_tool_calls(vec![RawModelToolCall {
                    id: "missing-move-source-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::MoveFile),
                    arguments: json!({ "target_path": "renamed.md" }),
                    assistant_summary: None,
                }])]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "move the file",
            PermissionPolicyMode::FullAccess,
        );

        assert!(session.actions().is_empty());
        assert_eq!(provider.messages.lock().unwrap().len(), 1);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("source path")
                    && message.content.contains("move the file")
        )));
        assert_no_raw_tool_validation_error(&session);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_shell_cwd_asks_user_without_running_command() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-shell-cwd",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "Running the command.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "missing-shell-cwd-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({ "command": "printf hello" }),
            assistant_summary: None,
        }])]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "run a shell command",
            PermissionPolicyMode::FullAccess,
        );

        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message
                        .content
                        .contains("working directory before I can run the command")
        )));
        assert_no_raw_tool_validation_error(&session);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_patch_find_asks_for_exact_text_without_raw_tool_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-patch-find",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("notes.md"), "old\n").unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "Patching the file.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "missing-patch-find-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::PatchFile),
            arguments: json!({
                "target_path": "notes.md",
                "find": "",
                "replace": "new"
            }),
            assistant_summary: None,
        }])]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "patch notes",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "old\n"
        );
        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("exact text to replace")
        )));
        assert_no_raw_tool_validation_error(&session);

        let _ = std::fs::remove_dir_all(&root);
    }

    fn assert_no_raw_tool_validation_error(session: &Session) {
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::Error(error)
                if error.message.contains("model tool")
                    || error.message.contains("missing required argument")
                    || error.message.contains("Tool error")
        )));
    }

    #[test]
    fn compact_context_line_truncates_unicode_at_char_boundary() {
        let input = format!("{} {}", "plan", "│".repeat(200));
        let line = compact_context_line(&input);

        assert!(line.ends_with("..."));
        assert!(line.is_char_boundary(line.len()));
    }

    #[test]
    fn verified_plan_excerpt_truncates_unicode_at_char_boundary() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-unicode-plan",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let plan = root.join("plan.md");
        std::fs::write(&plan, format!("# Plan\n\n{}\n", "├─ src/│".repeat(300))).unwrap();

        let excerpt = verified_plan_excerpt(&plan).unwrap();

        assert!(excerpt.ends_with("..."));
        assert!(excerpt.is_char_boundary(excerpt.len()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum CapturedProviderRequestMode {
        Plain,
        Tool,
    }

    #[derive(Debug, Clone)]
    struct CapturedProviderRequest {
        mode: CapturedProviderRequestMode,
        messages: Vec<ChatMessage>,
        tool_count: usize,
    }

    #[derive(Debug, Clone)]
    struct CapturingProvider {
        requests: std::sync::Arc<std::sync::Mutex<Vec<CapturedProviderRequest>>>,
        plain_output: crate::event::ProviderOutput,
        tool_output: crate::event::ProviderOutput,
    }

    impl CapturingProvider {
        fn new() -> Self {
            Self {
                requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                plain_output: crate::event::ProviderOutput::new("Plain answer."),
                tool_output: crate::event::ProviderOutput::new("I'll create it."),
            }
        }

        fn with_tool_output(mut self, output: crate::event::ProviderOutput) -> Self {
            self.tool_output = output;
            self
        }

        fn with_plain_output(mut self, output: crate::event::ProviderOutput) -> Self {
            self.plain_output = output;
            self
        }

        fn requests(&self) -> Vec<CapturedProviderRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn joined_request_messages(request: &CapturedProviderRequest) -> String {
        request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    impl ControllerProvider for CapturingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("capture", Some("test-model".to_string()), "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Ok(self.plain_output.clone())
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.requests.lock().unwrap().push(CapturedProviderRequest {
                mode: CapturedProviderRequestMode::Tool,
                messages,
                tool_count: tools.len(),
            });
            Ok(self.tool_output.clone())
        }

        fn chat_messages_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.requests.lock().unwrap().push(CapturedProviderRequest {
                mode: CapturedProviderRequestMode::Plain,
                messages,
                tool_count: 0,
            });
            Ok(self.plain_output.clone())
        }
    }

    #[test]
    fn permissive_agent_plain_text_uses_plain_provider_request_first() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plain-text-runtime",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        for input in ["hello", "say hi", "what are you?", "write a short sentence"] {
            let provider = CapturingProvider::new();
            let mut session = Session::new("session", &root, &root);

            run_permissive_agent_turn(&provider, &mut session, input);

            let requests = provider.requests();
            assert_eq!(requests.len(), 1, "unexpected request count for {input}");
            assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
            assert_eq!(requests[0].tool_count, 0);
            assert_eq!(requests[0].messages.last(), Some(&ChatMessage::user(input)));
            let joined = joined_request_messages(&requests[0]);
            assert!(!joined.contains("latest verified folder"));
            assert!(!joined.contains("latest verified plan"));
            assert!(!joined.contains("Verified filesystem context"));
            assert!(session.events().iter().any(|event| matches!(
                event,
                Event::ProviderStarted(started)
                    if started.request_mode.as_deref() == Some("plain_chat")
                        && started.model.as_deref() == Some("test-model")
                        && started.tool_count == Some(0)
            )));
            assert!(!session.events().iter().any(|event| matches!(
                event,
                Event::ProviderStarted(started)
                    if started.request_mode.as_deref() == Some("tool_enabled")
            )));
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_hello_after_verified_folder_does_not_inject_folder_memory() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-hello-no-folder-memory",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("remembered")).unwrap();
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);
        let folder_action = Action::proposed(
            "action-folder",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("remembered"),
            }),
            "create remembered folder",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &folder_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "remembered".to_string(),
            }),
        );

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        let joined = joined_request_messages(&requests[0]);
        assert!(!joined.contains("latest verified folder"));
        assert!(!joined.contains("remembered"));
        assert!(session.latest_provider_prompt_memory_selection().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_state_question_is_not_a_controller_phrase_trigger() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-question-model",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("remembered")).unwrap();
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);
        let folder_action = Action::proposed(
            "action-folder",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("remembered"),
            }),
            "create remembered folder",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &folder_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "remembered".to_string(),
            }),
        );

        run_permissive_agent_turn(&provider, &mut session, "what did you create?");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(
            requests[0].messages.last(),
            Some(&ChatMessage::user("what did you create?"))
        );
        let joined = joined_request_messages(&requests[0]);
        assert!(!joined.contains("latest verified folder"));
        assert!(!joined.contains("remembered"));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("created")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_plain_chat_does_not_surface_raw_tool_protocol_as_success() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plain-raw-tool-protocol",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new(
                "<|channel|>commentary to=filesystem.create code<|message|>{\"path\":\"testharness\",\"contents\":\"\"}\nCreated folder testharness.",
            ),
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a folder and name it testharness",
        );

        assert!(!root.join("testharness").exists());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("plain chat turn")
                    && message.content.contains("/tool <request>")
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("<|channel|>")
                    || message.content.contains("Created folder testharness")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_ok_after_verified_plan_stays_plain_without_file_creation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-ok-no-plan-execution",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let plan_path = root.join("app/project-plan.md");
        std::fs::write(
            &plan_path,
            "# Project Plan\n\n- Create package.json.\n- Create src/main.ts.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new().with_tool_output(
            crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "bad-ok-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "package.json",
                        "contents": "{}\n"
                    }),
                    assistant_summary: Some("create package".to_string()),
                },
            ]),
        );
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
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "app/project-plan.md".to_string(),
            });
        let mut plan_record = ActionRecord::new(plan_action.clone());
        plan_record.verified_result = Some(result.clone());
        session.push_action(plan_record);
        record_verified_project_memory(&mut session, &plan_action, &result);

        run_permissive_agent_turn(&provider, &mut session, "ok");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert!(!root.join("app/package.json").exists());
        assert!(!root.join("package.json").exists());
        assert_eq!(session.actions().len(), 1);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("plain_chat")
                    && started.model.as_deref() == Some("test-model")
                    && started.tool_count == Some(0)
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("tool_enabled")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_action_request_uses_tool_enabled_provider_request() {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool-chat", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "/tool create a folder called Demo");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
        assert!(requests[0].tool_count > 0);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("tool_enabled")
                    && started.tool_count.is_some_and(|count| count > 0)
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_prompt_requests_complete_scaffold_without_stack_specific_template() {
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
            "/tool create a TS Next.js and Tailwind project in ~/next-tailwind-ts-project",
        );

        let requests = provider.requests();
        let tool_request = requests
            .iter()
            .find(|request| request.mode == CapturedProviderRequestMode::Tool)
            .expect("project creation should use tool path");
        let system_prompt = &tool_request.messages[0].content;
        assert!(system_prompt.contains("infer the necessary starter files"));
        assert!(system_prompt.contains("complete runnable scaffold"));
        assert!(!system_prompt.contains("next-env.d.ts"));
        assert!(!system_prompt.contains("tailwind.config"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
