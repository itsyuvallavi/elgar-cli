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
    controller_shell_verify::verify_expected_shell_effect,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderOutput, ProviderStarted, UserMessage,
        VerifiedActionResult,
    },
    fs::Filesystem,
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ModelToolValidationErrorKind,
        RawModelToolCall, ValidatedModelGuidanceRequest, ValidatedModelToolAction,
        ValidatedModelToolOutput,
    },
    normal_turn_decision::{
        parse_normal_turn_decision, NormalTurnDecision, NormalTurnExecuteIntent,
    },
    path_resolution::{
        allowed_root_for_action, resolve_agent_action_paths,
        resolve_shell_action_paths_for_session, AgentPathResolution,
    },
    plan_contract::{PlanContractDraftIssue, PlanContractDraftIssueKind},
    policy::{PermissionPolicyMode, PolicyDecision},
    provider::{
        ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction, ControllerProvider,
        ProviderErrorKind,
    },
    provider_visible_text_from_text_only_output,
    router::Route,
    session::{
        ActionRecord, PendingActionSelection, ProviderPromptMemorySelectedFact,
        ProviderPromptMemorySelection, Session, StructuredProjectPlanStatus,
        VerifiedFolderReference, VerifiedPlanReference,
    },
    shell::ShellExecutor,
    verified_state_answer::verified_session_state_answer,
};

const AGENT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar, a permissive terminal-native coding agent. ",
    "Use tools to do the user's requested filesystem and shell work directly. ",
    "Do not ask for approval. Do not give instructions instead of acting when a tool can do it. ",
    "Ask one concise clarification question only when the target or intent is truly ambiguous. ",
    "If the user asks you to choose, choose a reasonable option and continue the prior request. ",
    "If the user asks for a plan and says to share it before implementation, create or update a plan file and summarize it; do not implement project files until asked. ",
    "If the user asks to create only a plan file with a future file tree, create only that plan file; do not ask whether to create the listed future files. ",
    "Plan files must include a concrete file tree, a Verification section, and an Acceptance Criteria section before implementation. ",
    "Verified plans guide runtime validation but do not make completed files immutable; if the user requests an edit under a verified plan root, use the appropriate file tool and let runtime validation, policy, and executors decide. ",
    "If the user asks what the plan is, summarize the existing plan; do not implement it. ",
    "If a verified plan already exists and the user gives a short choice follow-up, answer from that plan instead of recreating the same file. ",
    "When creating a framework project, infer the necessary starter files from the requested stack and create the complete runnable scaffold before the final answer. ",
    "After tools run, answer naturally and briefly with what happened."
);
const AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar. ",
    "Return one compact JSON object; no prose. ",
    "Route by required capability; do not draft artifacts here. ",
    "Use {\"route\":\"execute\"} when the request needs a persistent local change or verified filesystem, shell, artifact, plan, or project result. ",
    "Use {\"route\":\"chat\",\"content\":\"...\"} only for text answers with no local side effect. ",
    "Use {\"route\":\"execute\",\"intent\":\"plan_execution\"} only to apply implementation files now; plan artifact work is execute without intent. ",
    "Use {\"route\":\"state\",\"answer_kind\":\"...\"} for verified-state inspection. ",
    "Use {\"route\":\"ask_guidance\",\"question\":\"...\"} if a required detail is missing. ",
    "Runtime supplies verified context after routing."
);
const AGENT_ROUTE_JSON_REPAIR_PROMPT: &str = concat!(
    "The previous no-tool routing response was not valid route JSON. ",
    "Return exactly one compact JSON object for the original user request using the routing schema. ",
    "Do not answer in prose and do not draft artifacts."
);
const AGENT_PLAIN_FALLBACK_SYSTEM_PROMPT: &str = concat!(
    "The previous tool-enabled provider request returned no usable response. ",
    "Answer normally in prose. Do not claim filesystem or shell changes unless they were already reported by verified tool results."
);

const MAX_AGENT_TOOL_ROUNDS: usize = 16;
const TOOL_COMMAND_PREFIX: &str = "/tool";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlainAgentChatOutcome {
    Finished,
    Execute(AgentExecutionIntent),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AgentExecutionIntent {
    plan_execution: bool,
}

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
    session.start_reasoning_trace(input);

    let Some(tool_input) = explicit_tool_command_input(input) else {
        if let PlainAgentChatOutcome::Execute(intent) =
            run_plain_agent_chat(provider, session, input)
        {
            return run_agent_tool_chat(provider, session, input, policy_mode, start_index, intent);
        }
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

    session.record_reasoning_route("execute");
    session.push_reasoning_model_decision("explicit /tool route selected");
    run_agent_tool_chat(
        provider,
        session,
        tool_input,
        policy_mode,
        start_index,
        AgentExecutionIntent::default(),
    )
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
    session.start_reasoning_trace(input);
    session.record_reasoning_route("execute");
    session.push_reasoning_model_decision("tool-enabled turn started");
    run_agent_tool_chat(
        provider,
        session,
        input,
        policy_mode,
        start_index,
        AgentExecutionIntent::default(),
    )
}

fn push_provider_finished(
    session: &mut Session,
    provider: String,
    request_id: String,
    output: ProviderOutput,
) {
    if let Some(metrics) = output.metrics.as_ref() {
        session.record_provider_metrics(metrics);
    }
    session.push_event(Event::ProviderFinished(ProviderFinished::new(
        provider, request_id, output,
    )));
}

fn run_agent_tool_chat<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    policy_mode: PermissionPolicyMode,
    start_index: usize,
    intent: AgentExecutionIntent,
) -> TurnResult
where
    P: ControllerProvider,
{
    session.push_reasoning_runtime_check(format!("policy: {policy_mode:?}"));
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
    if intent.plan_execution
        && session
            .project_memory()
            .latest_structured_plan()
            .is_some_and(|plan| plan.runtime_status() == StructuredProjectPlanStatus::Completed)
    {
        session.record_reasoning_route("plan_execution");
        session.push_reasoning_runtime_check(
            "latest structured plan already complete; skipped tool loop",
        );
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "The latest verified plan is already complete. No filesystem changes were needed.",
            AssistantMessageSource::Controller,
        )));
        return TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        };
    }
    if intent.plan_execution {
        if let Some(message) = missing_expected_plan_paths_message(session) {
            session.push_reasoning_runtime_check(
                "seeded verified plan execution contract before first tool request",
            );
            messages.push(ChatMessage::system(format!(
                "Verified plan execution contract:\n{message}\nCreate all missing expected paths in one tool response when possible."
            )));
        }
    }
    messages.push(ChatMessage::user(input));
    let tools = elgar_model_tool_definitions();
    let mut handled_tool_call_ids = HashSet::new();
    let mut plan_created_this_turn = false;
    let mut plan_execution_in_progress = false;
    let mut plan_creation_repair_in_progress = false;
    let mut visible_skipped_tool_notice_shown = false;

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
                            push_provider_finished(
                                session,
                                fallback_request.provider,
                                fallback_request.request_id,
                                output,
                            );
                            if plan_execution_in_progress {
                                if let Some(message) =
                                    plan_execution_repair_message_or_mark_complete(session)
                                {
                                    messages.push(ChatMessage::system(message));
                                    continue;
                                }
                            }
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
        let assistant_thinking = output.thinking.clone();
        record_provider_planning_trace(session, assistant_thinking.as_deref(), &assistant_text);
        push_provider_finished(session, request.provider, request.request_id, output);

        if tool_calls.is_empty() {
            if plan_creation_repair_in_progress {
                messages.push(ChatMessage::system(plan_creation_repair_message(session)));
                continue;
            }
            if plan_execution_in_progress {
                if let Some(message) = plan_execution_repair_message_or_mark_complete(session) {
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
        record_validated_tool_output_trace(session, &outputs);

        let path_resolution = AgentPathResolution::new(None, None, &session.project_root);
        let resolved_outputs = anchor_verified_folder_tool_outputs(
            session,
            resolve_agent_tool_outputs(outputs, &path_resolution),
        );
        let resolved_outputs = anchor_verified_plan_tool_outputs(session, resolved_outputs);
        let resolved_outputs = guard_plan_creation_tool_outputs(
            session,
            resolved_outputs,
            plan_created_this_turn,
            plan_creation_repair_in_progress,
        );
        let resolved_outputs = guard_redundant_directory_tool_outputs(session, resolved_outputs);
        let plan_execution_batch =
            resolved_outputs_touch_structured_plan(session, &resolved_outputs);
        if plan_execution_batch {
            session.record_reasoning_route("plan_execution");
            session.push_reasoning_runtime_check("plan execution paths detected");
            if latest_plan_contract_needs_repair(session) {
                let message = plan_execution_blocked_by_contract_repair_message(session);
                session.push_reasoning_runtime_check(message.clone());
                plan_creation_repair_in_progress = true;
                for tool_call_id in resolved_outputs_tool_call_ids(&resolved_outputs) {
                    messages.push(ChatMessage::tool(tool_call_id, message.clone()));
                }
                messages.push(ChatMessage::system(plan_creation_repair_message(session)));
                continue;
            }
        }
        let starts_plan_execution = plan_execution_batch && !plan_execution_in_progress;
        plan_execution_in_progress |= plan_execution_batch;
        let resolved_outputs = guard_plan_execution_tool_outputs(
            session,
            resolved_outputs,
            plan_execution_in_progress,
        );

        if let Err(message) = preflight_verified_plan_tool_outputs(session, &resolved_outputs) {
            session.push_reasoning_runtime_check(format!("preflight blocked: {message}"));
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                message,
                AssistantMessageSource::Controller,
            )));
            break;
        }
        if starts_plan_execution {
            session.push_reasoning_runtime_check("latest structured plan marked executing");
            session.mark_latest_structured_project_plan_executing();
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

        let mut tool_results_need_provider_followup = false;
        for output in resolved_outputs {
            match output {
                ResolvedAgentToolOutput::Guidance(guidance) => {
                    tool_results_need_provider_followup = true;
                    session.push_reasoning_model_decision(format!(
                        "guidance requested: {}",
                        guidance.question
                    ));
                    push_provider_message_if_visible(session, guidance.question.clone());
                    messages.push(ChatMessage::tool(guidance.tool_call_id, guidance.question));
                }
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id,
                    message,
                    visible,
                } => {
                    tool_results_need_provider_followup = true;
                    session.push_reasoning_runtime_check(format!("skipped: {message}"));
                    if visible && !visible_skipped_tool_notice_shown {
                        session.push_event(Event::AssistantMessage(AssistantMessage::new(
                            message.clone(),
                            AssistantMessageSource::Controller,
                        )));
                        visible_skipped_tool_notice_shown = true;
                    }
                    messages.push(ChatMessage::tool(tool_call_id, message));
                }
                ResolvedAgentToolOutput::Action(action) => {
                    let is_plan_creation =
                        plan_creation_root_for_action(session, &action.request).is_some();
                    if is_plan_creation {
                        session.record_reasoning_route("plan_creation");
                        session.push_reasoning_runtime_check(format!(
                            "plan detected: {}",
                            action.request.approval_target()
                        ));
                    }
                    let result = apply_agent_action_with_policy(
                        session,
                        action.request,
                        action.summary,
                        policy_mode,
                    );
                    session.push_reasoning_runtime_check(format!("action result: {result}"));
                    if is_plan_creation
                        && session
                            .actions()
                            .last()
                            .and_then(|record| record.verified_result.as_ref())
                            .is_some()
                    {
                        plan_created_this_turn = true;
                        plan_creation_repair_in_progress =
                            latest_plan_contract_needs_repair(session);
                        if plan_creation_repair_in_progress {
                            messages
                                .push(ChatMessage::system(plan_creation_repair_message(session)));
                        }
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
            if let Some(message) = plan_execution_repair_message_or_mark_complete(session) {
                messages.push(ChatMessage::system(message));
                continue;
            }
            if !tool_results_need_provider_followup {
                session.push_reasoning_runtime_check(
                    "plan execution completed; skipped final provider synthesis",
                );
                break;
            }
        }
        if plan_created_this_turn
            && !plan_creation_repair_in_progress
            && !plan_execution_in_progress
        {
            session.push_reasoning_runtime_check(
                "plan creation completed; skipped final provider synthesis",
            );
            break;
        }
    }

    if plan_creation_repair_in_progress && latest_plan_contract_needs_repair(session) {
        let message = plan_creation_needs_revision_notice(session);
        session.push_reasoning_runtime_check(message.clone());
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            message,
            AssistantMessageSource::Controller,
        )));
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

fn record_provider_planning_trace(
    session: &mut Session,
    thinking: Option<&str>,
    assistant_text: &str,
) {
    if let Some(thinking) = thinking.filter(|value| !value.trim().is_empty()) {
        session.push_reasoning_provider_planning(format!("thinking: {}", thinking.trim()));
    }

    let text = assistant_text.trim();
    if !text.is_empty() && !looks_like_raw_tool_protocol(text) {
        session.push_reasoning_provider_planning(format!("visible text: {text}"));
    }
}

fn record_validated_tool_output_trace(session: &mut Session, outputs: &[ValidatedModelToolOutput]) {
    for output in outputs {
        match output {
            ValidatedModelToolOutput::Action(action) => {
                session.push_reasoning_model_decision(format!(
                    "requested {}: {}",
                    action_request_kind_label(&action.request),
                    action.request.approval_target()
                ));
            }
            ValidatedModelToolOutput::Guidance(guidance) => {
                session.push_reasoning_model_decision(format!(
                    "requested guidance: {}",
                    guidance.question
                ));
            }
        }
    }
}

fn action_request_kind_label(request: &ActionRequest) -> &'static str {
    match request {
        ActionRequest::CreateFile(_) => "create_file",
        ActionRequest::CreateDirectory(_) => "create_directory",
        ActionRequest::OverwriteFile(_) => "overwrite_file",
        ActionRequest::PatchFile(_) => "patch_file",
        ActionRequest::DeleteFile(_) => "delete_file",
        ActionRequest::MoveFile(_) => "move_file",
        ActionRequest::ShellCommand(_) => "shell_command",
    }
}

fn run_plain_agent_chat<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_chat", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            handle_plain_agent_decision(provider, session, input, assistant_text, true, true)
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
            PlainAgentChatOutcome::Finished
        }
    }
}

fn retry_plain_agent_chat_with_verified_context<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    let Some(context) = agent_verified_memory_context(session).prompt_context else {
        return PlainAgentChatOutcome::Finished;
    };
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_chat_context", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(context),
        ChatMessage::system(
            "Verified context is available for this retry. If it resolves the missing detail, choose the appropriate route instead of asking for guidance.",
        ),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            handle_plain_agent_decision(provider, session, input, assistant_text, false, true)
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider context retry request {} failed: {error}",
                request.provider, request.request_id
            ))));
            PlainAgentChatOutcome::Finished
        }
    }
}

fn retry_plain_agent_chat_for_route_json<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_route_retry", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(AGENT_ROUTE_JSON_REPAIR_PROMPT),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            handle_plain_agent_decision(provider, session, input, assistant_text, false, false)
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider route retry request {} failed: {error}",
                request.provider, request.request_id
            ))));
            PlainAgentChatOutcome::Finished
        }
    }
}

fn handle_plain_agent_decision<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    assistant_text: String,
    allow_context_retry: bool,
    allow_route_retry: bool,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    if looks_like_raw_tool_protocol(&assistant_text) {
        session.record_reasoning_route("execute");
        session.push_reasoning_model_decision(
            "normal turn decision returned raw tool protocol; routed to execute",
        );
        return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
    }
    match parse_normal_turn_decision(&assistant_text) {
        Some(NormalTurnDecision::Execute { intent }) => {
            let execution_intent = AgentExecutionIntent {
                plan_execution: matches!(intent, Some(NormalTurnExecuteIntent::PlanExecution)),
            };
            session.record_reasoning_route("execute");
            if execution_intent.plan_execution {
                session.push_reasoning_model_decision(
                    "normal turn decision selected execute intent plan_execution",
                );
            } else {
                session.push_reasoning_model_decision("normal turn decision selected execute");
            }
            PlainAgentChatOutcome::Execute(execution_intent)
        }
        Some(NormalTurnDecision::State { answer_kind }) => {
            session.record_reasoning_route("state");
            session.push_reasoning_model_decision("normal turn decision selected state");
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                verified_session_state_answer(session, answer_kind),
                AssistantMessageSource::Controller,
            )));
            PlainAgentChatOutcome::Finished
        }
        Some(NormalTurnDecision::Chat { content }) => {
            if looks_like_misrouted_artifact_chat(&content) {
                if !allow_route_retry {
                    session.record_reasoning_route("execute");
                    session.push_reasoning_model_decision(
                        "normal turn decision returned artifact-like chat after retry; routed to execute",
                    );
                    return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
                }
                session.push_reasoning_model_decision(
                    "normal turn decision returned artifact-like chat; retrying route JSON",
                );
                return retry_plain_agent_chat_for_route_json(provider, session, input);
            }
            if !allow_route_retry && looks_like_misrouted_artifact_chat_after_retry(&content) {
                session.record_reasoning_route("execute");
                session.push_reasoning_model_decision(
                    "normal turn decision returned compact artifact-like chat after retry; routed to execute",
                );
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
            }
            session.record_reasoning_route("chat");
            session.push_reasoning_model_decision("normal turn decision selected chat");
            push_plain_provider_message_if_visible(session, content);
            PlainAgentChatOutcome::Finished
        }
        Some(NormalTurnDecision::AskGuidance { question }) => {
            if allow_context_retry && has_verified_session_state(session) {
                session.push_reasoning_model_decision(
                    "normal turn decision requested guidance; retrying with verified context",
                );
                return retry_plain_agent_chat_with_verified_context(provider, session, input);
            }
            session.record_reasoning_route("ask_guidance");
            session.push_reasoning_model_decision("normal turn decision selected ask_guidance");
            push_plain_provider_message_if_visible(session, question);
            PlainAgentChatOutcome::Finished
        }
        None => {
            if allow_route_retry {
                session.push_reasoning_model_decision(
                    "normal turn decision did not return structured JSON; retrying route JSON",
                );
                return retry_plain_agent_chat_for_route_json(provider, session, input);
            }
            session.record_reasoning_route("ask_guidance");
            session.push_reasoning_model_decision(
                "normal turn decision did not return structured JSON after retry",
            );
            session.push_event(Event::Error(ErrorEvent::new(
                "Model routing response was not valid JSON; no filesystem action was applied.",
            )));
            PlainAgentChatOutcome::Finished
        }
    }
}

fn looks_like_misrouted_artifact_chat(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 1000 && path_count >= 3)
}

fn looks_like_misrouted_artifact_chat_after_retry(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 500 && path_count >= 3)
}

fn local_path_like_token_count(content: &str) -> usize {
    let mut paths = Vec::<String>::new();
    for line in content.lines() {
        let line = line.trim().trim_start_matches(|ch: char| {
            matches!(
                ch,
                '-' | '*' | '+' | '|' | '`' | '"' | '\'' | '[' | ']' | '(' | '├' | '└' | '│' | '─'
            ) || ch.is_ascii_digit()
                || ch == '.'
                || ch.is_whitespace()
        });
        for token in line
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '|' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '"' | '\'' | '{' | '}'
                    )
            })
            .filter(|part| !part.is_empty())
        {
            let token = token
                .trim_start_matches(|ch: char| {
                    matches!(ch, '-' | '*' | '+' | '├' | '└' | '│' | '─')
                })
                .trim_matches('`')
                .trim_end_matches('/');
            if token.is_empty()
                || token.contains("://")
                || token.contains('=')
                || token.starts_with('$')
                || token.starts_with('~')
            {
                continue;
            }
            let path = std::path::Path::new(token);
            let path_like = token.contains('/')
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with('.')
                            || path
                                .extension()
                                .and_then(|extension| extension.to_str())
                                .is_some_and(|extension| {
                                    !extension.is_empty()
                                        && extension.len() <= 12
                                        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
                                })
                    });
            if path_like && !paths.iter().any(|seen| seen == token) {
                paths.push(token.to_string());
            }
        }
    }
    paths.len()
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
    plan_creation_repair_in_progress: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if plan_creation_repair_in_progress {
        return outputs
            .into_iter()
            .map(|output| match output {
                ResolvedAgentToolOutput::Guidance(guidance) => {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: guidance.tool_call_id,
                        message: plan_creation_repair_message(session),
                        visible: false,
                    }
                }
                ResolvedAgentToolOutput::Action(action)
                    if is_latest_verified_plan_file_action(session, &action.request) =>
                {
                    ResolvedAgentToolOutput::Action(action)
                }
                ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped non-plan repair action. Update the same verified plan file before execution.".to_string(),
                    visible: false,
                },
                skipped => skipped,
            })
            .collect();
    }

    let plan_roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => {
                plan_creation_root_for_action(session, &action.request)
            }
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if plan_roots.is_empty() && !plan_created_this_turn && !plan_creation_repair_in_progress {
        return outputs;
    }

    let mut allowed_plan_file_used = false;
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if (plan_created_this_turn || plan_creation_repair_in_progress)
            =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped implementation tool calls after creating the verified plan. Ask to execute the plan when you want to apply it.".to_string(),
                    visible: true,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if plan_creation_root_for_action(session, &action.request).is_some() =>
            {
                if !allowed_plan_file_used {
                    allowed_plan_file_used = true;
                    ResolvedAgentToolOutput::Action(action)
                } else {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                        visible: true,
                    }
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if is_plan_parent_setup_action(session, &action.request, &plan_roots) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action)
                if !plan_roots.is_empty() && !plan_created_this_turn =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                    visible: true,
                }
            }
            ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: action.tool_call_id,
                message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                visible: true,
            },
            other => other,
        })
        .collect()
}

fn resolved_outputs_tool_call_ids(outputs: &[ResolvedAgentToolOutput]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => guidance.tool_call_id.clone(),
            ResolvedAgentToolOutput::Action(action) => action.tool_call_id.clone(),
            ResolvedAgentToolOutput::Skipped { tool_call_id, .. } => tool_call_id.clone(),
        })
        .collect()
}

fn latest_plan_contract_needs_repair(session: &Session) -> bool {
    session
        .latest_plan_contract()
        .is_some_and(|contract| !contract.review_draft().is_approvable())
}

fn is_latest_verified_plan_file_action(session: &Session, request: &ActionRequest) -> bool {
    let Some(contract) = session.latest_plan_contract() else {
        return false;
    };
    let target_path = match request {
        ActionRequest::CreateFile(action) => &action.target_path,
        ActionRequest::OverwriteFile(action) => &action.target_path,
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => return false,
    };

    absolute_session_path(session, target_path) == normalize_path(&contract.source_plan_path)
}

fn plan_creation_repair_message(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "The verified plan draft is not ready. Update the same plan file with a concrete file tree, Verification section, and Acceptance Criteria section before creating implementation files.".to_string();
    };
    let review = contract.review_draft();
    let mut lines = vec![
        "The verified plan draft is not approvable yet.".to_string(),
        "Update the same plan file before creating implementation files.".to_string(),
        "Blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push("The plan file must include a concrete fenced file tree or path list, a `Verification` section with bullet checks, and an `Acceptance Criteria` section with bullet criteria.".to_string());
    lines.push("Do not ask the user whether to rename the project root.".to_string());
    lines.push("Keep the existing project root and choose valid package or module names inside it, for example by using underscores for Python package paths.".to_string());
    lines.push("If verification or acceptance criteria reference a file path, include that path in the plan scope.".to_string());
    lines.join("\n")
}

fn plan_creation_needs_revision_notice(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "The plan needs revision before execution. Review /plan for details.".to_string();
    };
    let review = contract.review_draft();
    let mut lines = vec![
        "The plan needs revision before execution.".to_string(),
        "Blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push("Use /plan to review the current contract details.".to_string());
    lines.join("\n")
}

fn plan_execution_blocked_by_contract_repair_message(session: &Session) -> String {
    let Some(contract) = session.latest_plan_contract() else {
        return "Cannot execute the plan yet. Update the verified plan before creating implementation files.".to_string();
    };
    let review = contract.review_draft();
    let plan_path = display_agent_context_path(session, &contract.source_plan_path);
    let mut lines = vec![
        "Cannot execute the plan yet.".to_string(),
        "The plan contract has blocking issues:".to_string(),
    ];
    for issue in review.issues.iter().filter(|issue| {
        issue.severity == crate::plan_contract::PlanContractDraftIssueSeverity::Blocking
    }) {
        lines.push(format!("- {}", plan_draft_issue_message(issue)));
    }
    lines.push(format!(
        "Update the same verified plan file `{plan_path}` to fix these blockers before creating implementation files."
    ));
    lines.push("Do not create implementation files in this repair step.".to_string());
    lines.push("Do not ask the user whether to rename the project root; keep the existing project root and choose valid package or module names inside it.".to_string());
    lines.join("\n")
}

fn plan_draft_issue_message(issue: &PlanContractDraftIssue) -> String {
    let path = issue
        .path
        .as_ref()
        .map(|path| format!(": {}", path.display()))
        .unwrap_or_default();
    match &issue.kind {
        PlanContractDraftIssueKind::ContractNotDraft { status } => {
            format!("plan contract is not a draft ({status:?})")
        }
        PlanContractDraftIssueKind::MissingSourcePlan => format!("missing source plan{path}"),
        PlanContractDraftIssueKind::MissingProjectRoot => format!("missing project root{path}"),
        PlanContractDraftIssueKind::SourcePlanOutsideProjectRoot => {
            format!("source plan is outside the project root{path}")
        }
        PlanContractDraftIssueKind::EmptyExecutableScope => {
            "no executable expected paths; include a concrete fenced file tree or path list"
                .to_string()
        }
        PlanContractDraftIssueKind::PathOutsideProjectRoot => {
            format!("planned path is outside the project root{path}")
        }
        PlanContractDraftIssueKind::MalformedScopePath => {
            format!("planned path is malformed{path}")
        }
        PlanContractDraftIssueKind::ReferencedPathMissingFromScope => {
            format!("referenced path is missing from the plan scope{path}")
        }
        PlanContractDraftIssueKind::InvalidPythonModuleReference { module } => {
            format!("invalid Python module reference `{module}`")
        }
        PlanContractDraftIssueKind::DuplicateScopePath => {
            format!("duplicate planned path{path}")
        }
        PlanContractDraftIssueKind::MissingVerificationSteps => {
            "missing `Verification` section with bullet checks".to_string()
        }
        PlanContractDraftIssueKind::MissingAcceptanceCriteria => {
            "missing `Acceptance Criteria` section with bullet criteria".to_string()
        }
    }
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

fn anchor_verified_folder_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_verified_folder_action_request(session, action.request);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn anchor_verified_folder_action_request(
    session: &Session,
    request: ActionRequest,
) -> ActionRequest {
    if session.project_memory().latest_structured_plan().is_some() {
        return request;
    }
    if plan_creation_root_for_action(session, &request).is_some() {
        return request;
    }

    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path = anchor_verified_folder_create_path(session, &action.target_path);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::DeleteFile(mut action) => {
            action.target_path = anchor_verified_folder_existing_path(session, &action.target_path);
            ActionRequest::DeleteFile(action)
        }
        ActionRequest::MoveFile(mut action) => {
            let anchored_source =
                anchor_verified_folder_existing_path(session, &action.source_path);
            let source_was_anchored = anchored_source != action.source_path;
            action.source_path = anchored_source;
            if source_was_anchored {
                action.target_path =
                    anchor_path_under_verified_folder(session, &action.target_path)
                        .unwrap_or(action.target_path);
            } else {
                action.target_path =
                    anchor_verified_folder_create_path(session, &action.target_path);
            }
            ActionRequest::MoveFile(action)
        }
        ActionRequest::CreateDirectory(_) | ActionRequest::ShellCommand(_) => request,
    }
}

fn anchor_verified_folder_existing_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() || absolute_session_path(session, path).exists() {
        return path.to_path_buf();
    }

    anchor_path_under_verified_folder(session, path)
        .filter(|candidate| absolute_session_path(session, candidate).exists())
        .unwrap_or_else(|| path.to_path_buf())
}

fn anchor_verified_folder_create_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() || absolute_session_path(session, path).exists() {
        return path.to_path_buf();
    }

    anchor_path_under_verified_folder(session, path)
        .filter(|candidate| {
            absolute_session_path(session, candidate)
                .parent()
                .is_some_and(Path::exists)
        })
        .unwrap_or_else(|| path.to_path_buf())
}

fn anchor_path_under_verified_folder(session: &Session, path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }
    let folder = latest_verified_folder_for_prompt(session)?;
    let current_target = absolute_session_path(session, path);
    if path_is_within(&current_target, &folder.path) {
        return None;
    }
    let anchored_target = normalize_path(folder.path.join(path));
    Some(cwd_relative_path(session, &anchored_target))
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
        .any(|path| {
            let path = absolute_session_path(session, path);
            structured_plan_expects_path(plan, &path)
                || structured_plan_expects_child_under(plan, &path)
        })
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
            ResolvedAgentToolOutput::Action(action)
                if is_existing_plan_execution_directory(session, plan, &action.request) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped directory creation because the verified plan directory already exists.".to_string(),
                    visible: false,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if is_nonconstructive_plan_execution_action(session, plan, &action.request) =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped tool call because it does not create a missing expected path from the verified plan.".to_string(),
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

fn is_existing_plan_execution_directory(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    let ActionRequest::CreateDirectory(action) = request else {
        return false;
    };
    let target_path = absolute_session_path(session, &action.target_path);
    structured_plan_expects_path(plan, &target_path) && target_path.is_dir()
}

fn is_nonconstructive_plan_execution_action(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    match request {
        ActionRequest::CreateFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path) || target_path.is_file()
        }
        ActionRequest::OverwriteFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path) || target_path.is_file()
        }
        ActionRequest::CreateDirectory(_) => false,
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => true,
    }
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
    if !structured_plan_expects_path(plan, &anchored_target)
        && !structured_plan_expects_child_under(plan, &anchored_target)
    {
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

fn structured_plan_expects_child_under(
    plan: &crate::session::StructuredProjectPlan,
    directory: &Path,
) -> bool {
    let directory = normalize_path(directory);
    plan.expected_files
        .iter()
        .chain(plan.expected_directories.iter())
        .any(|expected| {
            let expected = normalize_path(expected);
            expected != directory && path_is_within(&expected, &directory)
        })
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
        .flat_map(|action| plan_preflight_paths(session, &action.request))
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

fn plan_preflight_paths<'a>(session: &Session, request: &'a ActionRequest) -> Vec<&'a Path> {
    if plan_creation_root_for_action(session, request).is_some() {
        return Vec::new();
    }

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

fn missing_expected_plan_paths_message(session: &Session) -> Option<String> {
    let missing_directories = missing_expected_plan_directories(session);
    let missing_files = missing_expected_plan_files(session);
    if missing_directories.is_empty() && missing_files.is_empty() {
        return None;
    }

    let mut lines = vec!["The verified plan is not complete.".to_string()];
    if !missing_directories.is_empty() {
        lines.push("Missing expected directories:".to_string());
        lines.extend(
            missing_directories
                .iter()
                .map(|path| format!("- {}", display_agent_context_path(session, path))),
        );
    }
    if !missing_files.is_empty() {
        lines.push("Missing expected files:".to_string());
        lines.extend(
            missing_files
                .iter()
                .map(|path| format!("- {}", display_agent_context_path(session, path))),
        );
    }
    lines.push("Use create_directory for missing expected directories and create_file for missing expected files under the verified plan root. Do not ask whether to create expected paths.".to_string());
    Some(lines.join("\n"))
}

fn plan_execution_repair_message_or_mark_complete(session: &mut Session) -> Option<String> {
    let message = missing_expected_plan_paths_message(session);
    if message.is_none() {
        session.mark_latest_structured_project_plan_completed();
    }
    message
}

fn missing_expected_plan_directories(session: &Session) -> Vec<PathBuf> {
    session
        .project_memory()
        .latest_structured_plan()
        .map(|plan| {
            plan.expected_directories
                .iter()
                .filter(|path| !path.is_dir())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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
    missing_expected_plan_paths_message(session).unwrap_or_else(|| {
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

    let execution_action = resolve_shell_action_paths_for_session(session, &action);
    let result: Result<VerifiedActionResult, String> = match &execution_action.request {
        ActionRequest::ShellCommand(shell) => ShellExecutor::execute(shell)
            .map_err(|error| error.to_string())
            .and_then(|result| verify_expected_shell_effect(shell, result)),
        _ => Filesystem::apply_file_action(&action, allowed_root_for_action(session, &action))
            .map_err(|error| error.to_string()),
    };

    match result {
        Ok(result) => record_agent_action_success(session, index, &execution_action, result),
        Err(reason) => {
            let record = session
                .action_mut(index)
                .expect("agent action index must reference an action record");
            record.verified_result = None;
            record.failure_reason = Some(reason.clone());
            record.action = execution_action.mark_failed();
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
            "The model returned raw tool protocol as text, so no filesystem action was executed. Ask again normally so the model can choose the execute route.",
            AssistantMessageSource::Controller,
        )));
        return;
    }

    push_provider_message_if_visible(session, message);
}

fn looks_like_raw_tool_protocol(message: &str) -> bool {
    [
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
    let repair_instruction = error
        .argument
        .as_deref()
        .map(|argument| format!("with `{argument}` included"))
        .unwrap_or_else(|| "with all required arguments included".to_string());
    Some(format!(
        "{} Use the original user request and verified session context to send a corrected `{tool}` tool call {repair_instruction}. No filesystem action was applied.",
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
    if let Some(folder) = latest_verified_folder_for_prompt(session) {
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
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        lines.push(format!(
            "- latest structured plan root: {}",
            display_agent_context_path(session, &plan.project_root)
        ));
        let missing_directories = plan
            .expected_directories
            .iter()
            .filter(|path| !path.is_dir())
            .map(|path| display_agent_context_path(session, path))
            .collect::<Vec<_>>();
        let missing_files = plan
            .expected_files
            .iter()
            .filter(|path| !path.is_file())
            .map(|path| display_agent_context_path(session, path))
            .collect::<Vec<_>>();
        if !missing_directories.is_empty() {
            lines.push(format!(
                "- missing expected directories:\n{}",
                missing_directories
                    .iter()
                    .map(|path| format!("  - {path}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !missing_files.is_empty() {
            lines.push(format!(
                "- missing expected files:\n{}",
                missing_files
                    .iter()
                    .map(|path| format!("  - {path}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !missing_directories.is_empty() || !missing_files.is_empty() {
            lines.push(
                "- when applying this incomplete structured plan, create all missing expected paths in one tool response when possible"
                    .to_string(),
            );
        }
        if missing_directories.is_empty() && missing_files.is_empty() {
            lines.push("- latest structured plan expected paths are complete".to_string());
            if !plan.expected_files.is_empty() {
                lines.push(format!(
                    "- completed structured plan expected files:\n{}",
                    plan.expected_files
                        .iter()
                        .take(12)
                        .map(|path| format!("  - {}", display_agent_context_path(session, path)))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            lines.push(
                "- completed structured plan files are still editable when the user requests changes; do not refuse edits solely because a path is part of a completed plan; runtime validation and policy decide whether the tool call is allowed".to_string(),
            );
        }
        selected.push(ProviderPromptMemorySelectedFact::new(
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            plan.source_action_id.clone().unwrap_or_default(),
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
            "Use these verified paths only when the current user turn refers to prior work."
                .to_string(),
            "Displayed paths are relative to the current working directory when possible."
                .to_string(),
        ];
        context.extend(lines);
        Some(context.join("\n"))
    };

    AgentVerifiedMemoryContext { prompt_context }
}

fn latest_verified_folder_for_prompt(session: &Session) -> Option<&VerifiedFolderReference> {
    let folders = &session.project_memory().verified_folders;
    let latest = folders.last()?;

    folders
        .iter()
        .rev()
        .skip(1)
        .find(|candidate| {
            latest.path != candidate.path && path_is_within(&latest.path, &candidate.path)
        })
        .or(Some(latest))
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
        verified_state_answer::VerifiedStateAnswerKind,
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
    fn policy_applied_shell_command_records_verified_expected_effect() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-verified",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let expected_directory = root.join("shell-out");
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "mkdir shell-out",
                        "cwd": root.display().to_string(),
                        "expected_directory": expected_directory.display().to_string()
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a directory using shell",
            PermissionPolicyMode::FullAccess,
        );

        assert!(expected_directory.is_dir());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Applied
        );
        let expected_effect = format!(
            "verified directory exists: {}",
            expected_directory.display()
        );
        let Some(VerifiedActionResult::Shell(shell)) =
            session.actions()[0].verified_result.as_ref()
        else {
            panic!("expected verified shell result");
        };
        assert_eq!(shell.exit_code, Some(0));
        assert_eq!(
            shell.verified_effect.as_deref(),
            Some(expected_effect.as_str())
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::Shell(shell)
                        if shell.verified_effect.as_deref() == Some(expected_effect.as_str())
                )
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_applied_shell_command_fails_when_expected_effect_is_missing() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-missing-effect",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let missing_file = root.join("missing.txt");
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf done",
                        "cwd": root.display().to_string(),
                        "expected_file": missing_file.display().to_string()
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a file using shell",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!missing_file.exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].verified_result.is_none());
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("expected files were not created")));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert!(!session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_applied_shell_command_fails_when_exit_is_nonzero() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-nonzero",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "exit 7",
                        "cwd": root.display().to_string()
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
            "run a failing shell command",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            crate::action::ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].verified_result.is_none());
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("shell command exited with status 7")));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert!(!session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_applied_shell_command_resolves_relative_cwd_and_expected_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-relative-paths",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("work")).unwrap();
        let expected_file = root.join("work/out.txt");
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf ok > out.txt",
                        "cwd": "work",
                        "expected_file": "out.txt"
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
            "run a command in work",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(std::fs::read_to_string(&expected_file).unwrap(), "ok");
        let Some(VerifiedActionResult::Shell(shell)) =
            session.actions()[0].verified_result.as_ref()
        else {
            panic!("expected verified shell result");
        };
        assert_eq!(shell.cwd, root.join("work").display().to_string());
        assert_eq!(
            shell.verified_effect.as_deref(),
            Some(format!("verified file exists: {}", expected_file.display()).as_str())
        );

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
                        "contents": "# React TS Tailwind Plan\n\n```text\npackage.json\nsrc/main.tsx\n```\n\n## Verification\n- Check package.json and src/main.tsx exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
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
        assert_eq!(provider.messages.lock().unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_records_reasoning_trace_for_review() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-reasoning-trace",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating only the plan file first.")
                .with_thinking("I need to create a plan before implementation and wait.")
                .with_tool_calls(vec![RawModelToolCall {
                    id: "plan-reasoning-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ReasoningPlan/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                }]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create only a project plan",
            PermissionPolicyMode::FullAccess,
        );

        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should be recorded");
        assert_eq!(trace.route.as_deref(), Some("plan_creation"));
        assert!(trace
            .provider_planning
            .iter()
            .any(|line| line.contains("create a plan before implementation")));
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("requested create_file")
                && line.contains("ReasoningPlan/plan.md")));
        assert!(trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("plan detected") && line.contains("ReasoningPlan/plan.md")));
        assert!(trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Wrote") && line.contains("ReasoningPlan/plan.md")));

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
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-4".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/README.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                    }),
                    assistant_summary: Some("create readme".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-5".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-6".to_string(),
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
        assert!(!root.join("PlanBatch/README.md").exists());
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
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
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
        assert_eq!(provider.messages.lock().unwrap().len(), 1);
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("Skipped implementation tool calls")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_repairs_plan_without_expected_paths_before_implementation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-empty-path-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("PlanRepair")).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating incomplete plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-repair-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanRepair/plan.md",
                        "contents": "# Project Plan\n\nThis is a tiny Python CLI app.\n"
                    }),
                    assistant_summary: Some("create incomplete plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-repair-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "PlanRepair/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("repair plan".to_string()),
                },
                RawModelToolCall {
                    id: "plan-repair-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanRepair/src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main too early".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan ready."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create only the project plan",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("PlanRepair/plan.md")).unwrap(),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
        );
        assert!(!root.join("PlanRepair/src/main.py").exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("repaired plan should be remembered");
        assert!(plan
            .expected_files
            .contains(&root.join("PlanRepair/src/main.py")));
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message.content.contains("no executable expected paths")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_repairs_plan_missing_review_sections_before_implementation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-review-section-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("PlanReviewRepair")).unwrap();
        let repaired_plan = "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n";
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating weak plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-review-repair-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanReviewRepair/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n"
                    }),
                    assistant_summary: Some("create weak plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Repairing review sections.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-review-repair-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "PlanReviewRepair/plan.md",
                        "contents": repaired_plan
                    }),
                    assistant_summary: Some("repair plan sections".to_string()),
                },
                RawModelToolCall {
                    id: "plan-review-repair-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanReviewRepair/src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main too early".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan ready."),
        ]);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create only the project plan",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("PlanReviewRepair/plan.md")).unwrap(),
            repaired_plan
        );
        assert!(!root.join("PlanReviewRepair/src/main.py").exists());
        let contract = session
            .latest_plan_contract()
            .expect("repaired plan should create a contract");
        assert!(contract.review_draft().is_approvable());
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(
                |message| message.content.contains("missing `Verification` section")
                    && message
                        .content
                        .contains("missing `Acceptance Criteria` section")
            ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_creation_reports_needs_revision_when_repair_does_not_converge() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-needs-revision",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("NeedsRevision")).unwrap();
        let mut outputs = vec![crate::event::ProviderOutput::new("Creating weak plan.")
            .with_tool_calls(vec![RawModelToolCall {
                id: "plan-needs-revision-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "NeedsRevision/plan.md",
                    "contents": "# Project Plan\n\n```text\nREADME.md\n```\n"
                }),
                assistant_summary: Some("create weak plan".to_string()),
            }])];
        for _ in 1..MAX_AGENT_TOOL_ROUNDS {
            outputs.push(crate::event::ProviderOutput::new("Plan created."));
        }
        let provider = SequenceProvider::new(outputs);
        let mut session = Session::new("session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create only the project plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("NeedsRevision/plan.md").is_file());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("The plan needs revision before execution")
                    && message.content.contains("missing `Verification` section")
                    && message.content.contains("missing `Acceptance Criteria` section")
        )));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| { line.contains("The plan needs revision before execution") })));

        let _ = std::fs::remove_dir_all(root);
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
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
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
    fn verified_plan_preflight_allows_new_independent_plan_creation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-independent-draft",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("existing")).unwrap();
        std::fs::write(
            root.join("existing/plan.md"),
            "# Existing Plan\n\n```text\nmain.py\n```\n\n## Verification\n- Check main.py exists.\n\n## Acceptance Criteria\n- Existing plan remains available.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating the new plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "new-independent-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "new-plan/plan.md",
                        "contents": "# New Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist.\n\n## Acceptance Criteria\n- New plan can be executed independently.\n"
                    }),
                    assistant_summary: Some("create new independent plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: root.join("existing/plan.md"),
            project_root: root.join("existing"),
            source_action_id: "action-existing-plan".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a separate plan for a different project",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("new-plan/plan.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("outside that project")
        )));
        let latest = session
            .project_memory()
            .latest_structured_plan()
            .expect("new plan should become latest structured plan");
        assert_eq!(latest.project_root, root.join("new-plan"));

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
                    arguments: json!({ "contents": "# Notes\n" }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing the file path.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "missing-create-target-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.md",
                        "contents": "# Notes\n"
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
            "create an md notes file",
            PermissionPolicyMode::FullAccess,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "# Notes\n"
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
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
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
    fn verified_plan_execution_repairs_non_approvable_contract_before_filesystem_changes() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-blocks-bad-contract",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\n```\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "blocked-plan-exec-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan first.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "repair-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "demo/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\n```\n\n## Verification\n- `src/main.py` exists.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n"
                    }),
                    assistant_summary: Some("repair plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan revised."),
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

        assert!(!cwd.join("demo/src/main.py").exists());
        assert!(cwd.join("demo/plan.md").is_file());
        assert!(session
            .latest_plan_contract()
            .is_some_and(|contract| contract.review_draft().is_approvable()));
        assert_eq!(session.actions().len(), 1);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(&applied.result, VerifiedActionResult::File(
                    crate::event::FileActionVerification::FileOverwritten { path }
                ) if path.ends_with("demo/plan.md"))
        )));
        assert_eq!(
            session
                .latest_reasoning_trace()
                .and_then(|trace| trace.route.as_deref()),
            Some("plan_creation")
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Cannot execute the plan yet"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_anchors_unlisted_parent_directory_under_plan_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-anchor-parent-dir",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating parent directory.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "parent-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "src" }),
                    assistant_summary: Some("create src".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
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

        assert!(cwd.join("demo/src").is_dir());
        assert!(cwd.join("demo/src/main.py").is_file());
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert!(!cwd.join("src").exists());
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message.content.contains("Missing expected files")
                && message.content.contains("demo/src/main.py")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_repair_only_updates_same_plan_file() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-repair-same-plan-only",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo-bad")).unwrap();
        std::fs::write(
            cwd.join("demo-bad/PLAN.md"),
            "# Project Plan\n\n```text\ncli.py\nutils.py\n```\n\n## Verification\n- Run `pytest tests/test_cli.py`.\n\n## Acceptance Criteria\n- Running `python -m demo-bad.cli` prints a greeting.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "blocked-exec-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "cli.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "rename-question-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::AskGuidance),
                    arguments: json!({
                        "question": "Should I rename the project folder to demo_bad?"
                    }),
                    assistant_summary: None,
                },
                RawModelToolCall {
                    id: "wrong-folder-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "demo_bad" }),
                    assistant_summary: Some("create replacement folder".to_string()),
                },
                RawModelToolCall {
                    id: "repair-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "demo-bad/PLAN.md",
                        "contents": "# Project Plan\n\n```text\ncli.py\nutils.py\ntests/test_cli.py\n```\n\n## Verification\n- `cli.py`, `utils.py`, and `tests/test_cli.py` exist.\n- Run `pytest tests/test_cli.py`.\n\n## Acceptance Criteria\n- Running `python cli.py --name Alice` prints `Hello, Alice!`.\n- The test suite passes.\n"
                    }),
                    assistant_summary: Some("repair same plan file".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan revised."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("demo-bad/PLAN.md"),
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
                path: "demo-bad/PLAN.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!cwd.join("demo_bad").exists());
        assert!(!cwd.join("demo-bad/cli.py").exists());
        assert!(std::fs::read_to_string(cwd.join("demo-bad/PLAN.md"))
            .unwrap()
            .contains("tests/test_cli.py"));
        assert!(session
            .latest_plan_contract()
            .is_some_and(|contract| contract.review_draft().is_approvable()));
        assert_eq!(session.actions().len(), 1);
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("Should I rename the project folder")
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
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
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
    fn verified_plan_execution_skips_late_directory_that_file_action_already_created() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-late-dir",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
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
                    id: "requirements-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating parent directory late.").with_tool_calls(
                vec![RawModelToolCall {
                    id: "late-src-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "src" }),
                    assistant_summary: Some("create src late".to_string()),
                }],
            ),
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

        assert!(cwd.join("demo/src").is_dir());
        assert!(cwd.join("demo/src/main.py").is_file());
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert_eq!(session.actions().len(), 2);
        assert!(session
            .actions()
            .iter()
            .all(|record| { !matches!(record.action.request, ActionRequest::CreateDirectory(_)) }));
        assert_eq!(
            provider.messages.lock().unwrap().len(),
            1,
            "completed plan execution should not request a late redundant directory round"
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("skipped final provider synthesis"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_skips_destructive_followup_after_expected_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-skip-delete",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
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
                    id: "requirements-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
                RawModelToolCall {
                    id: "delete-main-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::DeleteFile),
                    arguments: json!({
                        "target_path": "src/main.py"
                    }),
                    assistant_summary: Some("delete main".to_string()),
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
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert!(matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        ));
        assert_eq!(session.actions().len(), 2);
        assert!(session
            .actions()
            .iter()
            .all(|record| { !matches!(record.action.request, ActionRequest::DeleteFile(_)) }));
        assert!(!session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should remain recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message
                .content
                .contains("Skipped tool call because it does not create a missing expected path")));
        assert_eq!(
            session
                .latest_reasoning_trace()
                .and_then(|trace| trace.route.as_deref()),
            Some("plan_execution")
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line == "plan execution paths detected")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_repairs_malformed_tool_call_without_raw_error() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-repairs-malformed",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\n```\n\n## Verification\n- `src/main.py` exists.\n\n## Acceptance Criteria\n- Expected file exists under the plan root.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating directory.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "malformed-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!("src"),
                    assistant_summary: Some("create src".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "repaired-file-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
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

        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::Error(error)
                if error.message.contains("tool call is incomplete or malformed")
        )));
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message
                .content
                .contains("send a corrected `create_directory` tool call")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_can_complete_many_expected_paths_one_per_round() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-many-rounds",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n  main.py\ntests/\n  __init__.py\n  test_main.py\nREADME.md\nrequirements.txt\n.gitignore\n```\n\n## Verification\n- All listed files and directories exist.\n\n## Acceptance Criteria\n- The complete expected tree is present.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Create src.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-src".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "src" }),
                    assistant_summary: Some("create src".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create tests.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-tests".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "tests" }),
                    assistant_summary: Some("create tests".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create readme.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-readme".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "README.md",
                        "contents": "# Demo\n"
                    }),
                    assistant_summary: Some("create readme".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create main.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-main".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create test init.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-test-init".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "tests/__init__.py",
                        "contents": ""
                    }),
                    assistant_summary: Some("create test init".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create test file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-test-main".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "tests/test_main.py",
                        "contents": "def test_smoke():\n    assert True\n"
                    }),
                    assistant_summary: Some("create test".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create requirements.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-requirements".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Create gitignore.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "many-gitignore".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": ".gitignore",
                        "contents": "__pycache__/\n"
                    }),
                    assistant_summary: Some("create gitignore".to_string()),
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

        for path in [
            "demo/src/main.py",
            "demo/tests/__init__.py",
            "demo/tests/test_main.py",
            "demo/README.md",
            "demo/requirements.txt",
            "demo/.gitignore",
        ] {
            assert!(cwd.join(path).is_file(), "missing {path}");
        }
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should remain recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_skips_existing_expected_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-skip-existing-files",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo/src")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
        std::fs::write(cwd.join("demo/src/main.py"), "print('existing')\n").unwrap();
        std::fs::write(cwd.join("demo/requirements.txt"), "").unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Recreating expected files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "existing-main-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('new')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
                RawModelToolCall {
                    id: "existing-requirements-1".to_string(),
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

        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
            "print('existing')\n"
        );
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert!(session.actions().is_empty());
        assert!(!session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should remain recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| message
                .content
                .contains("Skipped tool call because it does not create a missing expected path")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_continues_for_missing_expected_directories() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-missing-dirs",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n└─ main.py\ntests/\n```\n\n## Verification\n- `src/main.py` exists and `tests/` exists.\n\n## Acceptance Criteria\n- Missing expected files and directories are created under the plan root.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating first file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "main-file-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Creating missing directory.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "tests-dir-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "tests" }),
                    assistant_summary: Some("create tests".to_string()),
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

        assert_eq!(
            std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
            "print('hello')\n"
        );
        assert!(cwd.join("demo/tests").is_dir());
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should remain recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(
                |message| message.content.contains("Missing expected directories")
                    && message.content.contains("demo/tests")
            ));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_plain_fallback_continues_missing_plan_repair() {
        #[derive(Debug, Clone)]
        struct FallbackDuringPlanProvider {
            tool_outputs: std::sync::Arc<
                std::sync::Mutex<Vec<Result<crate::event::ProviderOutput, ProviderError>>>,
            >,
            fallback_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
            messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<ChatMessage>>>>,
        }

        impl ControllerProvider for FallbackDuringPlanProvider {
            fn request_metadata(&self) -> ProviderRequestMetadata {
                ProviderRequestMetadata::new("fallback-plan", None, "request")
            }

            fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
                Err(ProviderError::configuration("unused"))
            }

            fn chat_messages_with_metadata(
                &self,
                messages: Vec<ChatMessage>,
                _metadata: &ProviderRequestMetadata,
            ) -> Result<crate::event::ProviderOutput, ProviderError> {
                self.messages.lock().unwrap().push(messages);
                Ok(self.fallback_outputs.lock().unwrap().remove(0))
            }

            fn chat_messages_with_tools_with_metadata(
                &self,
                messages: Vec<ChatMessage>,
                _metadata: &ProviderRequestMetadata,
                _tools: Vec<ChatToolDefinition>,
            ) -> Result<crate::event::ProviderOutput, ProviderError> {
                self.messages.lock().unwrap().push(messages);
                self.tool_outputs.lock().unwrap().remove(0)
            }
        }

        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-fallback-repair",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\ntests/\n```\n\n## Verification\n- `src/main.py`, `requirements.txt`, and `tests/` exist.\n\n## Acceptance Criteria\n- Missing expected paths are created under the plan root.\n",
        )
        .unwrap();
        let provider = FallbackDuringPlanProvider {
            tool_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                Ok(crate::event::ProviderOutput::new("Creating first file.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "main-file-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "src/main.py",
                            "contents": "print('hello')\n"
                        }),
                        assistant_summary: Some("create main".to_string()),
                    }],
                )),
                Err(ProviderError::empty_response("provider response contained no text")),
                Ok(crate::event::ProviderOutput::new("Creating remaining paths.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "requirements-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create requirements".to_string()),
                        },
                        RawModelToolCall {
                            id: "tests-dir-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                            arguments: json!({ "target_path": "tests" }),
                            assistant_summary: Some("create tests".to_string()),
                        },
                    ],
                )),
                Ok(crate::event::ProviderOutput::new("Done.")),
            ])),
            fallback_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                crate::event::ProviderOutput::new(
                    "<|channel|>commentary to=filesystem.create code<|message|>{\"path\":\"requirements.txt\",\"contents\":\"\"}",
                ),
            ])),
            messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
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

        assert!(cwd.join("demo/src/main.py").is_file());
        assert!(cwd.join("demo/requirements.txt").is_file());
        assert!(cwd.join("demo/tests").is_dir());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message) if message.content.contains("<|channel|>")
        )));
        assert!(provider
            .messages
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(
                |message| message.content.contains("Missing expected directories")
                    && message.content.contains("Missing expected files")
            ));

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
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
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
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("tui-state-test/PLAN.md"),
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
                path: cwd.join("tui-state-test/PLAN.md").display().to_string(),
            }),
        );

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
        assert!(verified_context
            .content
            .contains("- latest structured plan root: tui-state-test"));
        assert!(verified_context
            .content
            .contains("- missing expected directories:"));
        assert!(verified_context.content.contains("  - tui-state-test/src"));
        assert!(verified_context
            .content
            .contains("- missing expected files:"));
        assert!(verified_context
            .content
            .contains("  - tui-state-test/src/main.py"));
        assert!(verified_context
            .content
            .contains("  - tui-state-test/requirements.txt"));
        assert!(!verified_context
            .content
            .contains("playground/tui-state-test"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_folder_prompt_context_prefers_workspace_ancestor_over_child_folder() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-folder-context-ancestor",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let project = cwd.join("workspace");
        let child = project.join("notes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&child).unwrap();
        let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
            "I found the workspace.",
        )]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: project.clone(),
            source_action_id: "action-project".to_string(),
        });
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: child,
            source_action_id: "action-child".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue in that workspace",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("- latest verified folder: workspace"));
        assert!(!verified_context
            .content
            .contains("- latest verified folder: workspace/notes"));
        let selection = session
            .latest_provider_prompt_memory_selection()
            .expect("selection should be recorded");
        assert_eq!(selection.selected[0].path, project);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn completed_structured_plan_prompt_context_keeps_files_editable() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-completed-plan-editable-context",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let project = cwd.join("workspace");
        let plan_path = project.join("plan.md");
        let readme_path = project.join("README.md");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(&plan_path, "# Plan\n").unwrap();
        std::fs::write(&readme_path, "old\n").unwrap();
        let provider =
            SequenceProvider::new(vec![crate::event::ProviderOutput::new("I can update it.")]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_plan_reference(crate::session::VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project.clone(),
            source_action_id: "action-plan".to_string(),
        });
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Completed,
            expected_directories: Vec::new(),
            expected_files: vec![readme_path],
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "overwrite README.md in that project",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("completed structured plan files are still editable"));
        assert!(verified_context.content.contains("workspace/README.md"));
        assert!(verified_context
            .content
            .contains("runtime validation and policy decide"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_folder_context_anchors_relative_existing_file_actions_under_workspace() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-folder-action-anchor",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let project = cwd.join("workspace");
        let notes = project.join("notes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("archive.txt"), "archive\n").unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Moving archive.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "move-relative-under-workspace".to_string(),
                    name: RawModelToolName::Known(ModelToolName::MoveFile),
                    arguments: json!({
                        "source_path": "notes/archive.txt",
                        "target_path": "notes/archived.txt"
                    }),
                    assistant_summary: Some("move archive".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: project.clone(),
            source_action_id: "action-project".to_string(),
        });
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: notes,
            source_action_id: "action-notes".to_string(),
        });

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "move notes/archive.txt to notes/archived.txt",
            PermissionPolicyMode::FullAccess,
        );

        assert!(!project.join("notes/archive.txt").exists());
        assert!(project.join("notes/archived.txt").is_file());
        assert!(!cwd.join("notes/archived.txt").exists());

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
        tool_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
    }

    impl CapturingProvider {
        fn new() -> Self {
            Self {
                requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                plain_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                    crate::event::ProviderOutput::new(
                        "{\"route\":\"chat\",\"content\":\"Plain answer.\"}",
                    ),
                ])),
                tool_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                    crate::event::ProviderOutput::new("I'll create it."),
                ])),
            }
        }

        fn with_tool_output(mut self, output: crate::event::ProviderOutput) -> Self {
            self.tool_outputs = std::sync::Arc::new(std::sync::Mutex::new(vec![output]));
            self
        }

        fn with_tool_outputs(mut self, outputs: Vec<crate::event::ProviderOutput>) -> Self {
            self.tool_outputs = std::sync::Arc::new(std::sync::Mutex::new(outputs));
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
                outputs.first().cloned().unwrap_or_else(|| {
                    crate::event::ProviderOutput::new(
                        "{\"route\":\"chat\",\"content\":\"Plain answer.\"}",
                    )
                })
            }
        }

        fn next_tool_output(&self, has_tool_result: bool) -> crate::event::ProviderOutput {
            let mut outputs = self.tool_outputs.lock().unwrap();
            if !outputs.is_empty() {
                return outputs.remove(0);
            }
            if has_tool_result {
                crate::event::ProviderOutput::new("Done.")
            } else {
                crate::event::ProviderOutput::new("I'll create it.")
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
            let has_tool_result = messages
                .iter()
                .any(|message| matches!(message.role, ChatRole::Tool));
            self.requests.lock().unwrap().push(CapturedProviderRequest {
                mode: CapturedProviderRequestMode::Tool,
                messages,
                tool_count: tools.len(),
            });
            Ok(self.next_tool_output(has_tool_result))
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
            assert!(joined.len() <= 700, "plain route prompt grew: {joined}");
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
    fn normal_text_model_execute_decision_enters_tool_loop_without_slash_command() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-normal-execute-decision",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "normal-execute-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: json!({ "target_path": "model-selected-folder" }),
                        assistant_summary: Some("create model-selected folder".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "please handle this request");

        assert!(root.join("model-selected-folder").is_dir());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert!(requests[1].tool_count > 0);
        assert!(requests[2]
            .messages
            .iter()
            .any(|message| matches!(message.role, ChatRole::Tool)));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("model-selected-folder")
                )
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unstructured_route_response_retries_json_before_accepting_chat() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-route-json-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    "# Project Plan\n\n```text\nRepairPlan/\nREADME.md\n```\n\n## Verification\n- Check files.\n\n## Acceptance Criteria\n- Files exist.\n",
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "route-repair-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "RepairPlan/PLAN.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check README.md, src/main.py, and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny app",
        );

        assert!(root.join("RepairPlan/PLAN.md").is_file());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].tool_count, 0);
        assert!(
            joined_request_messages(&requests[1])
                .contains("previous no-tool routing response was not valid route JSON")
                || joined_request_messages(&requests[1])
                    .contains("Previous no-tool routing response was not valid route JSON")
        );
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        assert!(requests[2].tool_count > 0);
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("# Project Plan")
                    && message.source == AssistantMessageSource::Provider
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("retrying route JSON")));
        assert_eq!(trace.route.as_deref(), Some("plan_creation"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn artifact_like_chat_route_retries_before_rendering_plan_json() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-artifact-chat-route-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let artifact_json = format!(
            "{{\"project_name\":\"Demo\",\"files\":[{}]}}",
            "\"README.md\",\"src/main.py\",\"requirements.txt\",".repeat(80)
        );
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": artifact_json,
                    })
                    .to_string(),
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "artifact-chat-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ArtifactChatPlan/plan.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny app",
        );

        assert!(root.join("ArtifactChatPlan/plan.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("\"project_name\"")
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("artifact-like chat")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compact_json_plan_chat_routes_to_execute_instead_of_rendering() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-compact-json-artifact-chat",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let compact_plan_json = r#"{
  "project_name": "CompactJsonPlan",
  "structure": {
    "README.md": "Project overview.",
    "src/main.py": "CLI entry point.",
    "requirements.txt": "Runtime dependencies."
  },
  "verification": "Run python src/main.py --help.",
  "acceptance_criteria": ["All listed files exist."]
}"#;
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": compact_plan_json,
                    })
                    .to_string(),
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "compact-json-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CompactJsonPlan/PLAN.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Run python src/main.py --help after execution.\n\n## Acceptance Criteria\n- All listed files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny app",
        );

        assert!(root.join("CompactJsonPlan/PLAN.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("\"project_name\"")
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("artifact-like chat")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn numbered_tree_plan_chat_counts_local_paths_as_artifact_shape() {
        let artifact_markdown = r#"Project Plan: Tiny Python CLI Todo App
1. Folder Structure
   - playground/ManualEfficiencyFollowupCodex3/
     ├── README.md
     ├── src/
     │   └── main.py
     └── requirements.txt`
2. README.md
   - Project title and brief description.
3. src/main.py
   - Entry point for CLI using argparse.
4. requirements.txt
   - No external dependency.
5. Verification & Acceptance Criteria
   - python src/main.py add "Task description" adds a task.
   - python src/main.py list displays all tasks with IDs.
"#;

        assert!(local_path_like_token_count(artifact_markdown) >= 3);
        assert!(looks_like_misrouted_artifact_chat_after_retry(
            artifact_markdown
        ));
    }

    #[test]
    fn artifact_like_chat_after_route_retry_executes_instead_of_rendering() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-artifact-chat-after-route-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let artifact_markdown = format!(
            "{}\n{}",
            "File | Purpose\nREADME.md | docs\nsrc/main.py | CLI\nrequirements.txt | deps\nacceptance_criteria.md | checks\n",
            "Describe setup, usage, verification, and acceptance details.\n".repeat(8)
        );
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("Project plan artifact"),
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": artifact_markdown,
                    })
                    .to_string(),
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "artifact-chat-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ArtifactChatRetryPlan/plan.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny app",
        );

        assert!(root.join("ArtifactChatRetryPlan/plan.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("acceptance_criteria.md")
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert_eq!(trace.route.as_deref(), Some("plan_creation"));
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("compact artifact-like chat after retry")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn newly_created_plan_is_not_executed_in_same_turn_even_with_execution_intent() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-execution-intent",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan and files.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-exec-intent-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/plan.md",
                                "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-exec-intent-readme-too-early".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/README.md",
                                "contents": "# Calculator UI\n"
                            }),
                            assistant_summary: Some("create README too early".to_string()),
                        },
                    ],
                ),
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-exec-intent-readme".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/README.md",
                            "contents": "# Calculator UI\n"
                        }),
                        assistant_summary: Some("create README".to_string()),
                    },
                    RawModelToolCall {
                        id: "plan-exec-intent-calc".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/calculator.py",
                            "contents": "class Calculator:\n    pass\n"
                        }),
                        assistant_summary: Some("create calculator".to_string()),
                    },
                    RawModelToolCall {
                        id: "plan-exec-intent-ui".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/ui.py",
                            "contents": "from calculator import Calculator\n"
                        }),
                        assistant_summary: Some("create ui".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "execute the plan you just created");

        assert!(root.join("CalculatorUI/plan.md").is_file());
        assert!(!root.join("CalculatorUI/README.md").exists());
        assert!(!root.join("CalculatorUI/calculator.py").exists());
        assert!(!root.join("CalculatorUI/ui.py").exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded");
        assert_eq!(
            plan.status,
            crate::session::StructuredProjectPlanStatus::Verified
        );
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("execute intent plan_execution")));
        assert!(
            trace
                .runtime_checks
                .iter()
                .any(|line| line
                    .contains("plan creation completed; skipped final provider synthesis"))
        );
        assert!(trace.runtime_checks.iter().any(|line| line
            .contains("Skipped extra implementation tool calls in this plan-creation turn")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn followup_route_can_bind_latest_verified_plan_with_model_requested_context() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-followup-context",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("CalculatorUI");
        std::fs::create_dir_all(&project).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    "{\"route\":\"ask_guidance\",\"question\":\"Which plan should I execute?\"}",
                ),
                crate::event::ProviderOutput::new(
                    "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "followup-readme".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "README.md",
                            "contents": "# Calculator UI\n"
                        }),
                        assistant_summary: Some("create README".to_string()),
                    },
                    RawModelToolCall {
                        id: "followup-calc".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "calculator.py",
                            "contents": "class Calculator:\n    pass\n"
                        }),
                        assistant_summary: Some("create calculator".to_string()),
                    },
                    RawModelToolCall {
                        id: "followup-ui".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ui.py",
                            "contents": "from calculator import Calculator\n"
                        }),
                        assistant_summary: Some("create ui".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project.clone(),
            source_action_id: "action-plan".to_string(),
        });
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Verified,
            expected_directories: Vec::new(),
            expected_files: vec![
                project.join("README.md"),
                project.join("calculator.py"),
                project.join("ui.py"),
            ],
        });

        run_permissive_agent_turn(&provider, &mut session, "the plan you just created");

        assert!(project.join("README.md").is_file());
        assert!(project.join("calculator.py").is_file());
        assert!(project.join("ui.py").is_file());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert!(!joined_request_messages(&requests[0]).contains("latest verified plan"));
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        let context_retry_request = joined_request_messages(&requests[1]);
        assert!(context_retry_request.contains("latest verified plan: CalculatorUI/plan.md"));
        assert!(context_retry_request.contains("create all missing expected paths"));
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        let first_tool_request = joined_request_messages(&requests[2]);
        assert!(first_tool_request.contains("Verified plan execution contract"));
        for path in [
            "CalculatorUI/README.md",
            "CalculatorUI/calculator.py",
            "CalculatorUI/ui.py",
        ] {
            assert!(
                first_tool_request.contains(path),
                "first tool request did not include missing path {path}"
            );
        }
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded");
        assert_eq!(
            plan.runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace.runtime_checks.iter().any(|line| line
            .contains("seeded verified plan execution contract before first tool request")));
        assert!(trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("skipped final provider synthesis")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normal_text_model_plain_answer_renders_without_tools() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-normal-chat-decision",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"Hello there.\"}"),
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert!(requests[0].messages[0]
            .content
            .contains("{\"route\":\"execute\"}"));
        assert!(requests[0].messages[0]
            .content
            .contains("persistent local change"));
        assert!(requests[0].messages[0]
            .content
            .contains("Return one compact JSON object"));
        assert!(requests[0].messages[0].content.len() <= 700);
        assert!(!requests[0].messages[0].content.contains("Use `/tool"));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Hello there."
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message) if message.content.contains("\"route\"")
        )));
        assert!(session.actions().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wrapped_chat_route_does_not_trigger_tool_protocol_fallback() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-wrapped-chat-route",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "<|channel|>final<|message|>{\"route\":\"chat\",\"content\":\"Hello.\"}",
            ));
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Hello."
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("tool_enabled")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_hello_after_verified_folder_stays_one_plain_request() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-hello-no-folder-memory",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("remembered")).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"Hello.\"}"),
        );
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
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(
            requests[0].messages.last(),
            Some(&ChatMessage::user("hello"))
        );
        let joined = joined_request_messages(&requests[0]);
        assert!(!joined.contains("latest verified folder"));
        assert!(!joined.contains("remembered"));
        assert!(!joined.contains("Verified filesystem context"));
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
    fn permissive_agent_state_question_uses_model_selected_state_route() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-question-model",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("remembered")).unwrap();
        std::fs::create_dir_all(root.join("latest-folder")).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
            ));
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
        let joined = joined_request_messages(&requests[0]);
        assert!(joined.contains("{\"route\":\"state\""));
        assert!(!joined.contains("latest verified folder"));
        assert!(!joined.contains("remembered"));
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
    fn plan_creation_request_after_verified_folder_is_not_state_answer() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-request-after-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("planned")).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-after-folder-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "planned/plan.md",
                            "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);
        let folder_action = Action::proposed(
            "action-folder",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("planned"),
            }),
            "create planned folder",
        )
        .approve()
        .mark_applied();
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "planned".to_string(),
            });
        let mut record = ActionRecord::new(folder_action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &folder_action, &result);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a markdown project plan inside planned for a tiny Python CLI app",
        );

        assert!(root.join("planned/plan.md").is_file());
        assert!(session.project_memory().latest_structured_plan().is_some());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message) if message.content == "No verified plan recorded."
        )));
        let requests = provider.requests();
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert!(requests[1].tool_count > 0);

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
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"created_summary\"}",
            ));
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
    fn completed_plan_execution_intent_skips_tool_loop() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-completed-plan-execution-short-circuit",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("DonePlan");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Done Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected paths are present.\n",
        )
        .unwrap();
        std::fs::write(project.join("README.md"), "# Done\n").unwrap();
        std::fs::write(project.join("src/main.py"), "def main():\n    pass\n").unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ));
        let mut session = Session::new("session", &root, &root);
        session.record_verified_plan_reference(VerifiedPlanReference {
            project_root: project.clone(),
            path: plan_path.clone(),
            source_action_id: "action-plan".to_string(),
        });
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Verified,
            expected_directories: vec![root.join("DonePlan/src")],
            expected_files: vec![
                root.join("DonePlan/README.md"),
                root.join("DonePlan/src/main.py"),
            ],
        });

        run_permissive_agent_turn(&provider, &mut session, "execute the latest plan");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("already complete")
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("already complete; skipped tool loop")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn permissive_agent_raw_tool_protocol_decision_enters_tool_loop_without_surfacing_protocol() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plain-raw-tool-protocol",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "<|channel|>commentary to=filesystem.create code<|message|>{\"path\":\"testharness\",\"contents\":\"\"}\nCreated folder testharness.",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "raw-protocol-retry-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: json!({ "target_path": "testharness" }),
                        assistant_summary: Some("create testharness folder".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a folder and name it testharness",
        );

        assert!(root.join("testharness").is_dir());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert!(requests[1].tool_count > 0);
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
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"Ok.\"}",
            ))
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
        assert_eq!(requests.len(), 1);
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
        assert!(system_prompt.contains("do not make completed files immutable"));
        assert!(!system_prompt.contains("next-env.d.ts"));
        assert!(!system_prompt.contains("tailwind.config"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
