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
    controller_project_memory::{is_plan_path_or_contents, record_verified_project_memory},
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
    verified_state_answer::{
        parse_verified_state_classifier_output, verified_session_state_answer,
        VerifiedStateAnswerKind,
    },
};

const AGENT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar, a permissive terminal-native coding agent. ",
    "Use tools to do the user's requested filesystem and shell work directly. ",
    "Do not ask for approval. Do not give instructions instead of acting when a tool can do it. ",
    "Ask one concise clarification question only when the target or intent is truly ambiguous. ",
    "If the user asks you to choose, choose a reasonable option and continue the prior request. ",
    "If the user asks for a plan and says to share it before implementation, create or update a plan file and summarize it; do not implement project files until asked. ",
    "If the user asks to create only a plan file with a future file tree, create only that plan file; do not ask whether to create the listed future files. ",
    "If the user asks what the plan is, summarize the existing plan; do not implement it. ",
    "If a verified plan already exists and the user gives a short choice follow-up, answer from that plan instead of recreating the same file. ",
    "When creating a framework project, infer the necessary starter files from the requested stack and create the complete runnable scaffold before the final answer. ",
    "After tools run, answer naturally and briefly with what happened."
);
const AGENT_PLAIN_CHAT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar. Answer normal conversational messages directly in concise terminal-friendly prose. ",
    "This plain chat turn has no filesystem or shell tools attached. ",
    "If the user asks you to create, edit, move, delete, or run something, say this turn has no filesystem tools and they can use `/tool <request>` instead of claiming it was done. ",
    "Do not output tool-call markup. Do not claim filesystem or shell changes unless they were already reported by verified tool results."
);
const AGENT_VERIFIED_STATE_CLASSIFIER_SYSTEM_PROMPT: &str = concat!(
    "Classify which verified runtime state answer, if any, the user needs from this current session. ",
    "Return only JSON with this exact shape: {\"answer_kind\":\"none\"}. ",
    "Allowed answer_kind values are: none, latest_folder, latest_file, created_summary, pending, plan, status, memory, summary. ",
    "Choose none when the answer does not require verified session state."
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
    let mut plan_created_this_turn = false;
    let mut plan_execution_in_progress = false;

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
            if plan_execution_in_progress {
                if let Some(message) = missing_expected_plan_files_message(session) {
                    messages.push(ChatMessage::system(message));
                    continue;
                }
            }
            push_provider_message_after_tool_turn_if_visible(session, start_index, assistant_text);
            break;
        }

        tool_calls.retain(|tool_call| handled_tool_call_ids.insert(tool_call.id.clone()));
        if tool_calls.is_empty() {
            push_provider_message_after_tool_turn_if_visible(session, start_index, assistant_text);
            break;
        }

        messages.push(chat_assistant_tool_call_message(
            assistant_text,
            &tool_calls,
        ));

        let outputs = match validate_model_tool_outputs(&tool_calls) {
            Ok(outputs) => outputs,
            Err(error) => match tool_validation_recovery(&error) {
                ToolValidationRecovery::RepairModel(message) => {
                    for tool_call in tool_calls {
                        handled_tool_call_ids.remove(&tool_call.id);
                        messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                    }
                    continue;
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
        let resolved_outputs = anchor_verified_plan_tool_outputs(
            session,
            resolve_agent_tool_outputs(outputs, &path_resolution),
        );
        let resolved_outputs =
            guard_plan_creation_tool_outputs(session, resolved_outputs, plan_created_this_turn);
        let resolved_outputs = guard_redundant_directory_tool_outputs(session, resolved_outputs);
        let plan_execution_batch =
            resolved_outputs_touch_structured_plan(session, &resolved_outputs);
        plan_execution_in_progress |= plan_execution_batch;
        let resolved_outputs = guard_plan_execution_tool_outputs(
            session,
            resolved_outputs,
            plan_execution_in_progress,
        );

        if let Err(message) = preflight_verified_plan_tool_outputs(session, &resolved_outputs) {
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                message,
                AssistantMessageSource::Controller,
            )));
            break;
        }

        if policy_mode == PermissionPolicyMode::ReviewAll {
            if let Some(action) =
                review_required_action_to_propose(session, &resolved_outputs, policy_mode)
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

        let mut skipped_tool_notice_shown = false;
        for output in resolved_outputs {
            match output {
                ResolvedAgentToolOutput::Guidance(guidance) => {
                    push_provider_message_if_visible(session, guidance.question.clone());
                    messages.push(ChatMessage::tool(guidance.tool_call_id, guidance.question));
                }
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id,
                    message,
                    visible,
                } => {
                    if visible && !skipped_tool_notice_shown {
                        session.push_event(Event::AssistantMessage(AssistantMessage::new(
                            message.clone(),
                            AssistantMessageSource::Controller,
                        )));
                        skipped_tool_notice_shown = true;
                    }
                    messages.push(ChatMessage::tool(tool_call_id, message));
                }
                ResolvedAgentToolOutput::Action(action) => {
                    let is_plan_creation =
                        plan_creation_root_for_action(session, &action.request).is_some();
                    let result = apply_agent_action_with_policy(
                        session,
                        action.request,
                        action.summary,
                        policy_mode,
                    );
                    if is_plan_creation
                        && session
                            .actions()
                            .last()
                            .and_then(|record| record.verified_result.as_ref())
                            .is_some()
                    {
                        plan_created_this_turn = true;
                    }
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

        if plan_execution_in_progress {
            if let Some(message) = missing_expected_plan_files_message(session) {
                messages.push(ChatMessage::system(message));
                continue;
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
    if has_verified_session_state(session) {
        if let Some(answer_kind) = classify_verified_state_answer_kind(provider, session, input) {
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                verified_session_state_answer(session, answer_kind),
                AssistantMessageSource::Controller,
            )));
            return;
        }
    }

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

fn classify_verified_state_answer_kind<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> Option<VerifiedStateAnswerKind>
where
    P: ControllerProvider,
{
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_state_classifier", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_VERIFIED_STATE_CLASSIFIER_SYSTEM_PROMPT),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_with_metadata(messages, &request) {
        Ok(output) => {
            let decision = parse_verified_state_classifier_output(&output.text);
            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                request.provider,
                request.request_id,
                output,
            )));
            decision
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider state classification request {} failed: {error}",
                request.provider, request.request_id
            ))));
            None
        }
    }
}

fn has_verified_session_state(session: &Session) -> bool {
    session
        .actions()
        .iter()
        .any(|record| record.verified_result.is_some())
        || !matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        )
        || session.project_memory().latest_verified_folder().is_some()
        || session.project_memory().latest_verified_plan().is_some()
        || session.project_memory().latest_structured_plan().is_some()
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
    Skipped {
        tool_call_id: String,
        message: String,
        visible: bool,
    },
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

fn anchor_verified_plan_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => {
                ResolvedAgentToolOutput::Guidance(guidance)
            }
            ResolvedAgentToolOutput::Skipped {
                tool_call_id,
                message,
                visible,
            } => ResolvedAgentToolOutput::Skipped {
                tool_call_id,
                message,
                visible,
            },
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_verified_plan_action_request(session, action.request);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
        })
        .collect()
}

fn guard_plan_creation_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    plan_created_this_turn: bool,
) -> Vec<ResolvedAgentToolOutput> {
    let plan_roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                plan_creation_root_for_action(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if plan_roots.is_empty() && !plan_created_this_turn {
        return outputs;
    }

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action) if plan_created_this_turn => {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped implementation tool calls after creating the verified plan. Run `/tool execute the plan` when you want to apply it.".to_string(),
                    visible: true,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if plan_creation_root_for_action(session, &action.request).is_some()
                    || is_plan_parent_setup_action(session, &action.request, &plan_roots) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: action.tool_call_id,
                message: "Skipped extra implementation tool calls in this plan-creation turn. Run `/tool execute the plan` when you want to apply the verified plan.".to_string(),
                visible: true,
            },
            other => other,
        })
        .collect()
}

fn guard_redundant_directory_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    let file_targets = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                created_file_target_path(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if file_targets.is_empty() {
        return outputs;
    }

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if is_redundant_directory_action(session, &action.request, &file_targets) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped redundant directory creation because a file tool call in the same batch already creates that parent directory.".to_string(),
                    visible: false,
                }
            }
            other => other,
        })
        .collect()
}

fn resolved_outputs_touch_structured_plan(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return false;
    };

    outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => Some(&action.request),
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .flat_map(plan_guard_paths)
        .any(|path| structured_plan_expects_path(plan, &absolute_session_path(session, path)))
}

fn guard_plan_execution_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    plan_execution_in_progress: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if !plan_execution_in_progress {
        return outputs;
    }

    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: guidance.tool_call_id,
                message: plan_execution_continue_message(session),
                visible: false,
            },
            ResolvedAgentToolOutput::Action(action)
                if is_unexpected_plan_execution_directory(session, plan, &action.request) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped directory creation outside the verified plan; file tools create parent directories when needed.".to_string(),
                    visible: false,
                }
            }
            other => other,
        })
        .collect()
}

fn is_unexpected_plan_execution_directory(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    !structured_plan_expects_path(plan, &target_path)
}

fn created_file_target_path(session: &Session, request: &ActionRequest) -> Option<PathBuf> {
    match request {
        ActionRequest::CreateFile(action) => {
            Some(absolute_session_path(session, &action.target_path))
        }
        _ => None,
    }
}

fn is_redundant_directory_action(
    session: &Session,
    request: &ActionRequest,
    file_targets: &[PathBuf],
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let directory = absolute_session_path(session, &action.target_path);
    file_targets
        .iter()
        .any(|file| path_is_within(file, &directory))
}

fn plan_creation_root_for_action(session: &Session, request: &ActionRequest) -> Option<PathBuf> {
    let path = match request {
        ActionRequest::CreateFile(action)
            if is_plan_path_or_contents(&action.target_path, &action.contents) =>
        {
            &action.target_path
        }
        ActionRequest::OverwriteFile(action)
            if is_plan_path_or_contents(&action.target_path, &action.contents) =>
        {
            &action.target_path
        }
        _ => return None,
    };

    absolute_session_path(session, path)
        .parent()
        .map(Path::to_path_buf)
}

fn is_plan_parent_setup_action(
    session: &Session,
    request: &ActionRequest,
    plan_roots: &[PathBuf],
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    plan_roots
        .iter()
        .any(|plan_root| path_is_within(plan_root, &target_path))
}

fn anchor_verified_plan_action_request(session: &Session, request: ActionRequest) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::CreateDirectory(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::CreateDirectory(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::DeleteFile(mut action) => {
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::DeleteFile(action)
        }
        ActionRequest::MoveFile(mut action) => {
            action.source_path = anchor_verified_plan_path(session, &action.source_path);
            action.target_path = anchor_verified_plan_path(session, &action.target_path);
            ActionRequest::MoveFile(action)
        }
        ActionRequest::ShellCommand(action) => ActionRequest::ShellCommand(action),
    }
}

fn anchor_verified_plan_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let current_target = absolute_session_path(session, path);
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return path.to_path_buf();
    };
    if path_is_within(&current_target, &plan.project_root) {
        return path.to_path_buf();
    }

    let anchored_target = normalize_path(plan.project_root.join(path));
    if !structured_plan_expects_path(plan, &anchored_target) {
        return path.to_path_buf();
    }

    cwd_relative_path(session, &anchored_target)
}

fn structured_plan_expects_path(plan: &crate::session::StructuredProjectPlan, path: &Path) -> bool {
    let path = normalize_path(path);
    plan.expected_files
        .iter()
        .chain(plan.expected_directories.iter())
        .any(|expected| normalize_path(expected) == path)
}

fn cwd_relative_path(session: &Session, path: &Path) -> PathBuf {
    path.strip_prefix(&session.cwd)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
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
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
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
        ActionRequest::CreateDirectory(_) => Vec::new(),
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(action) => vec![&action.target_path],
        ActionRequest::MoveFile(action) => vec![&action.source_path, &action.target_path],
        ActionRequest::ShellCommand(_) => Vec::new(),
    }
}

fn plan_guard_paths(request: &ActionRequest) -> Vec<&Path> {
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

fn missing_expected_plan_files_message(session: &Session) -> Option<String> {
    let missing = missing_expected_plan_files(session);
    if missing.is_empty() {
        return None;
    }

    let paths = missing
        .iter()
        .map(|path| format!("- {}", display_agent_context_path(session, path)))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "The verified plan is not complete. Missing expected files:\n{paths}\nUse create_file for every missing file under the verified plan root. Do not ask whether to create expected files or directories."
    ))
}

fn missing_expected_plan_files(session: &Session) -> Vec<PathBuf> {
    session
        .project_memory()
        .latest_structured_plan()
        .map(|plan| {
            plan.expected_files
                .iter()
                .filter(|path| !path.is_file())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn plan_execution_continue_message(session: &Session) -> String {
    missing_expected_plan_files_message(session).unwrap_or_else(|| {
        "The verified plan already defines concrete expected paths; continue under the verified plan root without asking for clarification.".to_string()
    })
}

fn review_required_action_to_propose<'a>(
    session: &Session,
    outputs: &'a [ResolvedAgentToolOutput],
    policy_mode: PermissionPolicyMode,
) -> Option<&'a ValidatedModelToolAction> {
    let reviewed_actions = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if action_requires_review(session, policy_mode, action) =>
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
    session: &Session,
    policy_mode: PermissionPolicyMode,
    action: &ValidatedModelToolAction,
) -> bool {
    let proposed = Action::proposed(
        "policy-preview",
        action.request.clone(),
        action.summary.clone(),
    );
    policy_decision_for_agent_action(session, policy_mode, &proposed).user_approval_required
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
    let policy_decision = policy_decision_for_agent_action(session, policy_mode, &proposed);

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

fn policy_decision_for_agent_action(
    session: &Session,
    mode: PermissionPolicyMode,
    action: &Action,
) -> PolicyDecision {
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
        ) if action_targets_are_inside_workspace(session, action) => PolicyDecision::allow_apply(
            mode,
            "workspace_write_with_review allows validated workspace write actions",
        ),
        (
            PermissionPolicyMode::WorkspaceWriteWithReview,
            ActionRequest::CreateFile(_)
            | ActionRequest::CreateDirectory(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_),
        ) => PolicyDecision::require_review(
            mode,
            "workspace_write_with_review gates file writes outside the current workspace",
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

fn action_targets_are_inside_workspace(session: &Session, action: &Action) -> bool {
    let targets = workspace_write_targets(&action.request);
    !targets.is_empty()
        && targets
            .into_iter()
            .all(|target| path_is_within(&absolute_session_path(session, target), &session.cwd))
}

fn workspace_write_targets(request: &ActionRequest) -> Vec<&Path> {
    match request {
        ActionRequest::CreateFile(action) => vec![&action.target_path],
        ActionRequest::CreateDirectory(action) => vec![&action.target_path],
        ActionRequest::OverwriteFile(action) => vec![&action.target_path],
        ActionRequest::PatchFile(action) => vec![&action.target_path],
        ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => Vec::new(),
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

fn push_provider_message_after_tool_turn_if_visible(
    session: &mut Session,
    turn_start_index: usize,
    message: impl Into<String>,
) {
    if turn_has_verified_action_applied(session, turn_start_index) {
        return;
    }

    push_provider_message_if_visible(session, message);
}

fn turn_has_verified_action_applied(session: &Session, turn_start_index: usize) -> bool {
    session
        .events()
        .iter()
        .skip(turn_start_index)
        .any(|event| matches!(event, Event::ActionApplied(_)))
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
    RepairModel(String),
    Error(String),
}

fn tool_validation_recovery(
    error: &crate::model_runtime::ModelToolValidationError,
) -> ToolValidationRecovery {
    if let Some(message) = tool_validation_repair_message(error) {
        return ToolValidationRecovery::RepairModel(message);
    }

    ToolValidationRecovery::Error(format!(
        "{} No filesystem action was applied.",
        friendly_tool_validation_error(error)
    ))
}

fn tool_validation_repair_message(
    error: &crate::model_runtime::ModelToolValidationError,
) -> Option<String> {
    if !is_missing_or_malformed_tool_argument(error) {
        return None;
    }

    let tool = error.tool_name.as_deref().unwrap_or("tool");
    let argument = error.argument.as_deref()?;
    Some(format!(
        "{} Use the original user request and verified session context to send a corrected `{tool}` tool call with `{argument}` included. No filesystem action was applied.",
        friendly_tool_validation_error(error)
    ))
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
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("demo")
                )
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Done."
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
    fn plan_creation_batch_skips_implementation_tool_calls_in_same_response() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-batch-guard",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-batch-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanBatch" }),
                    assistant_summary: Some("create project folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanBatch/src" }),
                    assistant_summary: Some("create source folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/plan.md",
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-4".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-5".to_string(),
                    name: RawModelToolName::Known(ModelToolName::DeleteFile),
                    arguments: json!({ "target_path": "PlanBatch/requirements.txt" }),
                    assistant_summary: Some("delete requirements".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a project plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("PlanBatch").is_dir());
        assert!(root.join("PlanBatch/plan.md").is_file());
        assert!(!root.join("PlanBatch/src").exists());
        assert!(!root.join("PlanBatch/requirements.txt").exists());
        assert!(matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        ));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message
                        .content
                        .contains("Skipped extra implementation tool calls")
        )));
        assert!(session.project_memory().latest_structured_plan().is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_turn_skips_later_implementation_rounds() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-later-round-guard",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-later-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanLater/PROJECT_PLAN.md",
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Continuing.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-later-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanLater/src" }),
                    assistant_summary: Some("create source folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-later-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::DeleteFile),
                    arguments: json!({ "target_path": "PlanLater/src" }),
                    assistant_summary: Some("delete source folder".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan only."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create only the project plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("PlanLater/PROJECT_PLAN.md").is_file());
        assert!(!root.join("PlanLater/src").exists());
        assert!(matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        ));
        assert_eq!(session.actions().len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn execute_plan_skips_redundant_directory_create_when_file_creates_parent() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-redundant-dir",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("demo")).unwrap();
        std::fs::write(
            root.join("demo/PROJECT_PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Executing plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "redundant-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "demo/src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
                RawModelToolCall {
                    id: "redundant-dir-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "demo/src" }),
                    assistant_summary: Some("create src".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: root.join("demo/PROJECT_PLAN.md"),
            project_root: root.join("demo"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("demo/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("demo/src")
                )
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_preflight_allows_unrelated_folder_creation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-unrelated-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("demo")).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating folder.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "outside-folder-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "other-folder" }),
                    assistant_summary: Some("create folder".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: root.join("demo/PROJECT_PLAN.md"),
            project_root: root.join("demo"),
            source_action_id: "action-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create an unrelated folder",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("other-folder").is_dir());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("verified plan is rooted")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_create_file_target_gets_model_repair_without_user_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-create-target",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating the file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-create-target-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({ "contents": "# Plan\n" }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing the file path.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-create-target-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "plan.md",
                        "contents": "# Plan\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create an md file with a plan for projects",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("plan.md")).unwrap(),
            "# Plan\n"
        );
        assert_eq!(session.actions().len(), 1);
        assert_eq!(provider.messages.lock().unwrap().len(), 3);
        assert!(!session.events().iter().any(|event| matches!(
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
    fn verified_plan_execution_anchors_expected_unrooted_paths_under_plan_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-anchor-expected-paths",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("tui-state-memory-test")).unwrap();
        std::fs::write(
            cwd.join("tui-state-memory-test/PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "anchor-expected-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Creating remaining files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "anchor-expected-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("tui-state-memory-test/PLAN.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "tui-state-memory-test/PLAN.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(cwd.join("tui-state-memory-test/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(!cwd.join("src/main.py").exists());
        assert!(cwd.join("tui-state-memory-test/requirements.txt").is_file());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::FileWritten { path }
                        if path == &cwd
                            .join("tui-state-memory-test/src/main.py")
                            .display()
                            .to_string()
                )
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_skips_off_root_directory_and_continues_missing_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-continue-missing",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "off-root-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "src" }),
                    assistant_summary: Some("create src".to_string()),
                },
                RawModelToolCall {
                    id: "main-file-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
                RawModelToolCall {
                    id: "guidance-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::AskGuidance),
                    arguments: json!({
                        "question": "Should I create the src directory inside demo?"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Creating remaining file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "requirements-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("demo/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "demo/plan.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!cwd.join("src").exists());
        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("Should I create the src directory")
        )));
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message.content.contains("Missing expected files")
                && message.content.contains("demo/requirements.txt")));

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
    fn missing_move_source_path_gets_model_repair_without_raw_tool_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-move-source",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Moving the file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-move-source-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::MoveFile),
                    arguments: json!({ "target_path": "renamed.md" }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Which source path should I move?"),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "move the file",
            PermissionPolicyMode::FullAccess,
        );

        assert!(session.actions().is_empty());
        assert_eq!(provider.messages.lock().unwrap().len(), 2);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("source path")
        )));
        assert_no_raw_tool_validation_error(&session);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_shell_cwd_gets_model_repair_without_running_command() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-shell-cwd",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Running the command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-shell-cwd-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({ "command": "printf hello" }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Which working directory should I use?"),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "run a shell command",
            PermissionPolicyMode::FullAccess,
        );

        assert!(session.actions().is_empty());
        assert_eq!(provider.messages.lock().unwrap().len(), 2);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("working directory")
        )));
        assert_no_raw_tool_validation_error(&session);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_patch_find_gets_model_repair_without_raw_tool_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-missing-patch-find",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("notes.md"), "old\n").unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Patching the file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-patch-find-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::PatchFile),
                    arguments: json!({
                        "target_path": "notes.md",
                        "find": "",
                        "replace": "new"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Which exact text should I replace?"),
        ]);
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
        assert_eq!(provider.messages.lock().unwrap().len(), 2);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("exact text")
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
    fn verified_state_classifier_parser_accepts_wrapped_json() {
        assert_eq!(
            parse_verified_state_classifier_output(
                "```json\n{\"answer_kind\":\"latest_folder\"}\n```"
            ),
            Some(VerifiedStateAnswerKind::LatestFolder)
        );
        assert_eq!(
            parse_verified_state_classifier_output("{\"answer_kind\":\"none\"}"),
            None
        );
        assert_eq!(
            parse_verified_state_classifier_output("{\"needs_verified_state\":true}"),
            Some(VerifiedStateAnswerKind::Summary)
        );
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
        plain_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
        tool_output: crate::event::ProviderOutput,
    }

    impl CapturingProvider {
        fn new() -> Self {
            Self {
                requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                plain_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                    crate::event::ProviderOutput::new("Plain answer."),
                ])),
                tool_output: crate::event::ProviderOutput::new("I'll create it."),
            }
        }

        fn with_tool_output(mut self, output: crate::event::ProviderOutput) -> Self {
            self.tool_output = output;
            self
        }

        fn with_plain_output(mut self, output: crate::event::ProviderOutput) -> Self {
            self.plain_outputs = std::sync::Arc::new(std::sync::Mutex::new(vec![output]));
            self
        }

        fn with_plain_outputs(mut self, outputs: Vec<crate::event::ProviderOutput>) -> Self {
            self.plain_outputs = std::sync::Arc::new(std::sync::Mutex::new(outputs));
            self
        }

        fn requests(&self) -> Vec<CapturedProviderRequest> {
            self.requests.lock().unwrap().clone()
        }

        fn next_plain_output(&self) -> crate::event::ProviderOutput {
            let mut outputs = self.plain_outputs.lock().unwrap();
            if outputs.len() > 1 {
                outputs.remove(0)
            } else {
                outputs
                    .first()
                    .cloned()
                    .unwrap_or_else(|| crate::event::ProviderOutput::new("Plain answer."))
            }
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
            Ok(self.next_plain_output())
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
            Ok(self.next_plain_output())
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
        let provider = CapturingProvider::new().with_plain_outputs(vec![
            crate::event::ProviderOutput::new("{\"needs_verified_state\":false}"),
            crate::event::ProviderOutput::new("Hello."),
        ]);
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
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "remembered".to_string(),
            });
        let mut record = ActionRecord::new(folder_action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &folder_action, &result);

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(request.mode, CapturedProviderRequestMode::Plain);
            assert_eq!(request.tool_count, 0);
            let joined = joined_request_messages(request);
            assert!(!joined.contains("latest verified folder"));
            assert!(!joined.contains("remembered"));
            assert!(!joined.contains("Verified filesystem context"));
        }
        assert!(session.latest_provider_prompt_memory_selection().is_none());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Hello."
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_state_question_uses_model_classifier_then_verified_state() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-question-model",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("remembered")).unwrap();
        std::fs::create_dir_all(root.join("latest-folder")).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"answer_kind\":\"latest_folder\"}"),
        );
        let mut session = Session::new("session", &root, &root);
        for (action_id, target_path) in [
            ("action-folder-1", "remembered"),
            ("action-folder-2", "latest-folder"),
        ] {
            let folder_action = Action::proposed(
                action_id,
                ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                    target_path: PathBuf::from(target_path),
                }),
                "create folder",
            )
            .approve()
            .mark_applied();
            let result = VerifiedActionResult::File(
                crate::event::FileActionVerification::DirectoryCreated {
                    path: target_path.to_string(),
                },
            );
            let mut record = ActionRecord::new(folder_action.clone());
            record.verified_result = Some(result.clone());
            session.push_action(record);
            record_verified_project_memory(&mut session, &folder_action, &result);
        }

        run_permissive_agent_turn(&provider, &mut session, "what did you create?");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(
            requests[0].messages.last(),
            Some(&ChatMessage::user("what did you create?"))
        );
        let classifier = joined_request_messages(&requests[0]);
        assert!(!classifier.contains("latest verified folder"));
        assert!(!classifier.contains("remembered"));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content == "latest-folder"
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("tool_enabled")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_state_answer_keeps_latest_folder_ahead_of_created_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-latest-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("session", &root, &root);

        for (action_id, request, result) in [
            (
                "action-file",
                ActionRequest::CreateFile(crate::action::CreateFileAction {
                    target_path: PathBuf::from("demo/requirements.txt"),
                    contents: String::new(),
                }),
                VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                    path: "demo/requirements.txt".to_string(),
                }),
            ),
            (
                "action-folder",
                ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                    target_path: PathBuf::from("unrelated"),
                }),
                VerifiedActionResult::File(
                    crate::event::FileActionVerification::DirectoryCreated {
                        path: "unrelated".to_string(),
                    },
                ),
            ),
        ] {
            let action = Action::proposed(action_id, request, "apply")
                .approve()
                .mark_applied();
            let mut record = ActionRecord::new(action.clone());
            record.verified_result = Some(result.clone());
            session.push_action(record);
            record_verified_project_memory(&mut session, &action, &result);
        }

        let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::Summary);

        assert!(answer.contains("latest folder: unrelated"));
        assert!(answer.contains("latest file: demo/requirements.txt"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_created_summary_uses_verified_action_records() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-created-summary",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"answer_kind\":\"created_summary\"}"),
        );
        let mut session = Session::new("session", &root, &root);

        for (action_id, request, result) in [
            (
                "action-folder",
                ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                    target_path: PathBuf::from("demo"),
                }),
                VerifiedActionResult::File(
                    crate::event::FileActionVerification::DirectoryCreated {
                        path: "demo".to_string(),
                    },
                ),
            ),
            (
                "action-file",
                ActionRequest::CreateFile(crate::action::CreateFileAction {
                    target_path: PathBuf::from("demo/requirements.txt"),
                    contents: String::new(),
                }),
                VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                    path: "demo/requirements.txt".to_string(),
                }),
            ),
        ] {
            let action = Action::proposed(action_id, request, "apply")
                .approve()
                .mark_applied();
            let mut record = ActionRecord::new(action.clone());
            record.verified_result = Some(result.clone());
            session.push_action(record);
            record_verified_project_memory(&mut session, &action, &result);
        }

        run_permissive_agent_turn(&provider, &mut session, "what did you create?");

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content == "directory demo\nfile demo/requirements.txt"
        )));
        assert_eq!(provider.requests().len(), 1);

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
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("{\"needs_verified_state\":false}"),
                crate::event::ProviderOutput::new("Ok."),
            ])
            .with_tool_output(
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
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(request.mode, CapturedProviderRequestMode::Plain);
            assert_eq!(request.tool_count, 0);
        }
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
