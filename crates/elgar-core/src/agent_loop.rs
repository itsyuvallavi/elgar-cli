use std::{
    collections::HashSet,
    env, fs,
    path::{Component, Path, PathBuf},
};

use serde_json::{json, Value};

use crate::{
    action::{
        Action, ActionLifecycleState, ActionRequest, CreateFileAction, OverwriteFileAction,
        ShellCommandAction,
    },
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
        elgar_model_tool_definitions, elgar_model_tool_definitions_for,
        validate_model_tool_outputs, ModelToolName, ModelToolValidationErrorKind, RawModelToolCall,
        ValidatedModelGuidanceRequest, ValidatedModelToolAction, ValidatedModelToolOutput,
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
        ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction, ChatToolDefinition,
        ControllerProvider, ProviderErrorKind,
    },
    provider_visible_text_from_text_only_output,
    router::Route,
    session::{
        ActionRecord, PendingActionSelection, ProviderPromptMemorySelectedFact,
        ProviderPromptMemorySelection, Session, StructuredProjectPlanStatus,
        VerifiedFolderReference, VerifiedPlanReference,
    },
    session_log_memory::{latest_durable_verified_artifacts, DurableVerifiedArtifactFact},
    shell::ShellExecutor,
    shell_allowlist::is_read_only_shell_command,
    verified_artifact_memory::{
        earliest_verified_artifacts, latest_action_turn_artifacts, latest_verified_artifacts,
        verified_artifacts_under_folder, CappedVerifiedArtifacts, VerifiedArtifactFact,
    },
    verified_state_answer::{
        parse_verified_state_answer_kind, resolve_state_answer_kind,
        resolved_state_answer_trace_metadata, verified_session_state_answer,
        VerifiedStateAnswerKind,
    },
};

const AGENT_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar, a permissive terminal-native coding agent. ",
    "Use tools to do the user's requested filesystem and shell work directly. ",
    "Use shell_command for command execution or local file inspection; do not satisfy those requests by rewriting files. ",
    "Do not ask for approval. Do not give instructions instead of acting when a tool can do it. ",
    "Ask one concise clarification question only when the target or intent is truly ambiguous. ",
    "If the user asks you to choose, choose a reasonable option and continue the prior request. ",
    "If the user asks for a plan and says to share it before implementation, create or update a plan file and summarize it; do not implement project files until asked. ",
    "If the user asks to create only a plan file with a future file tree, create only that plan file; do not ask whether to create the listed future files. ",
    "If the user requests planning and implementation in the same turn, create the plan file first, then implement the planned files. ",
    "Plan files must include a concrete file tree, a Verification section, and an Acceptance Criteria section before implementation. ",
    "Verified plans guide runtime validation but do not make completed files immutable; if the user requests an edit under a verified plan root, use the appropriate file tool and let runtime validation, policy, and executors decide. ",
    "If the user asks what the plan is, summarize the existing plan; do not implement it. ",
    "If a verified plan already exists and the user gives a short choice follow-up, answer from that plan instead of recreating the same file. ",
    "When creating a framework project, infer the necessary starter files from the requested stack and create the complete runnable scaffold before the final answer. ",
    "After tools run, answer naturally and briefly with what happened."
);
const AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT: &str = concat!(
    "You are Elgar. Classify only; Return compact JSON, no prose. ",
    "{\"route\":\"execute\",\"intent\":\"shell_execution\"}=run/inspect shell. ",
    "{\"route\":\"execute\"}=local file/artifact/plan work or review current/root/this folder/project. ",
    "{\"route\":\"chat\",\"content\":\"...\"}=text only, not local/runtime state. ",
    "{\"route\":\"execute\",\"intent\":\"plan_execution\"}=execute verified plan. ",
    "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}=same prompt creates plan then executes/implements it. ",
    "Plan-only: execute, no intent. ",
    "{\"route\":\"state\",\"answer_kind\":\"...\"}=verified status/plan/created files questions. ",
    "{\"route\":\"ask_guidance\",\"question\":\"...\"}=missing required detail."
);
const AGENT_STATE_KIND_CLASSIFIER_PROMPT: &str = concat!(
    "The user asked about verified runtime state. ",
    "Return exactly one compact JSON object {\"answer_kind\":\"...\"}; no prose. ",
    "Valid answer kinds: latest_folder, latest_file, project_files, first_created, created_summary, recent_changes, last_block, plan, plan_details, plan_status, pending, status, memory, summary. ",
    "plan_details=plan expected dirs/files and contents; plan=latest plan status and expected paths; project_files=files under latest/referenced project; first_created=earliest verified artifact; ",
    "recent_changes=what was just done in the most recent action; last_block=why the latest runtime action was blocked/skipped/failed; created_summary=everything created so far; ",
    "latest_folder/latest_file=the most recent created folder/file; ",
    "pending=actions awaiting approval; status=applied counts and latest paths; memory=remembered folders/plans; summary=a short combined overview."
);
const AGENT_ROUTE_JSON_REPAIR_PROMPT: &str = concat!(
    "The previous no-tool routing response was not valid route JSON. ",
    "Return exactly one compact JSON object for the original user request using the routing schema. ",
    "Do not answer in prose and do not draft artifacts."
);
const AGENT_ROUTE_LOCAL_WORK_CHAT_REPAIR_PROMPT: &str = concat!(
    "The previous routing response chose chat for a request containing local filesystem or shell syntax. ",
    "Return exactly one compact JSON object for the original user request using the routing schema. ",
    "Choose execute when tools are needed to create, edit, inspect, or run local artifacts. ",
    "Choose chat only when the user is asking for text-only explanation. ",
    "Do not claim local work was completed in prose."
);
const AGENT_ROUTE_RUNTIME_BLOCK_CHAT_REPAIR_PROMPT: &str = concat!(
    "A recent runtime block/skip/failure is available as verified state. ",
    "Return exactly one compact JSON object for the original user request using the routing schema. ",
    "Choose state with answer_kind last_block when the user is asking about the prior runtime outcome or reason. ",
    "Choose chat only for text that is unrelated to verified runtime state."
);
const AGENT_ROUTE_STATE_WITH_PLAN_REPAIR_PROMPT: &str = concat!(
    "The previous route chose state while an incomplete verified plan is available. ",
    "Return route JSON for the original user request. ",
    "If the request commands applying the current verified plan, choose {\"route\":\"execute\",\"intent\":\"plan_execution\"}. ",
    "If it only asks about what happened or plan status, choose state with answer_kind plan_status/status/plan. ",
    "No prose."
);
const AGENT_POST_PLAN_CREATION_DECISION_PROMPT: &str = concat!(
    "A verified plan was just created. Reclassify the original request only. ",
    "If it requires implementing/executing the plan now, return {\"route\":\"execute\",\"intent\":\"plan_execution\"}. ",
    "If it was plan-only or asks to review/share first, return {\"route\":\"state\",\"answer_kind\":\"plan\"}. ",
    "No prose."
);

const MAX_AGENT_TOOL_ROUNDS: usize = 16;
const REPEATED_IDENTICAL_SKIP_BREAKER_LIMIT: usize = 2;
const TOOL_COMMAND_PREFIX: &str = "/tool";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlainAgentChatOutcome {
    Finished,
    Execute(AgentExecutionIntent),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AgentExecutionIntent {
    plan_execution: bool,
    plan_creation_execution: bool,
    shell_execution: bool,
    after_plan_creation_decision: bool,
    explicit_tool_command: bool,
}

impl AgentExecutionIntent {
    fn is_plan_work(self) -> bool {
        self.plan_execution || self.plan_creation_execution
    }
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
        return finish_agent_turn_result(session, start_index, Route::AskModel);
    };

    if tool_input.is_empty() {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "Usage: /tool <request>",
            AssistantMessageSource::Controller,
        )));
        return finish_agent_turn_result(session, start_index, Route::AskModel);
    }

    session.record_reasoning_route("execute");
    session.push_reasoning_model_decision("explicit /tool route selected");
    run_agent_tool_chat(
        provider,
        session,
        tool_input,
        policy_mode,
        start_index,
        AgentExecutionIntent {
            explicit_tool_command: true,
            ..AgentExecutionIntent::default()
        },
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

fn finish_agent_turn_result(session: &mut Session, start_index: usize, route: Route) -> TurnResult {
    session.finish_trace_turn();
    TurnResult {
        route,
        events: session.events()[start_index..].to_vec(),
    }
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
    let agent_context = agent_verified_memory_context(session, !intent.is_plan_work());
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
        && !input_contains_local_work_syntax(input)
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
        return finish_agent_turn_result(session, start_index, Route::AskModel);
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
    let tools = tool_definitions_for_intent(intent);
    let prompt_project_root = explicit_project_root_from_input(session, input);
    let mut handled_tool_call_ids = HashSet::new();
    let mut plan_created_this_turn = false;
    let mut plan_execution_in_progress = false;
    let mut plan_execution_requested_after_plan_creation = false;
    let mut plan_creation_repair_in_progress = false;
    let mut allow_implementation_after_plan_creation = intent.plan_creation_execution;
    let mut visible_skipped_tool_notice_shown = false;
    let mut no_tool_response_repair_attempted = false;
    let mut verified_actions_this_tool_turn = 0usize;
    let mut tool_call_rounds_this_tool_turn = 0usize;
    let mut previous_all_skipped_signature: Option<AllSkippedToolResultSignature> = None;
    let mut repeated_all_skipped_count = 0usize;
    let mut validation_repair_in_progress = false;
    let allow_no_tool_response_repair = !intent.explicit_tool_command;

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
                    if plan_execution_in_progress {
                        if let Some(message) =
                            plan_execution_repair_message_or_mark_complete(session)
                        {
                            session.push_reasoning_runtime_check(
                                "empty tool response during plan execution; continued from verified missing paths",
                            );
                            messages.push(ChatMessage::system(message));
                            continue;
                        }
                        session.push_reasoning_runtime_check(
                            "plan execution completed after empty tool response; skipped plain fallback",
                        );
                        break;
                    }

                    if plan_execution_requested_after_plan_creation {
                        if let Some(message) = missing_expected_plan_paths_message(session) {
                            session.push_reasoning_runtime_check(
                                "empty tool response during plan execution; continued from verified missing paths",
                            );
                            messages.push(ChatMessage::system(message));
                            continue;
                        }
                        session.push_reasoning_runtime_check(
                            "plan execution completed after empty tool response; skipped plain fallback",
                        );
                        break;
                    }

                    if allow_no_tool_response_repair
                        && verified_actions_this_tool_turn == 0
                        && tool_call_rounds_this_tool_turn == 0
                        && !no_tool_response_repair_attempted
                    {
                        no_tool_response_repair_attempted = true;
                        session.push_reasoning_runtime_check(
                            "empty tool response on execute route; requested tool repair",
                        );
                        messages.push(ChatMessage::system(no_tool_action_repair_message()));
                        continue;
                    }

                    session.push_reasoning_runtime_check(
                        "empty tool response on execute route; skipped plain fallback",
                    );
                    session.push_event(Event::AssistantMessage(AssistantMessage::new(
                        "The model did not return any tool actions, so no files or commands were changed.",
                        AssistantMessageSource::Controller,
                    )));
                    break;
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
            if allow_no_tool_response_repair
                && verified_actions_this_tool_turn == 0
                && tool_call_rounds_this_tool_turn == 0
            {
                if !no_tool_response_repair_attempted {
                    no_tool_response_repair_attempted = true;
                    session.push_reasoning_runtime_check(
                        "execute route returned no tool calls; requested tool repair",
                    );
                    messages.push(ChatMessage::system(no_tool_action_repair_message()));
                    continue;
                }
                session.push_reasoning_runtime_check(
                    "execute route returned no tool calls after repair; stopped",
                );
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    "The model did not return any tool actions, so no files or commands were changed.",
                    AssistantMessageSource::Controller,
                )));
                break;
            }
            push_provider_message_after_tool_turn_if_visible(
                session,
                start_index,
                assistant_text,
                validation_repair_in_progress,
            );
            break;
        }

        tool_call_rounds_this_tool_turn += 1;

        tool_calls.retain(|tool_call| handled_tool_call_ids.insert(tool_call.id.clone()));
        if tool_calls.is_empty() {
            push_provider_message_after_tool_turn_if_visible(
                session,
                start_index,
                assistant_text,
                false,
            );
            break;
        }

        messages.push(chat_assistant_tool_call_message(
            assistant_text,
            &tool_calls,
        ));

        if let Err(message) = validate_tool_calls_in_scope(&tool_calls, &tools) {
            validation_repair_in_progress = true;
            session.trace_event(
                "tool_call_repaired",
                json!({
                    "repair_kind": "tool_scope",
                    "tool_count": tool_calls.len(),
                    "tool_names": tool_calls.iter().map(|tool_call| tool_call.name.raw_label()).collect::<Vec<_>>(),
                    "message_chars": message.chars().count(),
                }),
            );
            for tool_call in tool_calls {
                handled_tool_call_ids.remove(&tool_call.id);
                messages.push(ChatMessage::tool(tool_call.id, message.clone()));
            }
            session.push_reasoning_runtime_check(format!("tool scope repair: {message}"));
            continue;
        }

        let outputs = match validate_model_tool_outputs(&tool_calls) {
            Ok(outputs) => outputs,
            Err(error) => match tool_validation_recovery(&error) {
                ToolValidationRecovery::RepairModel(message) => {
                    validation_repair_in_progress = true;
                    session.trace_event(
                        "tool_call_repaired",
                        json!({
                            "repair_kind": "tool_validation",
                            "error_kind": format!("{:?}", error.kind),
                            "tool_name": &error.tool_name,
                            "argument": &error.argument,
                            "message_chars": message.chars().count(),
                        }),
                    );
                    for tool_call in tool_calls {
                        handled_tool_call_ids.remove(&tool_call.id);
                        messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                    }
                    continue;
                }
                ToolValidationRecovery::Error(message) => {
                    session.trace_event(
                        "tool_call_repaired",
                        json!({
                            "repair_kind": "tool_validation_error",
                            "error_kind": format!("{:?}", error.kind),
                            "tool_name": &error.tool_name,
                            "argument": &error.argument,
                            "message_chars": message.chars().count(),
                        }),
                    );
                    session.push_event(Event::Error(ErrorEvent::new(message.clone())));
                    for tool_call in tool_calls {
                        messages.push(ChatMessage::tool(tool_call.id, message.clone()));
                    }
                    continue;
                }
            },
        };
        validation_repair_in_progress = false;
        session.trace_event(
            "tool_call_validated",
            json!({
                "tool_count": tool_calls.len(),
                "tool_names": tool_calls.iter().map(|tool_call| tool_call.name.raw_label()).collect::<Vec<_>>(),
                "validated_output_count": outputs.len(),
            }),
        );
        record_validated_tool_output_trace(session, &outputs);

        let path_resolution = AgentPathResolution::new(None, None, &session.project_root);
        let resolved_outputs = anchor_verified_folder_tool_outputs(
            session,
            resolve_agent_tool_outputs(outputs, &path_resolution),
        );
        let resolved_outputs = anchor_verified_plan_tool_outputs(session, resolved_outputs);
        let resolved_outputs = anchor_prompt_project_root_tool_outputs(
            session,
            resolved_outputs,
            prompt_project_root.as_deref(),
        );
        let resolved_outputs =
            guard_shell_execution_tool_outputs(session, resolved_outputs, intent.shell_execution);
        let resolved_outputs =
            anchor_bare_plan_artifacts_to_batch_project_root(session, resolved_outputs);
        let resolved_outputs = guard_plan_creation_tool_outputs(
            session,
            resolved_outputs,
            plan_created_this_turn,
            plan_creation_repair_in_progress,
            allow_implementation_after_plan_creation,
        );
        let resolved_outputs = guard_redundant_directory_tool_outputs(session, resolved_outputs);
        let resolved_outputs = prioritize_plan_creation_tool_outputs(session, resolved_outputs);
        let plan_execution_batch =
            resolved_outputs_touch_structured_plan(session, &resolved_outputs);
        let plan_execution_batch_completes_missing_paths = plan_execution_batch
            && resolved_outputs_complete_missing_plan_paths(session, &resolved_outputs);
        if plan_execution_batch {
            plan_execution_requested_after_plan_creation = false;
            session.record_reasoning_route("plan_execution");
            session.push_reasoning_runtime_check("plan execution paths detected");
            if latest_plan_contract_needs_repair(session) {
                let message = plan_execution_blocked_by_contract_repair_message(session);
                session.push_reasoning_runtime_check(message.clone());
                plan_creation_repair_in_progress = true;
                for tool_call_id in resolved_outputs_tool_call_ids(&resolved_outputs) {
                    append_tool_feedback_message(&mut messages, tool_call_id, message.clone());
                }
                messages.push(ChatMessage::system(plan_creation_repair_message(session)));
                continue;
            }
        }
        let starts_plan_execution = plan_execution_batch && !plan_execution_in_progress;
        plan_execution_in_progress |= plan_execution_batch;
        if should_preflight_verified_plan_tool_outputs(
            session,
            &resolved_outputs,
            plan_execution_batch,
            intent.plan_execution,
        ) {
            if let Err(message) = preflight_verified_plan_tool_outputs(session, &resolved_outputs) {
                session.push_reasoning_runtime_check(format!("preflight blocked: {message}"));
                session.record_runtime_block(message.clone());
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    message,
                    AssistantMessageSource::Controller,
                )));
                break;
            }
        }
        let resolved_outputs = guard_plan_execution_tool_outputs(
            session,
            resolved_outputs,
            plan_execution_in_progress,
            plan_execution_batch_completes_missing_paths,
        );
        let plan_execution_round_used_create_files = plan_execution_in_progress
            && tool_calls
                .iter()
                .any(|tool_call| tool_call.name.raw_label() == "create_files");
        let plan_execution_action_outputs_this_round = if plan_execution_in_progress {
            resolved_outputs
                .iter()
                .filter(|output| matches!(output, ResolvedAgentToolOutput::Action(_)))
                .count()
        } else {
            0
        };
        if let Some(signature) = all_skipped_tool_result_signature(&resolved_outputs) {
            if previous_all_skipped_signature.as_ref() == Some(&signature) {
                repeated_all_skipped_count += 1;
            } else {
                previous_all_skipped_signature = Some(signature.clone());
                repeated_all_skipped_count = 1;
            }
            if repeated_all_skipped_count >= REPEATED_IDENTICAL_SKIP_BREAKER_LIMIT {
                let message = repeated_identical_skip_breaker_message(&signature);
                session.push_reasoning_runtime_check(format!(
                    "repeated identical all-skipped tool result; stopped after {repeated_all_skipped_count} iterations"
                ));
                session.record_runtime_block(message.clone());
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    message,
                    AssistantMessageSource::Controller,
                )));
                break;
            }
        } else {
            previous_all_skipped_signature = None;
            repeated_all_skipped_count = 0;
        }

        if starts_plan_execution {
            session.push_reasoning_runtime_check("latest structured plan marked executing");
            session.mark_latest_structured_project_plan_executing();
        }
        let plan_missing_before_tool_results = if plan_execution_in_progress {
            Some(missing_expected_plan_path_counts(session))
        } else {
            None
        };

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
                return finish_agent_turn_result(session, start_index, Route::AskModel);
            }
        }

        let verified_actions_before_outputs = verified_actions_this_tool_turn;
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
                    append_tool_feedback_message(
                        &mut messages,
                        guidance.tool_call_id,
                        guidance.question,
                    );
                }
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id,
                    message,
                    visible,
                } => {
                    tool_results_need_provider_followup = true;
                    session.push_reasoning_runtime_check(format!("skipped: {message}"));
                    if visible && !visible_skipped_tool_notice_shown {
                        session.record_runtime_block(message.clone());
                        session.push_event(Event::AssistantMessage(AssistantMessage::new(
                            message.clone(),
                            AssistantMessageSource::Controller,
                        )));
                        visible_skipped_tool_notice_shown = true;
                    }
                    append_tool_feedback_message(&mut messages, tool_call_id, message);
                }
                ResolvedAgentToolOutput::Action(action) => {
                    if plan_creation_repair_in_progress
                        && !is_latest_verified_plan_file_action(session, &action.request)
                    {
                        tool_results_need_provider_followup = true;
                        let message = plan_creation_non_plan_repair_skip_message();
                        session.push_reasoning_runtime_check(format!("skipped: {message}"));
                        append_tool_feedback_message(&mut messages, action.tool_call_id, message);
                        continue;
                    }
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
                    if session
                        .actions()
                        .last()
                        .is_some_and(|record| record.action.state == ActionLifecycleState::Failed)
                    {
                        session.record_runtime_block(result.clone());
                    }
                    if session
                        .actions()
                        .last()
                        .and_then(|record| record.verified_result.as_ref())
                        .is_some()
                    {
                        verified_actions_this_tool_turn += 1;
                    }
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
                        if intent.plan_creation_execution && !plan_creation_repair_in_progress {
                            plan_execution_requested_after_plan_creation = true;
                            allow_implementation_after_plan_creation = true;
                            session.push_reasoning_runtime_check(
                                "new verified plan created during explicit plan creation execution turn; execution waits for implementation tool calls",
                            );
                        }
                        if plan_creation_repair_in_progress {
                            messages
                                .push(ChatMessage::system(plan_creation_repair_message(session)));
                        }
                    }
                    append_tool_feedback_message(&mut messages, action.tool_call_id, result);
                    if !matches!(
                        session.pending_action_selection(),
                        PendingActionSelection::None
                    ) {
                        return finish_agent_turn_result(session, start_index, Route::AskModel);
                    }
                }
            }
        }
        let verified_actions_this_round =
            verified_actions_this_tool_turn.saturating_sub(verified_actions_before_outputs);

        if plan_execution_in_progress {
            if let Some(before) = plan_missing_before_tool_results {
                let after = missing_expected_plan_path_counts(session);
                if before == after && before.total() > 0 && tool_results_need_provider_followup {
                    let message =
                        plan_execution_no_progress_message(session).unwrap_or_else(|| {
                            "Plan execution made no progress; stopped provider loop.".to_string()
                        });
                    session.push_reasoning_runtime_check(
                        "plan execution made no progress; stopped provider loop",
                    );
                    session.push_event(Event::AssistantMessage(AssistantMessage::new(
                        message,
                        AssistantMessageSource::Controller,
                    )));
                    break;
                }
            }
            if let Some(message) = plan_execution_repair_message_or_mark_complete(session) {
                if verified_actions_this_round > 0
                    && (plan_execution_round_used_create_files
                        || plan_execution_action_outputs_this_round > 1)
                {
                    let message = plan_execution_incomplete_after_partial_batch_message(message);
                    session.push_reasoning_runtime_check(
                        "plan execution made partial batch progress; stopped provider loop",
                    );
                    session.record_runtime_block(message.clone());
                    session.push_event(Event::AssistantMessage(AssistantMessage::new(
                        message,
                        AssistantMessageSource::Controller,
                    )));
                    break;
                }
                messages.push(ChatMessage::system(message));
                continue;
            }
            if !tool_results_need_provider_followup {
                session.push_reasoning_runtime_check(
                    "plan execution completed; skipped final provider synthesis",
                );
                break;
            }
            session.push_reasoning_runtime_check(
                "plan execution completed after skipped tool feedback; skipped final provider synthesis",
            );
            break;
        }
        if plan_created_this_turn
            && !plan_creation_repair_in_progress
            && !plan_execution_in_progress
        {
            if intent.plan_creation_execution && allow_implementation_after_plan_creation {
                if let Some(message) = missing_expected_plan_paths_message(session) {
                    plan_execution_requested_after_plan_creation = true;
                    messages.push(ChatMessage::system(message));
                    continue;
                }
            }
            if intent.after_plan_creation_decision
                && !visible_skipped_tool_notice_shown
                && post_plan_creation_decision_requests_execution(provider, session, input)
            {
                if let Some(message) = missing_expected_plan_paths_message(session) {
                    plan_execution_requested_after_plan_creation = true;
                    allow_implementation_after_plan_creation = true;
                    messages.push(ChatMessage::system(message));
                    continue;
                }
            }
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

    finish_agent_turn_result(session, start_index, Route::AskModel)
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

fn append_tool_feedback_message(
    messages: &mut Vec<ChatMessage>,
    tool_call_id: String,
    content: String,
) {
    if let Some(existing) = messages
        .iter_mut()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id.as_str()))
    {
        if !existing.content.is_empty() {
            existing.content.push('\n');
        }
        existing.content.push_str(&content);
        return;
    }

    messages.push(ChatMessage::tool(tool_call_id, content));
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

fn tool_definitions_for_intent(intent: AgentExecutionIntent) -> Vec<ChatToolDefinition> {
    if intent.explicit_tool_command {
        return elgar_model_tool_definitions();
    }
    if intent.shell_execution {
        return elgar_model_tool_definitions_for(&[
            ModelToolName::AskGuidance,
            ModelToolName::ShellCommand,
        ]);
    }
    if intent.plan_execution || intent.plan_creation_execution {
        return elgar_model_tool_definitions_for(&[
            ModelToolName::AskGuidance,
            ModelToolName::CreateFiles,
            ModelToolName::CreateFile,
            ModelToolName::CreateDirectory,
            ModelToolName::OverwriteFile,
            ModelToolName::PatchFile,
            ModelToolName::ShellCommand,
        ]);
    }
    elgar_model_tool_definitions()
}

fn validate_tool_calls_in_scope(
    tool_calls: &[RawModelToolCall],
    tools: &[ChatToolDefinition],
) -> Result<(), String> {
    let allowed_names = tools
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect::<Vec<_>>();
    for tool_call in tool_calls {
        let tool_name = tool_call.name.raw_label();
        if !allowed_names.contains(&tool_name.as_str()) {
            let allowed = allowed_names.join(", ");
            return Err(format!(
                "Tool `{tool_name}` is not available for this route. Use one of: {allowed}."
            ));
        }
    }
    Ok(())
}

fn retry_plain_agent_chat_with_verified_context<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    let Some(context) = agent_verified_memory_context(session, true).prompt_context else {
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
    retry_plain_agent_chat_for_route_json_with_repair(
        provider,
        session,
        input,
        AGENT_ROUTE_JSON_REPAIR_PROMPT,
        false,
    )
}

fn retry_plain_agent_chat_for_route_json_with_repair<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    repair_prompt: &str,
    include_verified_context: bool,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_route_retry", 0),
    ));
    let mut messages = vec![ChatMessage::system(
        AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT,
    )];
    if include_verified_context {
        if let Some(context) = agent_verified_memory_context(session, true).prompt_context {
            messages.push(ChatMessage::system(context));
        }
    }
    messages.extend([ChatMessage::system(repair_prompt), ChatMessage::user(input)]);

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

fn post_plan_creation_decision_requests_execution<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> bool
where
    P: ControllerProvider,
{
    let Some(context) = agent_verified_memory_context(session, false).prompt_context else {
        return false;
    };
    let request = provider.request_metadata();
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_post_plan_classifier", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(context),
        ChatMessage::system(AGENT_POST_PLAN_CREATION_DECISION_PROMPT),
        ChatMessage::user(input),
    ];

    let output = match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => output,
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider post-plan classifier request {} failed: {error}",
                request.provider, request.request_id
            ))));
            return false;
        }
    };

    let assistant_text = output.text.clone();
    push_provider_finished(session, request.provider, request.request_id, output);
    match parse_normal_turn_decision(&assistant_text) {
        Some(NormalTurnDecision::Execute {
            intent:
                Some(
                    NormalTurnExecuteIntent::PlanExecution
                    | NormalTurnExecuteIntent::PlanCreationAndExecution,
                ),
        }) => {
            session.push_reasoning_model_decision("post-plan classifier selected plan execution");
            true
        }
        Some(NormalTurnDecision::Execute { intent: None }) => {
            session.push_reasoning_model_decision(
                "post-plan classifier selected generic execute; kept plan-only boundary",
            );
            false
        }
        Some(NormalTurnDecision::Execute {
            intent: Some(NormalTurnExecuteIntent::ShellExecution),
        }) => {
            session.push_reasoning_model_decision(
                "post-plan classifier selected shell execution; kept plan-only boundary",
            );
            false
        }
        Some(NormalTurnDecision::State { answer_kind }) => {
            session.push_reasoning_model_decision(format!(
                "post-plan classifier kept plan-only state{}",
                answer_kind
                    .map(|kind| format!(" ({})", kind.as_str()))
                    .unwrap_or_default()
            ));
            false
        }
        Some(NormalTurnDecision::Chat { .. }) | Some(NormalTurnDecision::AskGuidance { .. }) => {
            session.push_reasoning_model_decision("post-plan classifier did not request execution");
            false
        }
        None => {
            session
                .push_reasoning_runtime_check("post-plan classifier returned no valid route JSON");
            false
        }
    }
}

fn retry_plain_agent_state_with_verified_plan_context<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    retry_plain_agent_chat_for_route_json_with_repair(
        provider,
        session,
        input,
        AGENT_ROUTE_STATE_WITH_PLAN_REPAIR_PROMPT,
        true,
    )
}

/// Secondary classification call: only used when the lean route prompt chose
/// `state` without pinning a valid `answer_kind`. Keeps the always-sent route
/// prompt cheap while still resolving the precise verified-state view reliably.
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
        ChatMessage::system(AGENT_STATE_KIND_CLASSIFIER_PROMPT),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            let kind = parse_state_answer_kind_from_text(&text);
            if kind.is_some() {
                session.push_reasoning_model_decision(
                    "state kind classifier resolved the verified-state view",
                );
            } else {
                session.push_reasoning_runtime_check(
                    "state kind classifier returned no valid answer kind",
                );
            }
            kind
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider state classifier request {} failed: {error}",
                request.provider, request.request_id
            ))));
            None
        }
    }
}

fn parse_state_answer_kind_from_text(text: &str) -> Option<VerifiedStateAnswerKind> {
    let trimmed = text.trim();
    let json_value = serde_json::from_str::<Value>(trimmed).ok().or_else(|| {
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        (start < end)
            .then(|| serde_json::from_str::<Value>(&trimmed[start..=end]).ok())
            .flatten()
    });
    if let Some(kind) = json_value
        .as_ref()
        .and_then(|value| value.get("answer_kind"))
        .and_then(Value::as_str)
        .and_then(parse_verified_state_answer_kind)
    {
        return Some(kind);
    }
    parse_verified_state_answer_kind(trimmed)
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
                plan_creation_execution: matches!(
                    intent,
                    Some(NormalTurnExecuteIntent::PlanCreationAndExecution)
                ),
                shell_execution: matches!(intent, Some(NormalTurnExecuteIntent::ShellExecution))
                    || (intent.is_none() && input_contains_executable_command_shape(input)),
                after_plan_creation_decision: intent.is_none(),
                explicit_tool_command: false,
            };
            session.record_reasoning_route("execute");
            if execution_intent.plan_execution {
                session.push_reasoning_model_decision(
                    "normal turn decision selected execute intent plan_execution",
                );
            } else if execution_intent.plan_creation_execution {
                session.push_reasoning_model_decision(
                    "normal turn decision selected execute intent plan_creation_execution",
                );
            } else if execution_intent.shell_execution {
                session.push_reasoning_model_decision(
                    "normal turn decision selected execute intent shell_execution",
                );
            } else {
                session.push_reasoning_model_decision("normal turn decision selected execute");
            }
            PlainAgentChatOutcome::Execute(execution_intent)
        }
        Some(NormalTurnDecision::State { answer_kind }) => {
            session.record_reasoning_route("state");
            session.push_reasoning_model_decision("normal turn decision selected state");
            let answer_kind = answer_kind
                .or_else(|| classify_verified_state_answer_kind(provider, session, input));
            let Some(answer_kind) = answer_kind else {
                session.push_reasoning_runtime_check(
                    "state route without a resolved answer kind; asked for guidance",
                );
                push_plain_provider_message_if_visible(
                    session,
                    "Which verified detail do you want: the latest plan, what was just done, created files, pending actions, or status?".to_string(),
                );
                return PlainAgentChatOutcome::Finished;
            };
            if allow_route_retry
                && state_answer_kind_can_mask_plan_execution_followup(answer_kind)
                && latest_structured_plan_has_missing_paths(session)
            {
                session.push_reasoning_model_decision(
                    "state route selected generic status with an incomplete verified plan; retrying route JSON",
                );
                return retry_plain_agent_state_with_verified_plan_context(
                    provider, session, input,
                );
            }
            push_verified_state_answer(session, answer_kind);
            PlainAgentChatOutcome::Finished
        }
        Some(NormalTurnDecision::Chat { content }) => {
            if allow_route_retry && session.latest_runtime_block_if_recent().is_some() {
                session.push_reasoning_model_decision(
                    "normal turn decision returned chat with a recorded runtime block; retrying route JSON",
                );
                return retry_plain_agent_chat_for_route_json_with_repair(
                    provider,
                    session,
                    input,
                    AGENT_ROUTE_RUNTIME_BLOCK_CHAT_REPAIR_PROMPT,
                    true,
                );
            }
            if !allow_route_retry && session.latest_runtime_block_if_recent().is_some() {
                session.record_reasoning_route("state");
                session.push_reasoning_model_decision(
                    "runtime block route repair still returned chat; surfaced verified last_block",
                );
                push_verified_state_answer(session, VerifiedStateAnswerKind::LastBlock);
                return PlainAgentChatOutcome::Finished;
            }
            if allow_route_retry && looks_like_local_work_chat_misroute(input, &content) {
                session.push_reasoning_model_decision(
                    "normal turn decision returned chat for local work-shaped input; retrying route JSON",
                );
                return retry_plain_agent_chat_for_route_json_with_repair(
                    provider,
                    session,
                    input,
                    AGENT_ROUTE_LOCAL_WORK_CHAT_REPAIR_PROMPT,
                    has_verified_session_state(session),
                );
            }
            if !allow_route_retry && looks_like_local_work_chat_misroute(input, &content) {
                session.record_reasoning_route("execute");
                session.push_reasoning_model_decision(
                    "normal turn decision returned chat for local work-shaped input after retry; routed to execute",
                );
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
            }
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
            if looks_like_misrouted_artifact_chat_after_retry(&assistant_text) {
                session.record_reasoning_route("execute");
                session.push_reasoning_model_decision(
                    "normal turn decision returned raw artifact-like text after retry; routed to execute",
                );
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
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

fn push_verified_state_answer(session: &mut Session, answer_kind: VerifiedStateAnswerKind) {
    let input = session
        .latest_reasoning_trace()
        .map(|trace| trace.user_input.clone())
        .unwrap_or_default();
    let resolution = resolve_state_answer_kind(session, &input, answer_kind);
    if let Some(reason) = resolution.fallback_reason {
        session.push_reasoning_runtime_check(format!(
            "state answer kind resolved from {} to {}: {reason}",
            resolution.requested_kind.as_str(),
            resolution.resolved_kind.as_str()
        ));
    }
    session.trace_event(
        "state_answer",
        resolved_state_answer_trace_metadata(session, resolution),
    );
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        verified_session_state_answer(session, resolution.resolved_kind),
        AssistantMessageSource::VerifiedState,
    )));
}

fn state_answer_kind_can_mask_plan_execution_followup(kind: VerifiedStateAnswerKind) -> bool {
    matches!(
        kind,
        VerifiedStateAnswerKind::Pending
            | VerifiedStateAnswerKind::Status
            | VerifiedStateAnswerKind::Summary
            | VerifiedStateAnswerKind::PlanStatus
    )
}

fn latest_structured_plan_has_missing_paths(session: &Session) -> bool {
    session
        .project_memory()
        .latest_structured_plan()
        .is_some_and(|plan| plan.runtime_status() != StructuredProjectPlanStatus::Completed)
}

fn looks_like_misrouted_artifact_chat(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 1000 && path_count >= 3)
        || (path_count >= 3 && numbered_artifact_line_count(trimmed) >= 4)
}

fn looks_like_misrouted_artifact_chat_after_retry(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 500 && path_count >= 3)
        || (path_count >= 3 && numbered_artifact_line_count(trimmed) >= 4)
}

fn looks_like_local_work_chat_misroute(input: &str, content: &str) -> bool {
    !content.trim().is_empty()
        && !looks_like_raw_tool_protocol(content)
        && !content_echoes_original_input(input, content)
        && input_contains_local_work_syntax(input)
}

fn content_echoes_original_input(input: &str, content: &str) -> bool {
    let input = input.trim();
    !input.is_empty() && content.contains(input)
}

fn input_contains_local_work_syntax(input: &str) -> bool {
    local_path_like_token_count(input) > 0 || shell_syntax_token_count(input) > 0
}

fn input_contains_executable_command_shape(input: &str) -> bool {
    local_path_like_token_count(input) > 0
        && input
            .lines()
            .flat_map(command_shape_segments)
            .any(segment_starts_with_executable_command_shape)
}

fn command_shape_segments(line: &str) -> Vec<&str> {
    line.split([';', '|'])
        .flat_map(|segment| segment.split("&&"))
        .collect()
}

fn segment_starts_with_executable_command_shape(segment: &str) -> bool {
    for token in segment.split_whitespace() {
        if is_command_shape_env_assignment(token) {
            continue;
        }
        let Some(token) = clean_command_shape_token(token) else {
            return false;
        };
        return executable_token_exists_on_path(token);
    }
    false
}

fn is_command_shape_env_assignment(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn clean_command_shape_token(token: &str) -> Option<&str> {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    if token.is_empty()
        || token.contains('/')
        || token.contains('=')
        || token.starts_with('-')
        || token.contains('.')
        || !token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }
    Some(token)
}

fn executable_token_exists_on_path(token: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| dir.join(token).is_file())
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

fn shell_syntax_token_count(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| {
            let token = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
                )
            });
            is_shell_option_token(token) || is_env_assignment_token(token)
        })
        .count()
}

fn is_shell_option_token(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn is_env_assignment_token(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn explicit_project_root_from_input(session: &Session, input: &str) -> Option<PathBuf> {
    input
        .split_whitespace()
        .find_map(|token| explicit_project_root_token(session, token))
}

fn explicit_project_root_token(session: &Session, token: &str) -> Option<PathBuf> {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
        )
    });
    if token.is_empty()
        || token.contains('*')
        || token.contains('?')
        || token.contains(':')
        || !(token.contains('/') || token.starts_with('/'))
    {
        return None;
    }

    let path = Path::new(token);
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains('.'))
    {
        return None;
    }
    if !path.is_absolute()
        && path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }

    let root = normalize_path(&absolute_session_path(session, path));
    (root != session.cwd && path_is_within(&root, &session.project_root)).then_some(root)
}

fn numbered_artifact_line_count(content: &str) -> usize {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
            digit_count > 0
                && trimmed[digit_count..]
                    .chars()
                    .next()
                    .is_some_and(|ch| matches!(ch, '.' | ')' | ':' | '-'))
        })
        .count()
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
        || session.latest_plan_contract().is_some()
}

fn agent_local_runtime_context(session: &mut Session) -> Option<String> {
    let project_root = session.project_root.clone();
    let cwd = session.cwd.clone();
    let max_window_tokens = session.context_accounting().max_window_tokens;
    let bundle = ContextBundle::from_default_local_files(project_root, cwd, max_window_tokens);
    session.set_context_accounting(bundle.accounting.clone());
    let cwd_relative = session
        .cwd
        .strip_prefix(&session.project_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    let runtime_context = format!(
        "Elgar runtime session:\n- project_root: {}\n- cwd: {}\n- cwd_relative_to_project_root: {}\n- current/root/this folder/project refers to cwd; use cwd `.` for shell commands targeting it.",
        session.project_root.display(),
        session.cwd.display(),
        cwd_relative
    );

    Some(match bundle.system_context() {
        Some(context) => format!("{runtime_context}\n\n{context}"),
        None => runtime_context,
    })
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

fn anchor_prompt_project_root_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    prompt_project_root: Option<&Path>,
) -> Vec<ResolvedAgentToolOutput> {
    let Some(prompt_project_root) = prompt_project_root else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request = anchor_prompt_project_root_action_request(
                    session,
                    action.request,
                    prompt_project_root,
                );
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn anchor_prompt_project_root_action_request(
    session: &Session,
    request: ActionRequest,
    prompt_project_root: &Path,
) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::CreateFile(action)
        }
        ActionRequest::CreateDirectory(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::CreateDirectory(action)
        }
        ActionRequest::OverwriteFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::OverwriteFile(action)
        }
        ActionRequest::PatchFile(mut action) => {
            action.target_path =
                anchor_prompt_project_root_path(session, &action.target_path, prompt_project_root);
            ActionRequest::PatchFile(action)
        }
        ActionRequest::ShellCommand(mut action) => {
            action.cwd = anchor_prompt_project_root_path(session, &action.cwd, prompt_project_root);
            ActionRequest::ShellCommand(action)
        }
        ActionRequest::DeleteFile(_) | ActionRequest::MoveFile(_) => request,
    }
}

fn anchor_prompt_project_root_path(
    session: &Session,
    path: &Path,
    prompt_project_root: &Path,
) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let current_target = absolute_session_path(session, path);
    if path_is_within(&current_target, prompt_project_root) {
        return cwd_relative_path(session, &current_target);
    }
    if is_plan_path_or_contents(path, "") {
        if let Some(rebased_target) =
            rebase_sibling_project_path(session, path, prompt_project_root)
        {
            return cwd_relative_path(session, &rebased_target);
        }
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        return path.to_path_buf();
    }
    cwd_relative_path(session, &normalize_path(prompt_project_root.join(path)))
}

fn rebase_sibling_project_path(
    session: &Session,
    path: &Path,
    project_root: &Path,
) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }

    let project_parent = project_root.parent()?;
    let current_target = absolute_session_path(session, path);
    if !path_is_within(&current_target, project_parent)
        || path_is_within(&current_target, project_root)
    {
        return None;
    }

    let relative_to_parent = current_target.strip_prefix(project_parent).ok()?;
    let mut components = relative_to_parent.components();
    components.next()?;
    let remainder = components.as_path();
    if remainder.as_os_str().is_empty() {
        return None;
    }

    Some(normalize_path(project_root.join(remainder)))
}

fn guard_shell_execution_tool_outputs(
    session: &mut Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    shell_execution: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if !shell_execution {
        return outputs;
    }

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action)
                if matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                if let ActionRequest::ShellCommand(shell) = action.request {
                    let (shell, dropped) =
                        drop_preexisting_shell_expected_paths(session, shell);
                    if dropped {
                        session.push_reasoning_runtime_check(
                            "ignored shell expected paths that already existed before execution",
                        );
                    }
                    action.request = ActionRequest::ShellCommand(shell);
                }
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action)
                if shell_execution_new_create_target_allowed(session, &action.request) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: action.tool_call_id,
                message: "Skipped non-shell tool call because this execute intent requires shell_command or a new create target.".to_string(),
                visible: false,
            },
            other => other,
        })
        .collect()
}

fn drop_preexisting_shell_expected_paths(
    session: &Session,
    mut shell: ShellCommandAction,
) -> (ShellCommandAction, bool) {
    let mut dropped = false;

    if shell
        .expected_file
        .as_ref()
        .is_some_and(|path| absolute_session_path(session, path).is_file())
    {
        shell.expected_file = None;
        dropped = true;
    }

    let original_file_count = shell.expected_files.len();
    shell
        .expected_files
        .retain(|path| !absolute_session_path(session, path).is_file());
    dropped |= shell.expected_files.len() != original_file_count;

    if shell
        .expected_directory
        .as_ref()
        .is_some_and(|path| absolute_session_path(session, path).is_dir())
    {
        shell.expected_directory = None;
        dropped = true;
    }

    let original_directory_count = shell.expected_directories.len();
    shell
        .expected_directories
        .retain(|path| !absolute_session_path(session, path).is_dir());
    dropped |= shell.expected_directories.len() != original_directory_count;

    (shell, dropped)
}

fn shell_execution_new_create_target_allowed(session: &Session, request: &ActionRequest) -> bool {
    match request {
        ActionRequest::CreateFile(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::OverwriteFile(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::CreateDirectory(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => false,
    }
}

fn anchor_bare_plan_artifacts_to_batch_project_root(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    let Some(project_root) = infer_batch_project_root_for_bare_plan_artifact(session, &outputs)
    else {
        return outputs;
    };

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                action.request =
                    anchor_bare_plan_artifact_request(session, action.request, &project_root);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn infer_batch_project_root_for_bare_plan_artifact(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Option<PathBuf> {
    for request in outputs.iter().filter_map(|output| match output {
        ResolvedAgentToolOutput::Action(action)
            if is_bare_plan_artifact_request(&action.request) =>
        {
            Some(&action.request)
        }
        ResolvedAgentToolOutput::Action(_)
        | ResolvedAgentToolOutput::Guidance(_)
        | ResolvedAgentToolOutput::Skipped { .. } => None,
    }) {
        if let Some(root) = infer_project_root_from_plan_artifact_contents(session, request) {
            return Some(root);
        }
    }

    common_batch_project_root(session, outputs)
}

fn is_bare_plan_artifact_request(request: &ActionRequest) -> bool {
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
        _ => return false,
    };

    !path.is_absolute() && path_has_no_meaningful_parent(path)
}

fn infer_project_root_from_plan_artifact_contents(
    session: &Session,
    request: &ActionRequest,
) -> Option<PathBuf> {
    let contents = match request {
        ActionRequest::CreateFile(action) => &action.contents,
        ActionRequest::OverwriteFile(action) => &action.contents,
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => return None,
    };

    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with('|')
            || trimmed.contains("──")
            || trimmed.chars().any(char::is_whitespace)
            || !trimmed.ends_with('/')
        {
            return None;
        }

        let root = trimmed.trim_end_matches('/');
        let path = Path::new(root);
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) {
            return None;
        }

        let root = absolute_session_path(session, path);
        (root != session.cwd && path_is_within(&root, &session.project_root)).then_some(root)
    })
}

fn common_batch_project_root(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> Option<PathBuf> {
    let mut roots = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if !is_bare_plan_artifact_request(&action.request) =>
            {
                batch_project_root_candidate(session, &action.request)
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();

    let mut common = roots.pop()?;
    for root in roots {
        common = common_path_prefix(&common, &root)?;
    }

    (common != session.cwd && path_is_within(&common, &session.project_root)).then_some(common)
}

fn batch_project_root_candidate(session: &Session, request: &ActionRequest) -> Option<PathBuf> {
    let path = match request {
        ActionRequest::CreateFile(action) => absolute_session_path(session, &action.target_path)
            .parent()
            .map(Path::to_path_buf),
        ActionRequest::OverwriteFile(action) => absolute_session_path(session, &action.target_path)
            .parent()
            .map(Path::to_path_buf),
        ActionRequest::CreateDirectory(action) => {
            Some(absolute_session_path(session, &action.target_path))
        }
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => None,
    }?;

    if path == session.cwd {
        return None;
    }
    path_is_within(&path, &session.project_root).then_some(path)
}

fn common_path_prefix(left: &Path, right: &Path) -> Option<PathBuf> {
    let mut common = PathBuf::new();
    for (left, right) in left.components().zip(right.components()) {
        if left != right {
            break;
        }
        common.push(left.as_os_str());
    }
    (!common.as_os_str().is_empty()).then_some(common)
}

fn anchor_bare_plan_artifact_request(
    session: &Session,
    request: ActionRequest,
    project_root: &Path,
) -> ActionRequest {
    match request {
        ActionRequest::CreateFile(mut action)
            if path_has_no_meaningful_parent(&action.target_path) =>
        {
            action.target_path =
                cwd_relative_path(session, &project_root.join(&action.target_path));
            ActionRequest::CreateFile(action)
        }
        ActionRequest::OverwriteFile(mut action)
            if path_has_no_meaningful_parent(&action.target_path) =>
        {
            action.target_path =
                cwd_relative_path(session, &project_root.join(&action.target_path));
            ActionRequest::OverwriteFile(action)
        }
        other => other,
    }
}

fn path_has_no_meaningful_parent(path: &Path) -> bool {
    path.parent()
        .is_none_or(|parent| parent.as_os_str().is_empty())
}

fn guard_plan_creation_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    plan_created_this_turn: bool,
    plan_creation_repair_in_progress: bool,
    allow_implementation_after_plan_creation: bool,
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

    if has_existing_plan_contract_or_reference(session)
        && !latest_structured_plan_is_completed(session)
        && resolved_outputs_touch_structured_plan(session, &outputs)
    {
        return outputs;
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
        if allow_implementation_after_plan_creation {
            if resolved_outputs_touch_structured_plan(session, &outputs) {
                return outputs;
            }
            if plain_create_batch_can_run_as_execute(session, &outputs) {
                return outputs;
            }
            if has_verified_session_state(session) && resolved_outputs_are_shell_only(&outputs) {
                return outputs;
            }
            return outputs
                .into_iter()
                .map(|output| match output {
                    ResolvedAgentToolOutput::Guidance(guidance) => {
                        ResolvedAgentToolOutput::Skipped {
                            tool_call_id: guidance.tool_call_id,
                            message: plan_creation_first_message(),
                            visible: false,
                        }
                    }
                    ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: plan_creation_first_message(),
                        visible: false,
                    },
                    skipped => skipped,
                })
                .collect();
        }
        return outputs;
    }

    let mut allowed_plan_file_used = false;
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(action)
                if (plan_created_this_turn || plan_creation_repair_in_progress)
                    && !allow_implementation_after_plan_creation =>
            {
                ResolvedAgentToolOutput::Skipped {
                    tool_call_id: action.tool_call_id,
                    message: "Skipped implementation tool calls after creating the verified plan. Ask to execute the plan when you want to apply it.".to_string(),
                    visible: true,
                }
            }
            ResolvedAgentToolOutput::Action(action)
                if (plan_created_this_turn || plan_creation_repair_in_progress)
            =>
            {
                ResolvedAgentToolOutput::Action(action)
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
                if allow_implementation_after_plan_creation {
                    ResolvedAgentToolOutput::Action(action)
                } else {
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message: "Skipped extra implementation tool calls in this plan-creation turn. Ask to execute the verified plan when you want to apply it.".to_string(),
                        visible: true,
                    }
                }
            }
            ResolvedAgentToolOutput::Action(action) if allow_implementation_after_plan_creation => {
                ResolvedAgentToolOutput::Action(action)
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

fn plain_create_batch_can_run_as_execute(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    if has_existing_plan_contract_or_reference(session)
        && !latest_structured_plan_is_completed(session)
    {
        return false;
    }

    let mut saw_action = false;
    for output in outputs {
        match output {
            ResolvedAgentToolOutput::Action(action)
                if matches!(
                    action.request,
                    ActionRequest::CreateFile(_)
                        | ActionRequest::CreateDirectory(_)
                        | ActionRequest::OverwriteFile(_)
                ) =>
            {
                saw_action = true;
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => return false,
        }
    }

    saw_action
}

fn latest_structured_plan_is_completed(session: &Session) -> bool {
    session
        .project_memory()
        .latest_structured_plan()
        .is_some_and(|plan| plan.runtime_status() == StructuredProjectPlanStatus::Completed)
}

fn has_existing_plan_contract_or_reference(session: &Session) -> bool {
    session.latest_plan_contract().is_some()
        || session.project_memory().latest_verified_plan().is_some()
        || session.project_memory().latest_structured_plan().is_some()
}

fn resolved_outputs_are_shell_only(outputs: &[ResolvedAgentToolOutput]) -> bool {
    let mut saw_action = false;
    for output in outputs {
        match output {
            ResolvedAgentToolOutput::Action(action)
                if matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                saw_action = true;
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => return false,
        }
    }
    saw_action
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AllSkippedToolResultSignature {
    messages: Vec<String>,
}

fn all_skipped_tool_result_signature(
    outputs: &[ResolvedAgentToolOutput],
) -> Option<AllSkippedToolResultSignature> {
    if outputs.is_empty() {
        return None;
    }

    let mut messages = Vec::with_capacity(outputs.len());
    for output in outputs {
        let ResolvedAgentToolOutput::Skipped { message, .. } = output else {
            return None;
        };
        messages.push(message.clone());
    }

    Some(AllSkippedToolResultSignature { messages })
}

fn repeated_identical_skip_breaker_message(signature: &AllSkippedToolResultSignature) -> String {
    let mut messages = Vec::<&str>::new();
    for message in &signature.messages {
        if !messages.iter().any(|seen| *seen == message.as_str()) {
            messages.push(message);
        }
    }

    format!(
        "Stopped because the model repeated the same blocked tool result without any verified action. Last block: {}",
        messages.join(" ")
    )
}

fn prioritize_plan_creation_tool_outputs(
    session: &Session,
    outputs: Vec<ResolvedAgentToolOutput>,
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
    if plan_roots.is_empty() {
        return outputs;
    }

    let mut setup = Vec::new();
    let mut plans = Vec::new();
    let mut rest = Vec::new();
    for output in outputs {
        match &output {
            ResolvedAgentToolOutput::Action(action)
                if is_plan_parent_setup_action(session, &action.request, &plan_roots) =>
            {
                setup.push(output);
            }
            ResolvedAgentToolOutput::Action(action)
                if plan_creation_root_for_action(session, &action.request).is_some() =>
            {
                plans.push(output);
            }
            ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. }
            | ResolvedAgentToolOutput::Action(_) => {
                rest.push(output);
            }
        }
    }

    setup.extend(plans);
    setup.extend(rest);
    setup
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

fn plan_creation_non_plan_repair_skip_message() -> String {
    "Skipped non-plan repair action. Update the same verified plan file before execution."
        .to_string()
}

fn plan_creation_first_message() -> String {
    "Create the project plan file first, then create implementation files from the verified plan."
        .to_string()
}

fn no_tool_action_repair_message() -> String {
    "This route requires tool actions. Use create_file/create_directory/overwrite_file for filesystem changes, shell_command for command execution, or ask concise guidance if a required target is missing.".to_string()
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
    allow_shell_commands: bool,
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
                if allow_shell_commands
                    && matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => {
                if let Some(message) =
                    nonconstructive_plan_execution_skip_message(session, plan, &action.request)
                {
                    let visible = is_off_plan_file_creation_attempt(session, plan, &action.request);
                    ResolvedAgentToolOutput::Skipped {
                        tool_call_id: action.tool_call_id,
                        message,
                        visible,
                    }
                } else {
                    ResolvedAgentToolOutput::Action(action)
                }
            }
            other => other,
        })
        .collect()
}

fn resolved_outputs_complete_missing_plan_paths(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
) -> bool {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return false;
    };

    let create_files = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => match &action.request {
                ActionRequest::CreateFile(file) => {
                    Some(absolute_session_path(session, &file.target_path))
                }
                ActionRequest::OverwriteFile(file) => {
                    Some(absolute_session_path(session, &file.target_path))
                }
                ActionRequest::CreateDirectory(_)
                | ActionRequest::PatchFile(_)
                | ActionRequest::DeleteFile(_)
                | ActionRequest::MoveFile(_)
                | ActionRequest::ShellCommand(_) => None,
            },
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    let create_directories = outputs
        .iter()
        .filter_map(|output| match output {
            ResolvedAgentToolOutput::Action(action) => match &action.request {
                ActionRequest::CreateDirectory(directory) => {
                    Some(absolute_session_path(session, &directory.target_path))
                }
                ActionRequest::CreateFile(_)
                | ActionRequest::OverwriteFile(_)
                | ActionRequest::PatchFile(_)
                | ActionRequest::DeleteFile(_)
                | ActionRequest::MoveFile(_)
                | ActionRequest::ShellCommand(_) => None,
            },
            ResolvedAgentToolOutput::Guidance(_) | ResolvedAgentToolOutput::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();

    let files_satisfied = plan.expected_files.iter().all(|expected| {
        expected.is_file()
            || create_files
                .iter()
                .any(|created| normalize_path(created) == *expected)
    });
    let directories_satisfied = plan.expected_directories.iter().all(|expected| {
        expected.is_dir()
            || create_directories
                .iter()
                .any(|created| normalize_path(created) == *expected)
            || create_files
                .iter()
                .any(|created| path_is_within(created, expected))
    });

    files_satisfied && directories_satisfied
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

fn nonconstructive_plan_execution_skip_message(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    request: &ActionRequest,
) -> Option<String> {
    match request {
        ActionRequest::CreateFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            if !structured_plan_expects_path(plan, &target_path) {
                return Some(off_plan_file_creation_skip_message(session, &target_path));
            }
            target_path.is_file().then(|| {
                "Skipped tool call because the expected file already exists in the verified plan."
                    .to_string()
            })
        }
        ActionRequest::OverwriteFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            if !structured_plan_expects_path(plan, &target_path) {
                return Some(off_plan_file_creation_skip_message(session, &target_path));
            }
            target_path.is_file().then(|| {
                "Skipped tool call because the expected file already exists in the verified plan."
                    .to_string()
            })
        }
        ActionRequest::CreateDirectory(_) => None,
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_) => Some(
            "Skipped tool call because it does not create a missing expected path from the verified plan."
                .to_string(),
        ),
        ActionRequest::ShellCommand(_) => Some(
            "Skipped shell command during verified plan execution; verification commands are recorded in the plan and should be run separately unless the plan explicitly includes a generated script or output path."
                .to_string(),
        ),
    }
}

fn is_off_plan_file_creation_attempt(
    session: &Session,
    plan: &crate::session::StructuredProjectPlan,
    request: &ActionRequest,
) -> bool {
    match request {
        ActionRequest::CreateFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path)
        }
        ActionRequest::OverwriteFile(action) => {
            let target_path = absolute_session_path(session, &action.target_path);
            !structured_plan_expects_path(plan, &target_path)
        }
        ActionRequest::CreateDirectory(_)
        | ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => false,
    }
}

fn off_plan_file_creation_skip_message(session: &Session, target_path: &Path) -> String {
    format!(
        "Skipped off-plan file `{}` because it is not listed in the verified plan. Verification commands can stay in the plan's Verification section; create a script file only when that file is explicitly included in the plan scope.",
        display_agent_context_path(session, target_path)
    )
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
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        if path_is_within(&current_target, &plan.project_root) {
            return cwd_relative_path(session, &current_target);
        }
    }
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return path.to_path_buf();
    };
    if path_is_within(&current_target, &plan.project_root) {
        return cwd_relative_path(session, &current_target);
    }

    if let Some(rebased_target) = rebase_sibling_project_path(session, path, &plan.project_root) {
        if structured_plan_expects_path(plan, &rebased_target)
            || structured_plan_expects_child_under(plan, &rebased_target)
        {
            return cwd_relative_path(session, &rebased_target);
        }
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

fn should_preflight_verified_plan_tool_outputs(
    session: &Session,
    outputs: &[ResolvedAgentToolOutput],
    plan_execution_batch: bool,
    plan_execution_intent: bool,
) -> bool {
    if plan_execution_batch || plan_execution_intent {
        return true;
    }

    session.project_memory().latest_verified_plan().is_some()
        && session.project_memory().latest_structured_plan().is_none()
        && outputs.iter().any(|output| {
            matches!(
                output,
                ResolvedAgentToolOutput::Action(_) | ResolvedAgentToolOutput::Guidance(_)
            )
        })
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
    } else if let Some(path) = project_root_relative_path_from_cwd_prefixed_relative(session, path)
    {
        path
    } else {
        session.cwd.join(path)
    })
}

fn project_root_relative_path_from_cwd_prefixed_relative(
    session: &Session,
    path: &Path,
) -> Option<PathBuf> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }

    let cwd_relative = session.cwd.strip_prefix(&session.project_root).ok()?;
    if cwd_relative.as_os_str().is_empty() || !path.starts_with(cwd_relative) {
        return None;
    }

    Some(session.project_root.join(path))
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
    lines.push("Use create_files for multiple missing expected paths when possible; otherwise use create_directory for missing expected directories and create_file for missing expected files under the verified plan root. Do not ask whether to create expected paths.".to_string());
    lines.push(
        "When multiple expected paths are missing, call the needed file and directory tools in one assistant response when possible."
            .to_string(),
    );
    Some(lines.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MissingPlanPathCounts {
    directories: usize,
    files: usize,
}

impl MissingPlanPathCounts {
    fn total(self) -> usize {
        self.directories + self.files
    }
}

fn missing_expected_plan_path_counts(session: &Session) -> MissingPlanPathCounts {
    MissingPlanPathCounts {
        directories: missing_expected_plan_directories(session).len(),
        files: missing_expected_plan_files(session).len(),
    }
}

fn plan_execution_no_progress_message(session: &Session) -> Option<String> {
    missing_expected_plan_paths_message(session).map(|message| {
        format!(
            "{message}\nStopped because the last tool response did not create any remaining expected plan paths."
        )
    })
}

fn plan_execution_incomplete_after_partial_batch_message(message: String) -> String {
    format!(
        "{message}\nStopped after creating verified plan paths because this batch did not complete the verified plan. No further model repair request was sent."
    )
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
    session.trace_event(
        "policy_decision",
        json!({
            "action_id": &proposed.id,
            "action_kind": format!("{:?}", proposed.kind()),
            "mode": policy_decision.mode.as_str(),
            "kind": format!("{:?}", policy_decision.kind),
            "user_approval_required": policy_decision.user_approval_required,
            "filesystem_verification_required": policy_decision.filesystem_verification_required,
            "reason_chars": policy_decision.reason.chars().count(),
        }),
    );

    if policy_decision.user_approval_required {
        return propose_agent_action_for_review(session, proposed, policy_decision);
    }

    let action = proposed.approve();
    let approval_source = policy_decision.approval_source.clone();
    let index = session.actions().len();
    let mut record = ActionRecord::new(action.clone());
    record.policy_decision = Some(policy_decision);
    session.push_action(record);

    let mut approved_event = action_event_for_action(&action);
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
            session.push_event(Event::ActionFailed(action_failed_for_action(
                &execution_action,
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
        action_event_for_action(&action).with_target(target.clone()),
    ));
    session.push_action(record);

    format!(
        "Proposed {:?} for review at {target}. Wait for the user to approve or reject before treating it as done.",
        action.kind()
    )
}

fn action_event_for_action(action: &Action) -> ActionEvent {
    let mut event = ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
        .with_target(action.request.approval_target());
    if let ActionRequest::ShellCommand(shell) = &action.request {
        event = event.with_shell_details(
            shell.cwd.display().to_string(),
            shell.timeout_seconds,
            shell.expected_effect.clone(),
        );
    }
    event
}

fn action_failed_for_action(action: &Action, reason: impl Into<String>) -> ActionFailed {
    let mut failed = ActionFailed::new(action.id.clone(), action.kind(), reason.into())
        .with_target(action.request.approval_target());
    if let ActionRequest::ShellCommand(shell) = &action.request {
        failed = failed.with_shell_details(
            shell.cwd.display().to_string(),
            shell.timeout_seconds,
            shell.expected_effect.clone(),
        );
    }
    failed
}

fn policy_decision_for_agent_action(
    session: &Session,
    mode: PermissionPolicyMode,
    action: &Action,
) -> PolicyDecision {
    if let ActionRequest::ShellCommand(shell) = &action.request {
        if is_read_only_shell_command(shell) {
            return PolicyDecision::allow_apply(
                mode,
                "policy allowlist permits read-only shell inspection commands",
            );
        }
    }

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
    session.clear_runtime_block();
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
    allow_unverified_provider_message: bool,
) {
    if turn_has_verified_action_applied(session, turn_start_index) {
        debug_assert!(session
            .actions_in_latest_action_turn()
            .iter()
            .any(|record| record.verified_result.is_some()));
        return;
    }

    let message = message.into();
    if allow_unverified_provider_message {
        push_provider_message_if_visible(session, message);
        return;
    }

    if provider_visible_text_from_text_only_output(message).is_some() {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            "No verified filesystem change occurred this turn, so no completion claim was recorded.",
            AssistantMessageSource::Controller,
        )));
    }
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

fn agent_verified_memory_context(
    session: &mut Session,
    include_durable: bool,
) -> AgentVerifiedMemoryContext {
    let mut selected = Vec::new();
    let mut lines = Vec::new();
    let latest_folder = latest_verified_folder_for_prompt(session).cloned();
    if let Some(folder) = latest_folder.as_ref() {
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
    append_verified_artifact_memory_context(
        session,
        latest_folder.as_ref().map(|folder| folder.path.as_path()),
        include_durable,
        &mut lines,
        &mut selected,
    );

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

const VERIFIED_ARTIFACT_LATEST_TURN_LIMIT: usize = 4;
const VERIFIED_ARTIFACT_LATEST_LIMIT: usize = 6;
const VERIFIED_ARTIFACT_EARLIEST_LIMIT: usize = 3;
const VERIFIED_ARTIFACT_FOLDER_LIMIT: usize = 4;
const DURABLE_VERIFIED_ARTIFACT_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VerifiedArtifactPromptKey {
    action_id: String,
    path: PathBuf,
}

impl VerifiedArtifactPromptKey {
    fn from_artifact(artifact: &VerifiedArtifactFact) -> Self {
        Self {
            action_id: artifact.action_id.clone(),
            path: artifact.path.clone(),
        }
    }
}

fn append_verified_artifact_memory_context(
    session: &Session,
    latest_folder: Option<&Path>,
    include_durable: bool,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
) {
    let latest_turn = latest_action_turn_artifacts(session, VERIFIED_ARTIFACT_LATEST_TURN_LIMIT);
    let latest = latest_verified_artifacts(session, VERIFIED_ARTIFACT_LATEST_LIMIT);
    let earliest = earliest_verified_artifacts(session, VERIFIED_ARTIFACT_EARLIEST_LIMIT);
    let under_latest_folder = latest_folder
        .map(|folder| {
            (
                folder,
                verified_artifacts_under_folder(session, folder, VERIFIED_ARTIFACT_FOLDER_LIMIT),
            )
        })
        .filter(|(_folder, artifacts)| !artifacts.artifacts.is_empty());

    if latest_turn.artifacts.is_empty()
        && latest.artifacts.is_empty()
        && earliest.artifacts.is_empty()
        && under_latest_folder.is_none()
    {
        if include_durable {
            append_durable_verified_artifact_memory_context(
                session,
                lines,
                selected,
                &HashSet::new(),
            );
        }
        return;
    }

    lines.push("- verified artifacts from prior actions:".to_string());
    let mut emitted = HashSet::new();
    append_artifact_group(
        session,
        lines,
        selected,
        "latest action turn",
        &latest_turn,
        &mut emitted,
    );
    append_artifact_group(
        session,
        lines,
        selected,
        "latest session artifacts",
        &latest,
        &mut emitted,
    );
    append_artifact_group(
        session,
        lines,
        selected,
        "earliest session artifacts",
        &earliest,
        &mut emitted,
    );
    if let Some((folder, artifacts)) = under_latest_folder {
        append_artifact_group(
            session,
            lines,
            selected,
            &format!(
                "artifacts under latest folder {}",
                display_agent_context_path(session, folder)
            ),
            &artifacts,
            &mut emitted,
        );
    }
    if include_durable {
        append_durable_verified_artifact_memory_context(session, lines, selected, &emitted);
    }
}

fn append_artifact_group(
    session: &Session,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
    label: &str,
    artifacts: &CappedVerifiedArtifacts,
    emitted: &mut HashSet<VerifiedArtifactPromptKey>,
) {
    if artifacts.artifacts.is_empty() {
        return;
    }

    let artifacts_to_emit = artifacts
        .artifacts
        .iter()
        .filter(|artifact| emitted.insert(VerifiedArtifactPromptKey::from_artifact(artifact)))
        .collect::<Vec<_>>();
    if artifacts_to_emit.is_empty() {
        return;
    }

    lines.push(format!("  - {label}:"));
    for artifact in artifacts_to_emit {
        lines.push(format!(
            "    - {}",
            verified_artifact_context_line(session, artifact)
        ));
        selected.push(ProviderPromptMemorySelectedFact::new(
            "verified_artifact",
            artifact.path.clone(),
            artifact.project_root.clone(),
            artifact.action_id.clone(),
        ));
    }
    if artifacts.omitted_count > 0 {
        lines.push(format!(
            "    - omitted {} older verified artifact(s) due to prompt cap",
            artifacts.omitted_count
        ));
    }
}

fn verified_artifact_context_line(session: &Session, artifact: &VerifiedArtifactFact) -> String {
    let mut line = format!(
        "{} turn {} {} {}",
        artifact.action_id,
        artifact.turn_index,
        artifact.operation,
        display_agent_context_path(session, &artifact.path)
    );
    if let Some(source_path) = artifact.source_path.as_ref() {
        line.push_str(&format!(
            " from {}",
            display_agent_context_path(session, source_path)
        ));
    }
    if let Some(project_root) = artifact.project_root.as_ref() {
        line.push_str(&format!(
            " under {}",
            display_agent_context_path(session, project_root)
        ));
    }
    line
}

fn append_durable_verified_artifact_memory_context(
    session: &Session,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
    in_memory_artifacts: &HashSet<VerifiedArtifactPromptKey>,
) {
    let durable = latest_durable_verified_artifacts(session, DURABLE_VERIFIED_ARTIFACT_LIMIT);
    let artifacts = durable
        .artifacts
        .iter()
        .filter(|artifact| {
            !in_memory_artifacts.contains(&VerifiedArtifactPromptKey {
                action_id: artifact.action_id.clone(),
                path: artifact.path.clone(),
            })
        })
        .collect::<Vec<_>>();
    if artifacts.is_empty() {
        return;
    }

    lines.push("- durable verified artifacts from local session logs:".to_string());
    for artifact in artifacts {
        lines.push(format!(
            "  - {}",
            durable_verified_artifact_context_line(session, artifact)
        ));
        selected.push(ProviderPromptMemorySelectedFact::new(
            "durable_verified_artifact",
            artifact.path.clone(),
            artifact.project_root.clone(),
            format!("{}:{}", artifact.session_id, artifact.action_id),
        ));
    }
    if durable.omitted_count > 0 {
        lines.push(format!(
            "  - omitted {} older durable verified artifact(s) due to prompt cap",
            durable.omitted_count
        ));
    }
}

fn durable_verified_artifact_context_line(
    session: &Session,
    artifact: &DurableVerifiedArtifactFact,
) -> String {
    let mut line = format!(
        "{}:{} turn {} {} {}",
        artifact.session_id,
        artifact.action_id,
        artifact.turn_index,
        artifact.operation,
        display_agent_context_path(session, &artifact.path)
    );
    if let Some(source_path) = artifact.source_path.as_ref() {
        line.push_str(&format!(
            " from {}",
            display_agent_context_path(session, source_path)
        ));
    }
    if let Some(project_root) = artifact.project_root.as_ref() {
        line.push_str(&format!(
            " under {}",
            display_agent_context_path(session, project_root)
        ));
    }
    line
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
    fn agent_prompts_describe_plan_artifact_before_same_turn_execution() {
        assert!(AGENT_SYSTEM_PROMPT
            .contains("create the plan file first, then implement the planned files"));
        assert!(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT
            .contains("same prompt creates plan then executes/implements it"));
        assert!(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT
            .contains("review current/root/this folder/project"));
        assert!(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT.len() <= 700);
    }

    #[test]
    fn explicit_project_root_rejects_url_and_scheme_tokens() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-root-token-guard",
            std::process::id()
        ));
        let session = Session::new("session", &root, &root);

        for token in [
            "https://nextjs.org/docs/messages/module-not-found",
            "http://example.com/path",
            "file:///tmp/thing",
            "scheme:opaque/value",
        ] {
            assert_eq!(
                explicit_project_root_token(&session, token),
                None,
                "scheme/url token must not become a project root: {token}"
            );
        }

        // A normal relative project path under the root is still accepted.
        assert_eq!(
            explicit_project_root_token(&session, "my-project/api"),
            Some(normalize_path(&root.join("my-project/api")))
        );
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
                if message.source == AssistantMessageSource::VerifiedState
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
        assert_eq!(
            provider.messages.lock().unwrap().len(),
            1,
            "completed plan execution should not request a destructive follow-up round"
        );
        assert_eq!(
            session
                .latest_reasoning_trace()
                .and_then(|trace| trace.route.as_deref()),
            Some("plan_execution")
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line
                == "plan execution completed after skipped tool feedback; skipped final provider synthesis")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_execution_reports_off_plan_verification_script_attempt() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-off-plan-verify-script",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("greeter")).unwrap();
        std::fs::write(
            cwd.join("greeter/plan.md"),
            "# Project Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "verify-readme-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "README.md",
                        "contents": "# Greeter\n"
                    }),
                    assistant_summary: Some("create readme".to_string()),
                },
                RawModelToolCall {
                    id: "verify-requirements-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
                RawModelToolCall {
                    id: "verify-main-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main".to_string()),
                },
                RawModelToolCall {
                    id: "verify-test-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "tests/test_main.py",
                        "contents": "def test_smoke():\n    assert True\n"
                    }),
                    assistant_summary: Some("create test".to_string()),
                },
                RawModelToolCall {
                    id: "verify-script-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "shell_verify.sh",
                        "contents": "python -m py_compile src/main.py tests/test_main.py\n"
                    }),
                    assistant_summary: Some("create verification script".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Done."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("greeter/plan.md"),
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
                path: "greeter/plan.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(cwd.join("greeter/README.md").is_file());
        assert!(cwd.join("greeter/requirements.txt").is_file());
        assert!(cwd.join("greeter/src/main.py").is_file());
        assert!(cwd.join("greeter/tests/test_main.py").is_file());
        assert!(!cwd.join("greeter/shell_verify.sh").exists());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("Skipped off-plan file")
                    && message.content.contains("shell_verify.sh")
                    && message
                        .content
                        .contains("Verification commands can stay in the plan's Verification section")
        )));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(
                |line| line.contains("Skipped off-plan file") && line.contains("shell_verify.sh")
            )));

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
        assert_eq!(
            provider.messages.lock().unwrap().len(),
            1,
            "completed plan execution should not request a redundant follow-up round"
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line
                == "plan execution completed after skipped tool feedback; skipped final provider synthesis")));

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
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I found the verified plan."),
            crate::event::ProviderOutput::new("I still need tool actions."),
        ]);
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
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I found the workspace."),
            crate::event::ProviderOutput::new("I still need tool actions."),
        ]);
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
    fn verified_artifact_memory_injects_created_files_into_tool_turns() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-artifact-context-tool",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let project = cwd.join("workspace");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&project).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I can use verified artifacts."),
            crate::event::ProviderOutput::new("No tool action needed."),
        ]);
        let mut session = Session::new("session", &root, &cwd);
        session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
            path: project.clone(),
            source_action_id: "action-folder".to_string(),
        });
        push_verified_file_record(&mut session, "action-readme", "workspace/README.md");
        push_verified_file_record(&mut session, "action-main", "workspace/src/main.py");

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue the project",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("- verified artifacts from prior actions:"));
        assert!(verified_context.content.contains("latest action turn"));
        assert!(verified_context.content.contains("action-main turn"));
        assert!(verified_context
            .content
            .contains("created_file workspace/src/main.py under workspace"));
        assert!(verified_context.content.contains("action-readme turn"));

        let selection = session
            .latest_provider_prompt_memory_selection()
            .expect("artifact prompt selection should be recorded");
        assert!(selection
            .selected
            .iter()
            .any(|fact| fact.kind == "verified_artifact"
                && fact.path.ends_with("workspace/src/main.py")));
        let artifact_facts = selection
            .selected
            .iter()
            .filter(|fact| fact.kind == "verified_artifact")
            .collect::<Vec<_>>();
        let unique_artifact_facts = artifact_facts
            .iter()
            .map(|fact| (fact.source_action_id.as_str(), fact.path.as_path()))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(artifact_facts.len(), unique_artifact_facts.len());
        assert_eq!(
            verified_context
                .content
                .matches("workspace/src/main.py")
                .count(),
            1
        );
        assert_eq!(
            verified_context
                .content
                .matches("workspace/README.md")
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_artifact_memory_stays_out_of_plain_chat() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-artifact-plain-clean",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);
        push_verified_file_record(&mut session, "action-notes", "notes.txt");

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        let joined = joined_request_messages(&requests[0]);
        assert!(!joined.contains("verified artifacts from prior actions"));
        assert!(!joined.contains("notes.txt"));
        assert!(session.latest_provider_prompt_memory_selection().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn durable_session_log_memory_injects_prior_artifacts_into_tool_turns() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-durable-artifact-context-tool",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_prior_session_log(
            &root,
            "prior-session",
            &[
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":2,"metadata":{"action_id":"action-shell","action_kind":"ShellCommand","operation":"shell_command","command_chars":7}}"#,
                "not json",
            ],
        );
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I can use durable verified artifacts."),
            crate::event::ProviderOutput::new("No tool action needed."),
        ]);
        let mut session = Session::new("current-session", &root, &root);

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "continue the project from the previous session",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("- durable verified artifacts from local session logs:"));
        assert!(verified_context
            .content
            .contains("prior-session:action-prior turn 1 file_written prior/README.md"));
        assert!(!verified_context.content.contains("action-shell"));

        let selection = session
            .latest_provider_prompt_memory_selection()
            .expect("durable artifact prompt selection should be recorded");
        assert!(selection.selected.iter().any(|fact| {
            fact.kind == "durable_verified_artifact"
                && fact.path.ends_with("prior/README.md")
                && fact.source_action_id == "prior-session:action-prior"
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn durable_session_log_memory_stays_out_of_plan_work_tool_turns() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-durable-artifact-plan-clean",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_prior_session_log(
            &root,
            "prior-session",
            &[
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
            ],
        );
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(crate::event::ProviderOutput::new("No tool action needed."));
        let mut session = Session::new("current-session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create a project plan, then execute it.",
        );

        let tool_request = only_tool_request(&provider);
        let joined = joined_request_messages(&tool_request);
        assert!(!joined.contains("durable verified artifacts"));
        assert!(!joined.contains("prior/README.md"));
        assert!(!session
            .latest_provider_prompt_memory_selection()
            .map(|selection| selection
                .selected
                .iter()
                .any(|fact| fact.kind == "durable_verified_artifact"))
            .unwrap_or(false));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn durable_session_log_memory_stays_out_of_plain_chat() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-durable-artifact-plain-clean",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_prior_session_log(
            &root,
            "prior-session",
            &[
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
            ],
        );
        let provider = CapturingProvider::new();
        let mut session = Session::new("current-session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        let joined = joined_request_messages(&requests[0]);
        assert!(!joined.contains("durable verified artifacts"));
        assert!(!joined.contains("prior/README.md"));
        assert!(session.latest_provider_prompt_memory_selection().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_artifact_memory_prompt_caps_and_reports_omitted_count() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-artifact-context-cap",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I can use capped verified artifacts."),
            crate::event::ProviderOutput::new("No tool action needed."),
        ]);
        let mut session = Session::new("session", &root, &root);
        for index in 1..=8 {
            push_verified_file_record(
                &mut session,
                &format!("action-{index}"),
                &format!("file-{index}.txt"),
            );
        }

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "use the first file you created",
            PermissionPolicyMode::FullAccess,
        );

        let messages = provider.messages.lock().unwrap();
        let verified_context = messages[0]
            .iter()
            .find(|message| message.content.contains("Verified filesystem context"))
            .expect("tool turn should include verified memory context");
        assert!(verified_context
            .content
            .contains("earliest session artifacts"));
        assert!(verified_context.content.contains("action-1 turn"));
        assert!(verified_context.content.contains("file-1.txt"));
        assert!(verified_context
            .content
            .contains("latest session artifacts"));
        assert!(verified_context.content.contains("action-8 turn"));
        assert!(verified_context.content.contains("file-8.txt"));
        assert!(verified_context
            .content
            .contains("omitted 2 older verified artifact(s) due to prompt cap"));
        assert!(verified_context
            .content
            .contains("omitted 5 older verified artifact(s) due to prompt cap"));

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
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("I can update it."),
            crate::event::ProviderOutput::new("I still need tool actions."),
        ]);
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
    fn verified_plan_preflight_allows_deduplicated_cwd_prefix_target() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-preflight-duplicate-prefix",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("demo")).unwrap();
        let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "duplicate-prefix-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "playground/demo/index.tsx",
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

        assert!(cwd.join("demo/index.tsx").is_file());
        assert!(!cwd.join("playground/demo/index.tsx").exists());
        assert_eq!(session.actions().len(), 1);

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
        tool_names: Vec<String>,
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

    #[derive(Debug, Clone)]
    enum CapturedToolStep {
        Output(crate::event::ProviderOutput),
        EmptyResponse,
    }

    #[derive(Debug, Clone)]
    struct CapturingProviderWithToolErrors {
        requests: std::sync::Arc<std::sync::Mutex<Vec<CapturedProviderRequest>>>,
        plain_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
        tool_steps: std::sync::Arc<std::sync::Mutex<Vec<CapturedToolStep>>>,
    }

    impl CapturingProviderWithToolErrors {
        fn new(
            plain_outputs: Vec<crate::event::ProviderOutput>,
            tool_steps: Vec<CapturedToolStep>,
        ) -> Self {
            Self {
                requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                plain_outputs: std::sync::Arc::new(std::sync::Mutex::new(plain_outputs)),
                tool_steps: std::sync::Arc::new(std::sync::Mutex::new(tool_steps)),
            }
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

        fn next_tool_step(&self) -> CapturedToolStep {
            let mut steps = self.tool_steps.lock().unwrap();
            if steps.is_empty() {
                CapturedToolStep::Output(crate::event::ProviderOutput::new("Done."))
            } else {
                steps.remove(0)
            }
        }
    }

    impl ControllerProvider for CapturingProviderWithToolErrors {
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
                tool_names: tools.into_iter().map(|tool| tool.function.name).collect(),
            });
            match self.next_tool_step() {
                CapturedToolStep::Output(output) => Ok(output),
                CapturedToolStep::EmptyResponse => {
                    Err(ProviderError::empty_response("empty tool response"))
                }
            }
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
                tool_names: Vec::new(),
            });
            Ok(self.next_plain_output())
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

    fn only_tool_request(provider: &CapturingProvider) -> CapturedProviderRequest {
        provider
            .requests()
            .into_iter()
            .find(|request| request.mode == CapturedProviderRequestMode::Tool)
            .expect("tool request should be captured")
    }

    fn push_verified_file_record(session: &mut Session, action_id: &str, path: &str) {
        session.start_reasoning_trace(format!("turn for {action_id}"));
        let action = Action::proposed(
            action_id,
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from(path),
                contents: String::new(),
            }),
            "create file",
        )
        .approve()
        .mark_applied();
        let mut record = ActionRecord::new(action);
        record.verified_result = Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileCreated {
                path: path.to_string(),
            },
        ));
        session.push_action(record);
    }

    fn write_prior_session_log(root: &Path, session_id: &str, lines: &[&str]) {
        let path = crate::local_session_log::session_log_file_path(root, session_id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n")).unwrap();
    }

    fn trace_events(root: &Path, session_id: &str) -> Vec<serde_json::Value> {
        let path = crate::local_trace::trace_file_path(root, session_id);
        let contents = std::fs::read_to_string(path).expect("trace file should exist");
        contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("trace line should be valid json"))
            .collect()
    }

    fn session_log_events(root: &Path, session_id: &str) -> Vec<serde_json::Value> {
        let path = crate::local_session_log::session_log_file_path(root, session_id);
        let contents = std::fs::read_to_string(path).expect("session log file should exist");
        contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("session log line should be valid json"))
            .collect()
    }

    fn trace_kinds(events: &[serde_json::Value]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| event.get("kind").and_then(serde_json::Value::as_str))
            .map(ToString::to_string)
            .collect()
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
                tool_names: tools.into_iter().map(|tool| tool.function.name).collect(),
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
                tool_names: Vec::new(),
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
    fn project_review_tool_turn_gets_runtime_location_context() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-runtime-location-context",
            std::process::id()
        ));
        let cwd = root.join("playground").join("Nextjs-1");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&cwd).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(crate::event::ProviderOutput::new("No tool action needed."));
        let mut session = Session::new("session", &root, &cwd);

        run_permissive_agent_turn(&provider, &mut session, "review the folder you are in");

        let tool_request = only_tool_request(&provider);
        let joined = joined_request_messages(&tool_request);
        assert!(joined.contains("Elgar runtime session:"));
        assert!(joined.contains(&format!("project_root: {}", root.display())));
        assert!(joined.contains(&format!("cwd: {}", cwd.display())));
        assert!(joined.contains("cwd_relative_to_project_root: playground/Nextjs-1"));
        assert!(joined.contains("current/root/this folder/project refers to cwd"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_chat_writes_redacted_local_trace_without_tools_or_memory() {
        std::env::set_var("ELGAR_TRACE", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plain-trace",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"Plain answer.\"}"),
        );
        let mut session = Session::new("trace-session", &root, &root);

        let result = run_permissive_agent_turn(
            &provider,
            &mut session,
            "hello secret-user-prompt-that-must-not-be-traced",
        );

        assert_eq!(result.route, Route::AskModel);
        let events = trace_events(&root, "trace-session");
        let kinds = trace_kinds(&events);
        assert!(kinds.contains(&"turn_start".to_string()));
        assert!(kinds.contains(&"provider_request_start".to_string()));
        assert!(kinds.contains(&"provider_request_finish".to_string()));
        assert!(kinds.contains(&"route_decision".to_string()));
        assert!(kinds.contains(&"turn_finish".to_string()));
        assert!(!kinds.contains(&"memory_selected".to_string()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret-user-prompt-that-must-not-be-traced"));
        assert!(events.iter().any(|event| {
            event.get("kind").and_then(serde_json::Value::as_str) == Some("provider_request_start")
                && event
                    .get("metadata")
                    .and_then(|metadata| metadata.get("tool_count"))
                    .and_then(serde_json::Value::as_u64)
                    == Some(0)
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_action_turn_writes_action_trace_without_file_contents() {
        std::env::set_var("ELGAR_TRACE", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-tool-trace",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "notes.txt",
                            "contents": "secret-file-contents-that-must-not-be-traced",
                        }),
                        assistant_summary: None,
                    },
                ]),
            );
        let mut session = Session::new("trace-session-tool", &root, &root);

        let result = run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a file with secret-file-contents-that-must-not-be-traced",
        );

        assert_eq!(result.route, Route::AskModel);
        assert!(root.join("notes.txt").is_file());
        let events = trace_events(&root, "trace-session-tool");
        let kinds = trace_kinds(&events);
        assert!(kinds.contains(&"tool_call_validated".to_string()));
        assert!(kinds.contains(&"policy_decision".to_string()));
        assert!(kinds.contains(&"action_approved".to_string()));
        assert!(kinds.contains(&"action_applied".to_string()));
        assert!(kinds.contains(&"turn_finish".to_string()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret-file-contents-that-must-not-be-traced"));
        assert!(serialized.contains("notes.txt"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_writes_trace_metadata_without_prompt_text() {
        std::env::set_var("ELGAR_TRACE", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-trace",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("TracePlan");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("plan.md"), "# Trace Plan\n").unwrap();
        std::fs::write(project.join("README.md"), "# Trace\n").unwrap();
        std::fs::write(project.join("src/main.py"), "print('trace')\n").unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"route\":\"state\",\"answer_kind\":\"status\"}"),
        );
        let mut session = Session::new("trace-session-state", &root, &root);
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: project.join("plan.md"),
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Verified,
            expected_directories: vec![root.join("TracePlan/src")],
            expected_files: vec![
                root.join("TracePlan/README.md"),
                root.join("TracePlan/src/main.py"),
            ],
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "did you execute the trace plan secret-state-prompt",
        );

        let events = trace_events(&root, "trace-session-state");
        let state_answer = events
            .iter()
            .find(|event| {
                event.get("kind").and_then(serde_json::Value::as_str) == Some("state_answer")
            })
            .expect("state answer trace should be written");
        let metadata = state_answer
            .get("metadata")
            .expect("state answer should include metadata");
        assert_eq!(
            metadata
                .get("state_answer_kind")
                .and_then(serde_json::Value::as_str),
            Some("status")
        );
        assert_eq!(
            metadata
                .get("plan_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            metadata
                .get("answer_scope")
                .and_then(serde_json::Value::as_str),
            Some("session_status")
        );
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret-state-prompt"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_chat_writes_redacted_append_only_session_log() {
        std::env::set_var("ELGAR_SESSION_LOG", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plain-session-log",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"Plain answer secret-assistant-log-content.\"}",
            ));
        let mut session = Session::new("session-log-plain", &root, &root);

        let result = run_permissive_agent_turn(
            &provider,
            &mut session,
            "hello secret-user-session-log-content",
        );

        assert_eq!(result.route, Route::AskModel);
        let events = session_log_events(&root, "session-log-plain");
        let kinds = trace_kinds(&events);
        assert!(kinds.contains(&"turn_start".to_string()));
        assert!(kinds.contains(&"user_message".to_string()));
        assert!(kinds.contains(&"provider_request_start".to_string()));
        assert!(kinds.contains(&"provider_request_finish".to_string()));
        assert!(kinds.contains(&"route_decision".to_string()));
        assert!(kinds.contains(&"assistant_message".to_string()));
        assert!(kinds.contains(&"turn_finish".to_string()));
        assert!(!kinds.contains(&"memory_selected".to_string()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret-user-session-log-content"));
        assert!(!serialized.contains("secret-assistant-log-content"));
        assert!(events.iter().all(|event| {
            event.get("session_id").and_then(serde_json::Value::as_str) == Some("session-log-plain")
                && event.get("turn_index").and_then(serde_json::Value::as_u64) == Some(1)
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_action_turn_writes_session_log_without_file_contents() {
        std::env::set_var("ELGAR_SESSION_LOG", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-tool-session-log",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "notes.txt",
                            "contents": "secret-session-log-file-contents",
                        }),
                        assistant_summary: None,
                    },
                ]),
            );
        let mut session = Session::new("session-log-tool", &root, &root);

        let result = run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a file with secret-session-log-file-contents",
        );

        assert_eq!(result.route, Route::AskModel);
        assert!(root.join("notes.txt").is_file());
        let events = session_log_events(&root, "session-log-tool");
        let kinds = trace_kinds(&events);
        assert!(kinds.contains(&"tool_call_validated".to_string()));
        assert!(kinds.contains(&"policy_decision".to_string()));
        assert!(kinds.contains(&"action_approved".to_string()));
        assert!(kinds.contains(&"action_applied".to_string()));
        assert!(kinds.contains(&"turn_finish".to_string()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret-session-log-file-contents"));
        assert!(serialized.contains("notes.txt"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_resolves_empty_latest_folder_to_project_files() {
        std::env::set_var("ELGAR_TRACE", "on");
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-resolver-project-files",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("PostNewSmoke1");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
        std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
        std::fs::write(project.join("requirements.txt"), "").unwrap();
        std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
        std::fs::write(
            project.join("tests/test_main.py"),
            "def test_main(): pass\n",
        )
        .unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
            ));
        let mut session = Session::new("trace-session-resolver-project", &root, &root);
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: project.join("PLAN.md"),
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Completed,
            expected_directories: vec![
                root.join("PostNewSmoke1/src"),
                root.join("PostNewSmoke1/tests"),
            ],
            expected_files: vec![
                root.join("PostNewSmoke1/README.md"),
                root.join("PostNewSmoke1/requirements.txt"),
                root.join("PostNewSmoke1/src/main.py"),
                root.join("PostNewSmoke1/tests/test_main.py"),
            ],
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "What files did you create in playground/PostNewSmoke1?",
        );

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content.contains("project: PostNewSmoke1")
                    && message.content.contains("files: 4/4 present")
                    && message.content.contains("PostNewSmoke1/src/main.py")
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.content.contains("No verified folder creation recorded")
        )));

        let events = trace_events(&root, "trace-session-resolver-project");
        let state_answer = events
            .iter()
            .find(|event| {
                event.get("kind").and_then(serde_json::Value::as_str) == Some("state_answer")
            })
            .expect("state answer trace should be written");
        let metadata = state_answer
            .get("metadata")
            .expect("state answer should include metadata");
        assert_eq!(
            metadata
                .get("requested_state_answer_kind")
                .and_then(serde_json::Value::as_str),
            Some("latest_folder")
        );
        assert_eq!(
            metadata
                .get("resolved_state_answer_kind")
                .and_then(serde_json::Value::as_str),
            Some("project_files")
        );
        assert_eq!(
            metadata
                .get("state_answer_fallback_reason")
                .and_then(serde_json::Value::as_str),
            Some("requested_latest_folder_with_referenced_project_files")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_resolves_broad_created_summary_to_referenced_project_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-resolver-created-project",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("PostNewSmoke2");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
        std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
        std::fs::write(project.join("requirements.txt"), "").unwrap();
        std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
        std::fs::write(
            project.join("tests/test_main.py"),
            "def test_main(): pass\n",
        )
        .unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"created_summary\"}",
            ));
        let mut session = Session::new("session", &root, &root);
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: project.join("PLAN.md"),
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Completed,
            expected_directories: vec![
                root.join("PostNewSmoke2/src"),
                root.join("PostNewSmoke2/tests"),
            ],
            expected_files: vec![
                root.join("PostNewSmoke2/README.md"),
                root.join("PostNewSmoke2/requirements.txt"),
                root.join("PostNewSmoke2/src/main.py"),
                root.join("PostNewSmoke2/tests/test_main.py"),
            ],
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "What files did you create in PostNewSmoke2?",
        );

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content.contains("project: PostNewSmoke2")
                    && message.content.contains("files: 4/4 present")
                    && message.content.contains("PostNewSmoke2/tests/test_main.py")
                    && !message.content.contains("current session:")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_keeps_empty_latest_folder_without_better_state() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-empty-latest-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
            ));
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "what is the latest folder?");

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content == "No verified folder creation recorded."
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_latest_folder_reports_latest_project_root_without_file_fallback() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-latest-project-folder",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("StateResolverSmoke4");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
        std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
        std::fs::write(project.join("requirements.txt"), "").unwrap();
        std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
        std::fs::write(
            project.join("tests/test_main.py"),
            "def test_main(): pass\n",
        )
        .unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
            ));
        let mut session = Session::new("session", &root, &root);
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: project.join("PLAN.md"),
            project_root: project,
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Completed,
            expected_directories: vec![
                root.join("StateResolverSmoke4/src"),
                root.join("StateResolverSmoke4/tests"),
            ],
            expected_files: vec![
                root.join("StateResolverSmoke4/README.md"),
                root.join("StateResolverSmoke4/requirements.txt"),
                root.join("StateResolverSmoke4/src/main.py"),
                root.join("StateResolverSmoke4/tests/test_main.py"),
            ],
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "What is the latest folder you created?",
        );

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content == "StateResolverSmoke4"
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_answer_resolves_empty_kind_to_created_summary_for_artifacts_without_project() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-answer-resolver-created-summary",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
            ));
        let mut session = Session::new("session", &root, &root);
        push_verified_file_record(&mut session, "action-file", "standalone.txt");

        run_permissive_agent_turn(&provider, &mut session, "what did you create?");

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content == "current session:\n- file standalone.txt"
        )));

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
        std::fs::create_dir_all(root.join("Demo")).unwrap();
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
        std::fs::create_dir_all(root.join("Demo")).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    "# Project Plan\n\n```text\nRepairPlan/\nREADME.md\n```\n\n## Verification\n- Check files.\n\n## Acceptance Criteria\n- Files exist.\n",
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"done\"}"),
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
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: root.join("Demo"),
            source_action_id: "action-folder".to_string(),
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny app",
        );

        assert!(root.join("RepairPlan/PLAN.md").is_file());
        let requests = provider.requests();
        assert_eq!(requests.len(), 4);
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
        assert_eq!(requests[3].mode, CapturedProviderRequestMode::Plain);
        assert!(joined_request_messages(&requests[3]).contains("A verified plan was just created"));
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
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: root.join("Demo"),
            source_action_id: "action-folder".to_string(),
        });

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
    fn local_file_work_chat_route_retries_before_rendering_fake_claim() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-local-file-chat-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join("Demo")).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    r#"{"route":"chat","content":"Created USAGE.md with the specified content."}"#,
                ),
                crate::event::ProviderOutput::new(
                    r#"{"route":"chat","content":"Created USAGE.md in the project root."}"#,
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating USAGE.md.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "usage-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "Demo/USAGE.md",
                            "contents": "PYTHONPATH=src python -m demo.cli sample.txt\n"
                        }),
                        assistant_summary: Some("create usage file".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: root.join("Demo"),
            source_action_id: "action-folder".to_string(),
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create USAGE.md inside Demo containing exact text PYTHONPATH=src python -m demo.cli sample.txt.",
        );

        assert!(root.join("Demo/USAGE.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Created USAGE.md")
        )));
        let requests = provider.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.mode == CapturedProviderRequestMode::Plain)
                .count(),
            2
        );
        assert!(joined_request_messages(&requests[1]).contains("local filesystem or shell syntax"));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .model_decisions
            .iter()
            .any(|line| line.contains("local work-shaped input"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_work_chat_route_retries_before_rendering_fake_claim() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-chat-repair",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join("Demo")).unwrap();
        let expected_file = root.join("compile.out");
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    r#"{"route":"chat","content":"Compiled all Python files successfully."}"#,
                ),
                crate::event::ProviderOutput::new(
                    r#"{"route":"chat","content":"I executed the compile command successfully."}"#,
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Running compile command.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "compile-shell".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "printf ok > compile.out",
                            "cwd": root.display().to_string(),
                            "expected_file": expected_file.display().to_string()
                        }),
                        assistant_summary: Some("run verification command".to_string()),
                    }],
                ),
            );
        let mut session = Session::new("session", &root, &root);
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: root.join("Demo"),
            source_action_id: "action-folder".to_string(),
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "run python -m compileall src inside that project",
        );

        assert_eq!(std::fs::read_to_string(&expected_file).unwrap(), "ok");
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Compiled all Python")
        )));
        assert!(session.actions().iter().any(|record| {
            matches!(
                record.verified_result.as_ref(),
                Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(0)
            )
        }));

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
        assert!(numbered_artifact_line_count(artifact_markdown) >= 4);
        assert!(looks_like_misrouted_artifact_chat(artifact_markdown));
        assert!(looks_like_misrouted_artifact_chat_after_retry(
            artifact_markdown
        ));
    }

    #[test]
    fn short_numbered_plan_chat_counts_as_artifact_shape() {
        let artifact_markdown = r#"Plan:
1. Create folder playground/same-prompt-plan-execute-1.
2. Create README.md explaining project.
3. Create calculator.py with functions.
4. Create ui.py for a small CLI.
5. Add python -m unittest verification.
6. Create test_calculator.py to verify functions.
"#;

        assert!(local_path_like_token_count(artifact_markdown) >= 3);
        assert!(numbered_artifact_line_count(artifact_markdown) >= 4);
        assert!(looks_like_misrouted_artifact_chat(artifact_markdown));
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
    fn raw_artifact_text_after_route_retry_executes_instead_of_erroring() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-raw-artifact-after-route-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let artifact_markdown = format!(
            "{}\n{}",
            r#"Project Plan - Tiny Notes CLI

File tree:
playground/RawArtifactRetryPlan/
├── README.md
├── requirements.txt
└── src/
    └── main.py

Verification:
- Check that README.md exists.
- Check that requirements.txt exists.
- Check that src/main.py exists.

Acceptance Criteria:
- The plan file exists.
- The future implementation files are listed.
"#,
            "The README should document installation, usage, verification, and acceptance criteria for the future implementation.\n".repeat(8)
        );
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("I will draft the project plan."),
                crate::event::ProviderOutput::new(artifact_markdown),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan file.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "raw-artifact-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/RawArtifactRetryPlan/PLAN.md",
                            "contents": "# Raw Artifact Retry Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files are listed.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan for a tiny notes cli",
        );

        assert!(root
            .join("playground/RawArtifactRetryPlan/PLAN.md")
            .is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::Error(error)
                if error
                    .message
                    .contains("Model routing response was not valid JSON")
        )));
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert_eq!(trace.route.as_deref(), Some("plan_creation"));
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("raw artifact-like text after retry")));

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
    fn explicit_plan_creation_execution_intent_can_create_plan_then_files_same_turn() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-create-execute-intent",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-create-execute-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/plan.md",
                            "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files from plan.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-create-execute-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/README.md",
                                "contents": "# Calculator UI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-create-execute-calc".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/calculator.py",
                                "contents": "class Calculator:\n    pass\n"
                            }),
                            assistant_summary: Some("create calculator".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-create-execute-ui".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/ui.py",
                                "contents": "from calculator import Calculator\n"
                            }),
                            assistant_summary: Some("create ui".to_string()),
                        },
                    ],
                ),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a plan for the calculator UI and execute it",
        );

        assert!(root.join("CalculatorUI/plan.md").is_file());
        assert!(root.join("CalculatorUI/README.md").is_file());
        assert!(root.join("CalculatorUI/calculator.py").is_file());
        assert!(root.join("CalculatorUI/ui.py").is_file());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded");
        assert_eq!(
            plan.status,
            crate::session::StructuredProjectPlanStatus::Completed
        );
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .model_decisions
            .iter()
            .any(|line| line.contains("execute intent plan_creation_execution")));
        assert!(trace.runtime_checks.iter().any(|line| line
            .contains("new verified plan created during explicit plan creation execution turn")));
        let requests = provider.requests();
        assert!(requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .skip(1)
            .any(|request| joined_request_messages(request)
                .contains("call the needed file and directory tools in one assistant response")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_plan_creation_execution_can_use_create_files_batch() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-create-execute-batch",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "batch-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "NotesCLI/plan.md",
                            "contents": "# Notes Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python -m src.main`.\n- Run `pytest tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist and tests pass.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "batch-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["NotesCLI/src", "NotesCLI/tests"],
                            "files": [
                                {
                                    "target_path": "NotesCLI/README.md",
                                    "contents": "# Notes CLI\n"
                                },
                                {
                                    "target_path": "NotesCLI/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "NotesCLI/src/main.py",
                                    "contents": "def main():\n    print('notes')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "NotesCLI/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a notes project plan and execute it",
        );

        for path in [
            "NotesCLI/plan.md",
            "NotesCLI/README.md",
            "NotesCLI/requirements.txt",
            "NotesCLI/src/main.py",
            "NotesCLI/tests/test_main.py",
        ] {
            assert!(root.join(path).is_file(), "missing {path}");
        }
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(
            session.actions().len(),
            5,
            "plan plus four expected files should be applied without another provider round"
        );
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
    fn generic_execute_plan_creation_can_post_decide_to_execute_same_turn() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-post-plan-execute",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new(
                    "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
                ),
            ])
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-decision-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "PostDecision/PLAN.md",
                            "contents": "# Post Decision Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-decision-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["PostDecision/src"],
                            "files": [
                                {
                                    "target_path": "PostDecision/README.md",
                                    "contents": "# Post Decision\n"
                                },
                                {
                                    "target_path": "PostDecision/src/main.py",
                                    "contents": "print('ok')\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a project plan, then execute it",
        );

        for path in [
            "PostDecision/PLAN.md",
            "PostDecision/README.md",
            "PostDecision/src/main.py",
        ] {
            assert!(root.join(path).is_file(), "missing {path}");
        }
        let requests = provider.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
        assert!(joined_request_messages(&requests[2]).contains("A verified plan was just created"));
        assert_eq!(requests[3].mode, CapturedProviderRequestMode::Tool);
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .model_decisions
            .iter()
            .any(|line| line.contains("post-plan classifier selected plan execution"))));
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should be recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_execution_create_files_batch_does_not_reclassify_readme_as_new_plan() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-readme-planish",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(cwd.join("GemmaReadingTracker1")).unwrap();
        std::fs::write(
            cwd.join("GemmaReadingTracker1/plan.md"),
            "# Project Plan\n\n```text\nREADME.md\nrequirements.txt\nmain.py\ntracker/__init__.py\ntracker/models.py\ntracker/storage.py\ntracker/cli.py\ntests/__init__.py\ntests/test_models.py\ntests/test_storage.py\n```\n\n## Verification\n- Run pytest.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "reading-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "GemmaReadingTracker1/tracker",
                                "GemmaReadingTracker1/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "GemmaReadingTracker1/README.md",
                                    "contents": "# Project Plan\n\nThis README describes the reading tracker project plan and usage.\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/requirements.txt",
                                    "contents": "pytest\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/main.py",
                                    "contents": "from tracker.cli import main\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/__init__.py",
                                    "contents": ""
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/models.py",
                                    "contents": "class Book:\n    pass\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/storage.py",
                                    "contents": "def load_books():\n    return []\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/cli.py",
                                    "contents": "def main():\n    print('reading tracker')\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/__init__.py",
                                    "contents": ""
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/test_models.py",
                                    "contents": "def test_model_smoke():\n    assert True\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/test_storage.py",
                                    "contents": "def test_storage_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create reading tracker files".to_string()),
                    }],
                ),
            );
        let mut session = Session::new("session", &root, &cwd);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("GemmaReadingTracker1/plan.md"),
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
                path: "GemmaReadingTracker1/plan.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        for path in [
            "GemmaReadingTracker1/README.md",
            "GemmaReadingTracker1/requirements.txt",
            "GemmaReadingTracker1/main.py",
            "GemmaReadingTracker1/tracker/__init__.py",
            "GemmaReadingTracker1/tracker/models.py",
            "GemmaReadingTracker1/tracker/storage.py",
            "GemmaReadingTracker1/tracker/cli.py",
            "GemmaReadingTracker1/tests/__init__.py",
            "GemmaReadingTracker1/tests/test_models.py",
            "GemmaReadingTracker1/tests/test_storage.py",
        ] {
            assert!(cwd.join(path).is_file(), "missing {path}");
        }
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded");
        assert_eq!(
            plan.source_plan_path,
            cwd.join("GemmaReadingTracker1/plan.md")
        );
        assert_eq!(
            plan.runtime_status(),
            StructuredProjectPlanStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_execution_blocks_implementation_when_new_plan_needs_repair() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-create-exec-bad-plan",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating files before the plan.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "bad-plan-early-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README too early".to_string()),
                        },
                        RawModelToolCall {
                            id: "bad-plan-file".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/project_plan.txt",
                                "contents": "Create README.md, requirements.txt, src/main.py, and tests/test_main.py.\n"
                            }),
                            assistant_summary: Some("create incomplete plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "bad-plan-requirements".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create requirements too early".to_string()),
                        },
                    ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project, execute it, and run verification",
        );

        assert!(root.join("GreeterCLI/project_plan.txt").is_file());
        assert!(!root.join("GreeterCLI/README.md").exists());
        assert!(!root.join("GreeterCLI/requirements.txt").exists());
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("The plan needs revision before execution")
        )));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Skipped non-plan repair action"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_prompt_project_root_anchors_bare_project_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-prompt-root-anchor",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating project with bare paths.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "prompt-root-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "project_plan.md",
                                "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist and compile.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "prompt-root-files".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFiles),
                            arguments: json!({
                                "directories": ["src", "tests"],
                                "files": [
                                    {
                                        "target_path": "README.md",
                                        "contents": "# Greeter CLI\n"
                                    },
                                    {
                                        "target_path": "requirements.txt",
                                        "contents": ""
                                    },
                                    {
                                        "target_path": "src/main.py",
                                        "contents": "def main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n"
                                    },
                                    {
                                        "target_path": "tests/test_main.py",
                                        "contents": "def test_smoke():\n    assert True\n"
                                    }
                                ]
                            }),
                            assistant_summary: Some("create files".to_string()),
                        },
                    ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project inside playground/GreeterPromptRoot and execute it",
        );

        for path in [
            "playground/GreeterPromptRoot/project_plan.md",
            "playground/GreeterPromptRoot/README.md",
            "playground/GreeterPromptRoot/requirements.txt",
            "playground/GreeterPromptRoot/src/main.py",
            "playground/GreeterPromptRoot/tests/test_main.py",
        ] {
            assert!(root.join(path).is_file(), "missing {path}");
        }
        assert!(!root.join("project_plan.md").exists());
        assert!(!root.join("README.md").exists());
        assert!(!root.join("src/main.py").exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded");
        assert_eq!(plan.project_root, root.join("playground/GreeterPromptRoot"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_prompt_project_root_deduplicates_cwd_relative_prefix() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-prompt-root-dedupe-cwd",
            std::process::id()
        ));
        let cwd = root.join("playground");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&cwd).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating a plan at the requested root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "dedupe-root-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/CoreSolidNotes1/PLAN.md",
                            "contents": "# Notes Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Do not run shell commands.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
            );
        let mut session = Session::new("session", &root, &cwd);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create only a project plan. The project root must be exactly playground/CoreSolidNotes1.",
        );

        assert!(cwd.join("CoreSolidNotes1/PLAN.md").is_file());
        assert!(!cwd.join("playground/CoreSolidNotes1/PLAN.md").exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded without duplicated cwd prefix");
        assert_eq!(plan.project_root, cwd.join("CoreSolidNotes1"));
        assert_eq!(plan.source_plan_path, cwd.join("CoreSolidNotes1/PLAN.md"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_prompt_project_root_rebases_sibling_project_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-prompt-root-rebase-sibling",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan under the wrong sibling root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "sibling-root-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/GemmaBookmarkManager5/PLAN.md",
                            "contents": "# Bookmark Manager Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The bookmark manager files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
                crate::event::ProviderOutput::new("Creating files under the same wrong sibling root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "sibling-root-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "playground/GemmaBookmarkManager5/src",
                                "playground/GemmaBookmarkManager5/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/README.md",
                                    "contents": "# Bookmark Manager\n"
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/src/main.py",
                                    "contents": "def main():\n    print('bookmarks')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    }]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create a complete bookmark manager project. The project root must be exactly playground/GemmaBookmarkManagerSamePrompt5. First create a project plan, then execute it.",
        );

        for path in [
            "playground/GemmaBookmarkManagerSamePrompt5/PLAN.md",
            "playground/GemmaBookmarkManagerSamePrompt5/README.md",
            "playground/GemmaBookmarkManagerSamePrompt5/requirements.txt",
            "playground/GemmaBookmarkManagerSamePrompt5/src/main.py",
            "playground/GemmaBookmarkManagerSamePrompt5/tests/test_main.py",
        ] {
            assert!(root.join(path).is_file(), "missing {path}");
        }
        assert!(!root
            .join("playground/GemmaBookmarkManager5/PLAN.md")
            .exists());
        assert!(!root
            .join("playground/GemmaBookmarkManagerSamePrompt5/playground/GemmaBookmarkManager5/PLAN.md")
            .exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("rebased plan should be recorded");
        assert_eq!(
            plan.project_root,
            root.join("playground/GemmaBookmarkManagerSamePrompt5")
        );
        assert_eq!(
            plan.runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_execution_requires_plan_before_implementation() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-required-first",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Implementing without a plan.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "plan-required-early-shell".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "printf '# Greeter CLI\\n' > README.md",
                            "cwd": "GreeterCLI",
                            "expected_file": "README.md"
                        }),
                        assistant_summary: Some("create README too early".to_string()),
                    }]),
                crate::event::ProviderOutput::new("Creating plan and files.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-required-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/plan.md",
                                "contents": "# Greeter Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py`.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-required-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-required-main".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/src/main.py",
                                "contents": "print('hello')\n"
                            }),
                            assistant_summary: Some("create main".to_string()),
                        },
                    ],
                ),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project, first create a plan, then execute it",
        );

        assert!(root.join("GreeterCLI/plan.md").is_file());
        assert!(root.join("GreeterCLI/README.md").is_file());
        assert!(root.join("GreeterCLI/src/main.py").is_file());
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Create the project plan file first")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_execution_plain_create_without_plan_is_applied() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-adhoc-create-plan-intent",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating notes.txt.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "adhoc-create-notes".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "notes.txt",
                            "contents": "hello world\n"
                        }),
                        assistant_summary: Some("create notes".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a file notes.txt with the text hello world",
        );

        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "hello world\n"
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(&applied.result, VerifiedActionResult::FileWritten { path }
                    if path.ends_with("notes.txt"))
                    || matches!(&applied.result, VerifiedActionResult::File(
                        crate::event::FileActionVerification::FileCreated { path }
                    ) if path.ends_with("notes.txt"))
        )));
        assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Create the project plan file first"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_execution_plain_create_after_completed_plan_is_applied() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-post-plan-create",
            std::process::id()
        ));
        let project = root.join("Demo");
        let plan_path = project.join("plan.md");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(&plan_path, "# Demo Plan\n").unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating usage.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-plan-usage".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "Demo/USAGE.md",
                            "contents": "PYTHONPATH=src python -m text_tools.cli sample.txt.\n"
                        }),
                        assistant_summary: Some("create usage".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);
        session.record_structured_project_plan(crate::session::StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: crate::session::StructuredProjectPlanStatus::Completed,
            expected_directories: vec![project.clone()],
            expected_files: Vec::new(),
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create USAGE.md inside Demo containing a command line.",
        );

        assert_eq!(
            std::fs::read_to_string(project.join("USAGE.md")).unwrap(),
            "PYTHONPATH=src python -m text_tools.cli sample.txt.\n"
        );
        assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Create the project plan file first"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_only_does_not_mark_plan_executing() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-only-not-executing",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-only-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "myapp/plan.md",
                            "contents": "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("No further tool actions."),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a plan in ./myapp for a tiny script that prints hi",
        );

        assert!(root.join("myapp/plan.md").is_file());
        assert!(!root.join("myapp/script.py").exists());
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded");
        assert_eq!(
            plan.runtime_status(),
            crate::session::StructuredProjectPlanStatus::Verified
        );
        assert_ne!(
            plan.status,
            crate::session::StructuredProjectPlanStatus::Executing
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_plan_preflight_allows_unrelated_non_execution_create() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-preflight-unrelated",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let plan_provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "unrelated-plan-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "myapp/plan.md",
                            "contents": "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("No further tool actions."),
            ]);
        let mut session = Session::new("session", &root, &root);
        run_permissive_agent_turn(
            &plan_provider,
            &mut session,
            "create a plan in ./myapp for a tiny script that prints hi",
        );

        let create_provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating unrelated file.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "unrelated-create".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "other/x.txt",
                            "contents": "outside plan\n"
                        }),
                        assistant_summary: Some("create unrelated file".to_string()),
                    }],
                ),
            );

        run_permissive_agent_turn(
            &create_provider,
            &mut session,
            "create file other/x.txt outside the plan",
        );

        assert_eq!(
            std::fs::read_to_string(root.join("other/x.txt")).unwrap(),
            "outside plan\n"
        );
        assert_eq!(
            session
                .project_memory()
                .latest_structured_plan()
                .expect("plan should remain recorded")
                .runtime_status(),
            crate::session::StructuredProjectPlanStatus::Verified
        );
        assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("verified plan is rooted"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_last_block_reports_latest_preflight_block() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-last-block-state",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("myapp");
        std::fs::create_dir_all(&project).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n",
        )
        .unwrap();
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
            expected_files: vec![project.join("script.py")],
        });
        let blocked_provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan file and unrelated file.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "blocked-plan-file".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "myapp/script.py",
                                "contents": "print('hi')\n"
                            }),
                            assistant_summary: Some("create expected script".to_string()),
                        },
                        RawModelToolCall {
                            id: "blocked-outside-file".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "other/x.txt",
                                "contents": "outside plan\n"
                            }),
                            assistant_summary: Some("create outside file".to_string()),
                        },
                    ]),
            );

        run_permissive_agent_turn(
            &blocked_provider,
            &mut session,
            "execute the verified plan and create other/x.txt too",
        );

        assert!(!project.join("script.py").exists());
        assert!(!root.join("other/x.txt").exists());
        let block = session
            .latest_runtime_block()
            .expect("preflight block should be recorded")
            .message
            .clone();
        assert!(block.contains("verified plan is rooted"));
        assert!(block.contains("other/x.txt"));

        let state_provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"last_block\"}",
            ));

        run_permissive_agent_turn(
            &state_provider,
            &mut session,
            "why was the previous request blocked?",
        );

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content == block
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn chat_route_after_runtime_block_retries_state_routing() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-block-chat-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"I need more details.\"}",
            ),
            crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"Still not sure.\"}",
            ),
        ]);
        let mut session = Session::new("session", &root, &root);
        session.record_runtime_block(
            "The verified plan is rooted at myapp, but the tool call targets other/x.txt outside that project. No filesystem action was applied.",
        );

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "explain the latest runtime outcome",
        );

        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        assert!(joined_request_messages(&requests[1]).contains("runtime block/skip/failure"));
        assert!(session
            .latest_reasoning_trace()
            .is_some_and(|trace| trace.route.as_deref() == Some("state")));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content.contains("verified plan is rooted at myapp")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_runtime_block_does_not_hijack_later_chat() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-stale-block-chat",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_plain_output(
            crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"Hello!\"}"),
        );
        let mut session = Session::new("session", &root, &root);
        session.record_runtime_block("Old block message.");
        session.start_reasoning_trace("intervening turn one");
        session.start_reasoning_trace("intervening turn two");

        run_permissive_agent_turn(&provider, &mut session, "hello");

        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert!(session
            .latest_reasoning_trace()
            .is_some_and(|trace| trace.route.as_deref() == Some("chat")));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Hello!"
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content == "Old block message."
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verified_action_clears_prior_runtime_block() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-clear-block-on-action",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let action_provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "clear-block-create".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ok.txt",
                            "contents": "ok\n"
                        }),
                        assistant_summary: Some("create ok file".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);
        session.record_runtime_block("Previous block message.");

        run_permissive_agent_turn(&action_provider, &mut session, "create ok.txt");

        assert!(root.join("ok.txt").is_file());
        assert!(session.latest_runtime_block().is_none());

        let chat_provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"Hello after action.\"}",
            ));

        run_permissive_agent_turn(&chat_provider, &mut session, "hello");

        assert_eq!(chat_provider.requests().len(), 1);
        assert!(session
            .latest_reasoning_trace()
            .is_some_and(|trace| trace.route.as_deref() == Some("chat")));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "Hello after action."
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_identical_all_skipped_tool_results_stop_the_loop() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-repeated-skip-breaker",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Demo")).unwrap();
        std::fs::write(
            root.join("Demo/plan.md"),
            "# Demo Plan\n\n```text\nREADME.md\n```\n\n## Verification\n- README.md exists.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("Demo/plan.md"),
                contents: "# Demo Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "Demo/plan.md".to_string(),
            }),
        );
        let repeated_create = |id: &str| RawModelToolCall {
            id: id.to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "notes.txt",
                "contents": "hello world\n"
            }),
            assistant_summary: Some("create notes".to_string()),
        };
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Trying the create.")
                    .with_tool_calls(vec![repeated_create("repeat-skip-1")]),
                crate::event::ProviderOutput::new("Trying the create again.")
                    .with_tool_calls(vec![repeated_create("repeat-skip-2")]),
                crate::event::ProviderOutput::new("Created notes.txt."),
            ]);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a file notes.txt with the text hello world",
        );

        assert!(!root.join("notes.txt").exists());
        assert!(provider.requests().len() <= REPEATED_IDENTICAL_SKIP_BREAKER_LIMIT + 1);
        assert_eq!(
            session
                .events()
                .iter()
                .filter(|event| matches!(
                    event,
                    Event::AssistantMessage(message)
                        if message.source == AssistantMessageSource::Controller
                            && message.content.contains("repeated the same blocked tool result")
                ))
                .count(),
            1
        );
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Created notes.txt")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn execute_prose_without_verified_action_is_not_reported_as_success() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-false-success-guard",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Demo")).unwrap();
        std::fs::write(
            root.join("Demo/plan.md"),
            "# Demo Plan\n\n```text\nREADME.md\n```\n\n## Verification\n- README.md exists.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("Demo/plan.md"),
                contents: "# Demo Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "Demo/plan.md".to_string(),
            }),
        );
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Trying the create.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "false-success-skipped".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "notes.txt",
                            "contents": "hello world\n"
                        }),
                        assistant_summary: Some("create notes".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Created notes.txt with hello world."),
            ]);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a file notes.txt with the text hello world",
        );

        assert!(!root.join("notes.txt").exists());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Created notes.txt")
        )));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("No verified filesystem change occurred")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn executable_command_shape_skips_existing_file_tools_and_accepts_shell_command() {
        assert!(input_contains_executable_command_shape("cat notes.txt"));
        assert!(input_contains_executable_command_shape(
            "PYTHONPATH=src python -m text_tools.cli sample.txt"
        ));
        assert!(!input_contains_executable_command_shape(
            "Create USAGE.md containing PYTHONPATH=src python -m text_tools.cli sample.txt."
        ));

        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-intent-guard",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("notes.txt"), "hello world\n").unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Rewriting notes.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "shell-intent-wrong-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "notes.txt",
                            "contents": "changed\n"
                        }),
                        assistant_summary: Some("rewrite notes".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Running cat.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "shell-intent-cat".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "cat notes.txt",
                            "cwd": ".",
                            "expected_effect": "hello world",
                            "expected_file": "notes.txt"
                        }),
                        assistant_summary: Some("cat notes".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "cat notes.txt");

        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "hello world\n"
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(&applied.result, VerifiedActionResult::Shell(shell)
                    if shell.exit_code == Some(0)
                        && shell.stdout.contains("hello world"))
        )));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Tool `create_file` is not available"))));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("ignored shell expected paths"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_execution_intent_exposes_only_shell_safe_tools_before_model_call() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-shell-scoped-tools",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("USAGE.md"), "usage\n").unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating usage.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "shell-scoped-wrong-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "USAGE.md",
                            "contents": "changed\n"
                        }),
                        assistant_summary: Some("rewrite usage".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Running cat.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "shell-scoped-cat".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "cat USAGE.md",
                            "cwd": ".",
                            "expected_effect": "usage",
                            "expected_file": "USAGE.md"
                        }),
                        assistant_summary: Some("cat usage".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "run cat USAGE.md");

        let tool_requests = provider
            .requests()
            .into_iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .collect::<Vec<_>>();
        assert!(tool_requests.len() >= 2);
        for tool_request in &tool_requests {
            assert_eq!(
                tool_request.tool_names,
                vec!["ask_guidance".to_string(), "shell_command".to_string()]
            );
            assert_eq!(tool_request.tool_count, 2);
            assert!(!tool_request
                .tool_names
                .iter()
                .any(|name| name == "create_file" || name == "create_files"));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("USAGE.md")).unwrap(),
            "usage\n"
        );
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Tool `create_file` is not available"))));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("ignored shell expected paths"))));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(&applied.result, VerifiedActionResult::Shell(shell)
                    if shell.stdout.contains("usage"))
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_execution_intent_exposes_plan_safe_tools_before_model_call() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-scoped-tools",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("PlanScoped");
        std::fs::create_dir_all(&project).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-scoped-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["PlanScoped/src"],
                            "files": [
                                {
                                    "target_path": "PlanScoped/README.md",
                                    "contents": "# Plan Scoped\n"
                                },
                                {
                                    "target_path": "PlanScoped/src/main.py",
                                    "contents": "print('ok')\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
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
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("README.md"), project.join("src/main.py")],
        });

        run_permissive_agent_turn(&provider, &mut session, "execute the verified plan");

        let tool_request = only_tool_request(&provider);
        assert_eq!(
            tool_request.tool_names,
            vec![
                "ask_guidance".to_string(),
                "create_files".to_string(),
                "create_file".to_string(),
                "create_directory".to_string(),
                "overwrite_file".to_string(),
                "patch_file".to_string(),
                "shell_command".to_string(),
            ]
        );
        assert_eq!(tool_request.tool_count, 7);
        assert!(!tool_request
            .tool_names
            .iter()
            .any(|name| name == "delete_file" || name == "move_file"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_tool_command_still_exposes_full_tool_set() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-explicit-full-tools",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new().with_tool_output(
            crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "explicit-full-tools-file".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "hello\n"
                    }),
                    assistant_summary: Some("create file".to_string()),
                },
            ]),
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, "/tool create notes.txt");

        let tool_request = only_tool_request(&provider);
        assert_eq!(tool_request.tool_count, 9);
        assert!(tool_request
            .tool_names
            .iter()
            .any(|name| name == "delete_file"));
        assert!(tool_request
            .tool_names
            .iter()
            .any(|name| name == "move_file"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn execute_no_tool_text_response_retries_without_rendering_fake_claim() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-execute-no-tool-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new(
                    "Plan created for playground/FakePlan. Files added: README.md, src/main.py.",
                ),
                crate::event::ProviderOutput::new("Creating the plan file.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "no-tool-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/FakePlan/plan.md",
                            "contents": "# Fake Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan inside playground/FakePlan",
        );

        assert!(root.join("playground/FakePlan/plan.md").is_file());
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Files added")
        )));
        let requests = provider.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
                .count(),
            2
        );
        assert!(joined_request_messages(
            requests
                .iter()
                .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
                .nth(1)
                .expect("second tool request should exist")
        )
        .contains("This route requires tool actions"));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("execute route returned no tool calls"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn execute_empty_tool_response_does_not_plain_fallback_to_fake_completion() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-execute-empty-no-fallback",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProviderWithToolErrors::new(
            vec![
                crate::event::ProviderOutput::new(
                    "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
                ),
                crate::event::ProviderOutput::new(
                    "Created project files and ran verification successfully.",
                ),
            ],
            vec![CapturedToolStep::EmptyResponse],
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project, execute it, and run verification",
        );

        assert!(!root.join("GreeterCLI").exists());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("Created project files")
        )));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("did not return any tool actions")
        )));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(
                |line| line.contains("empty tool response on execute route; requested tool repair")
            )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_empty_tool_response_retries_and_creates_plan() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-empty-tool-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProviderWithToolErrors::new(
            vec![crate::event::ProviderOutput::new("{\"route\":\"execute\"}")],
            vec![
                CapturedToolStep::EmptyResponse,
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating the plan file.").with_tool_calls(
                        vec![RawModelToolCall {
                            id: "empty-retry-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "playground/RetryPlan/plan.md",
                                "contents": "# Retry Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        }],
                    ),
                ),
            ],
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan inside playground/RetryPlan",
        );

        assert!(root.join("playground/RetryPlan/plan.md").is_file());
        let requests = provider.requests();
        assert!(requests.len() >= 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        assert!(requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .nth(1)
            .is_some_and(|request| joined_request_messages(request)
                .contains("This route requires tool actions")));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(
                |line| line.contains("empty tool response on execute route; requested tool repair")
            )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bare_plan_artifact_is_anchored_to_batch_project_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-bare-plan-batch-root",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "bare-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "PLAN.md",
                            "contents": "# Greeter Plan\n\n## File Tree\n```text\nplayground/GreeterCLI/\n├── README.md\n├── requirements.txt\n├── src/\n│   └── main.py\n└── tests/\n    └── test_main.py\n```\n\n## Verification\n- Run `python -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter runs and tests pass.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "bare-plan-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "tests",
                                "playground/GreeterCLI/src",
                                "playground/GreeterCLI/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "playground/GreeterCLI/README.md",
                                    "contents": "# Greeter CLI\n"
                                },
                                {
                                    "target_path": "playground/GreeterCLI/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "playground/GreeterCLI/src/main.py",
                                    "contents": "def main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "playground/GreeterCLI/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project in playground and execute it",
        );

        assert!(!root.join("PLAN.md").exists());
        assert!(root.join("playground/GreeterCLI/PLAN.md").is_file());
        for path in [
            "playground/GreeterCLI/README.md",
            "playground/GreeterCLI/requirements.txt",
            "playground/GreeterCLI/src/main.py",
            "playground/GreeterCLI/tests/test_main.py",
        ] {
            assert!(root.join(path).is_file(), "missing {path}");
        }
        let plan = session
            .project_memory()
            .latest_structured_plan()
            .expect("anchored plan should be recorded");
        assert_eq!(plan.project_root, root.join("playground/GreeterCLI"));
        assert_eq!(
            plan.runtime_status(),
            crate::session::StructuredProjectPlanStatus::Completed
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_execution_can_run_shell_verification_after_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-shell-verify",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("GreeterCLI");
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan, files, and verifying.")
                    .with_tool_calls(vec![
                    RawModelToolCall {
                        id: "verify-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/plan.md",
                            "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter files exist and compile.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                        RawModelToolCall {
                            id: "verify-files".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFiles),
                            arguments: json!({
                                "directories": ["GreeterCLI/src", "GreeterCLI/tests"],
                                "files": [
                                    {
                                        "target_path": "GreeterCLI/README.md",
                                        "contents": "# Greeter CLI\n"
                                    },
                                    {
                                        "target_path": "GreeterCLI/requirements.txt",
                                        "contents": ""
                                    },
                                    {
                                        "target_path": "GreeterCLI/src/main.py",
                                        "contents": "def greeting(name='World'):\n    return f'Hello, {name}!'\n\nif __name__ == '__main__':\n    print(greeting())\n"
                                    },
                                    {
                                        "target_path": "GreeterCLI/tests/test_main.py",
                                        "contents": "import sys\nfrom pathlib import Path\nsys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))\nfrom main import greeting\n\ndef test_greeting():\n    assert greeting('Alice') == 'Hello, Alice!'\n"
                                    }
                                ]
                            }),
                            assistant_summary: Some("create files".to_string()),
                        },
                        RawModelToolCall {
                            id: "verify-shell".to_string(),
                            name: RawModelToolName::Known(ModelToolName::ShellCommand),
                            arguments: json!({
                                "command": "python3 -m py_compile src/main.py tests/test_main.py",
                                "cwd": project.display().to_string(),
                                "expected_effect": "Python files compile"
                            }),
                            assistant_summary: Some("compile Python files".to_string()),
                        },
                    ]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project, execute it, and run verification",
        );

        assert!(project.join("plan.md").is_file());
        assert!(project.join("src/main.py").is_file());
        assert!(project.join("tests/test_main.py").is_file());
        assert!(session.actions().iter().any(|record| {
            matches!(
                record.verified_result.as_ref(),
                Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(0)
            )
        }));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| !trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Skipped shell command during verified plan execution"))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_execution_stops_after_no_progress_off_plan_tool_round() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-no-progress",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "no-progress-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/plan.md",
                            "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter files exist and compile.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating part of the project.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "no-progress-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                    ]),
                crate::event::ProviderOutput::new("Trying wrong paths and verification.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "no-progress-wrong-test".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "Greeter/wrong/test_main.py",
                                "contents": "def test_smoke():\n    assert True\n"
                            }),
                            assistant_summary: Some("create wrong test".to_string()),
                        },
                        RawModelToolCall {
                            id: "no-progress-wrong-requirements".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "Greeter/wrong/requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create wrong requirements".to_string()),
                        },
                        RawModelToolCall {
                            id: "no-progress-shell".to_string(),
                            name: RawModelToolName::Known(ModelToolName::ShellCommand),
                            arguments: json!({
                                "command": "python3 -m py_compile src/main.py tests/test_main.py",
                                "cwd": root.join("GreeterCLI").display().to_string(),
                                "expected_effect": "Python files compile"
                            }),
                            assistant_summary: Some("compile Python files".to_string()),
                        },
                    ]),
                crate::event::ProviderOutput::new("This request should not be reached.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "no-progress-late-test".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/tests/test_main.py",
                            "contents": "def test_late():\n    assert True\n"
                        }),
                        assistant_summary: Some("late test".to_string()),
                    }]),
            ]);
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a greeter project, execute it, and run verification",
        );

        assert!(root.join("GreeterCLI/plan.md").is_file());
        assert!(root.join("GreeterCLI/README.md").is_file());
        assert!(!root.join("GreeterCLI/src/main.py").exists());
        assert!(!root.join("GreeterCLI/tests/test_main.py").exists());
        assert!(!root.join("GreeterCLI/requirements.txt").exists());
        assert!(!root.join("Greeter/wrong/test_main.py").exists());
        assert_eq!(
            provider
                .requests()
                .iter()
                .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
                .count(),
            3,
            "the no-progress skipped/off-plan round should stop the loop"
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("Stopped because the last tool response")
                    && message.content.contains("GreeterCLI/tests/test_main.py")
                    && message.content.contains("GreeterCLI/requirements.txt")
        )));
        assert!(session
            .latest_reasoning_trace()
            .is_some_and(|trace| trace.runtime_checks.iter().any(
                |line| line.contains("plan execution made no progress; stopped provider loop")
            )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_execution_stops_after_partial_create_files_batch() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-partial-batch",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Journal")).unwrap();
        std::fs::write(
            root.join("Journal/plan.md"),
            "# Journal Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project scaffold is complete.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new().with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Creating most files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "partial-batch".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFiles),
                    arguments: json!({
                        "directories": ["Journal/src", "Journal/tests"],
                        "files": [
                            {
                                "target_path": "Journal/README.md",
                                "contents": "# Journal\n"
                            },
                            {
                                "target_path": "Journal/requirements.txt",
                                "contents": ""
                            },
                            {
                                "target_path": "Journal/src/main.py",
                                "contents": "print('journal')\n"
                            }
                        ]
                    }),
                    assistant_summary: Some("create most expected files".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("This repair request should not be reached.")
                .with_tool_calls(vec![RawModelToolCall {
                    id: "partial-late-test".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "Journal/tests/test_main.py",
                        "contents": "def test_late():\n    assert True\n"
                    }),
                    assistant_summary: Some("late test".to_string()),
                }]),
        ]);
        let mut session = Session::new("session", &root, &root);
        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("Journal/plan.md"),
                contents: "# Journal Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        record_verified_project_memory(
            &mut session,
            &plan_action,
            &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "Journal/plan.md".to_string(),
            }),
        );

        run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "execute the verified plan",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("Journal/README.md").is_file());
        assert!(root.join("Journal/requirements.txt").is_file());
        assert!(root.join("Journal/src/main.py").is_file());
        assert!(!root.join("Journal/tests/test_main.py").exists());
        assert_eq!(
            provider
                .requests()
                .iter()
                .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
                .count(),
            1,
            "partial create_files batch should not trigger an open-ended repair request"
        );
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("No further model repair request was sent")
                    && message.content.contains("Journal/tests/test_main.py")
        )));
        assert!(session
            .latest_runtime_block()
            .is_some_and(|block| block.message.contains("Journal/tests/test_main.py")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_execution_empty_tool_response_continues_without_plain_fallback() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-exec-empty-tool-no-fallback",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProviderWithToolErrors::new(
            vec![crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            )],
            vec![
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(
                        vec![RawModelToolCall {
                            id: "empty-response-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "NotesCLI/PROJECT_PLAN.md",
                                "contents": "# Notes Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- `README.md` and `src/main.py` exist.\n\n## Acceptance Criteria\n- Running `python -m src.main` prints a notes CLI help message.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        }],
                    ),
                ),
                CapturedToolStep::EmptyResponse,
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating all missing files.")
                        .with_tool_calls(vec![
                            RawModelToolCall {
                                id: "empty-response-readme".to_string(),
                                name: RawModelToolName::Known(ModelToolName::CreateFile),
                                arguments: json!({
                                    "target_path": "NotesCLI/README.md",
                                    "contents": "# Notes CLI\n"
                                }),
                                assistant_summary: Some("create README".to_string()),
                            },
                            RawModelToolCall {
                                id: "empty-response-main".to_string(),
                                name: RawModelToolName::Known(ModelToolName::CreateFile),
                                arguments: json!({
                                    "target_path": "NotesCLI/src/main.py",
                                    "contents": "def main():\n    print('notes')\n\nif __name__ == '__main__':\n    main()\n"
                                }),
                                assistant_summary: Some("create main".to_string()),
                            },
                        ]),
                ),
            ],
        );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a notes plan and execute it",
        );

        assert!(root.join("NotesCLI/PROJECT_PLAN.md").is_file());
        assert!(root.join("NotesCLI/README.md").is_file());
        assert!(root.join("NotesCLI/src/main.py").is_file());
        let requests = provider.requests();
        assert_eq!(
            requests.len(),
            4,
            "plain route plus three tool attempts; no plain fallback request"
        );
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert!(requests[1..]
            .iter()
            .all(|request| request.mode == CapturedProviderRequestMode::Tool));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line.contains(
                "empty tool response during plan execution; continued from verified missing paths"
            ))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_creation_with_skipped_files_waits_for_followup_without_post_decision() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-skipped-files-no-post-decision",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-plan-decision-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/plan.md",
                            "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                    RawModelToolCall {
                        id: "post-plan-decision-readme-too-early".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/README.md",
                            "contents": "# Calculator UI\n"
                        }),
                        assistant_summary: Some("create README too early".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create a plan for the calculator UI and execute it",
        );

        assert!(root.join("CalculatorUI/plan.md").is_file());
        assert!(!root.join("CalculatorUI/README.md").exists());
        assert!(!root.join("CalculatorUI/calculator.py").exists());
        assert!(!root.join("CalculatorUI/ui.py").exists());
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(
                |line| line.contains("plan creation completed; skipped final provider synthesis")
            )));
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_only_route_post_plan_decision_can_keep_plan_only_boundary() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-plan-only-no-post-decision",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new("{\"route\":\"state\",\"answer_kind\":\"plan\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan only.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-only-route-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "TodoPlan/PLAN.md",
                            "contents": "# Todo Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Expected files exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "create only a project plan inside TodoPlan",
        );

        assert!(root.join("TodoPlan/PLAN.md").is_file());
        assert!(!root.join("TodoPlan/README.md").exists());
        assert!(!root.join("TodoPlan/src/main.py").exists());
        assert!(!root.join("TodoPlan/requirements.txt").exists());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
        assert!(joined_request_messages(&requests[2]).contains("A verified plan was just created"));
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(
                |line| line.contains("plan creation completed; skipped final provider synthesis")
            )));

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
    fn generic_state_status_with_incomplete_plan_retries_and_can_execute() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-plan-retry",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("RetryPlan");
        std::fs::create_dir_all(&project).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Retry Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    "{\"route\":\"state\",\"answer_kind\":\"status\"}",
                ),
                crate::event::ProviderOutput::new(
                    "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "state-retry-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["RetryPlan/src"],
                            "files": [
                                {
                                    "target_path": "RetryPlan/README.md",
                                    "contents": "# Retry Plan\n"
                                },
                                {
                                    "target_path": "RetryPlan/src/main.py",
                                    "contents": "print('retry')\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
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
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("README.md"), project.join("src/main.py")],
        });

        run_permissive_agent_turn(&provider, &mut session, "execute the plan!");

        assert!(project.join("README.md").is_file());
        assert!(project.join("src/main.py").is_file());
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        assert!(
            joined_request_messages(&requests[1]).contains("incomplete verified plan is available")
        );
        assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
        assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .model_decisions
            .iter()
            .any(|line| line.contains(
                "state route selected generic status with an incomplete verified plan"
            ))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn existing_verified_plan_execution_is_not_blocked_by_plan_creation_intent() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-existing-plan-creation-intent",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("TodoPlan");
        std::fs::create_dir_all(&project).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Todo Plan\n\n```text\nTodoPlan/\n├── README.md\n├── src/main.py\n└── requirements.txt\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n",
        )
        .unwrap();
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "existing-plan-readme".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "README.md",
                            "contents": "# Todo Plan\n"
                        }),
                        assistant_summary: Some("create README".to_string()),
                    },
                    RawModelToolCall {
                        id: "existing-plan-main".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "src/main.py",
                            "contents": "def main():\n    print('todo')\n\nif __name__ == '__main__':\n    main()\n"
                        }),
                        assistant_summary: Some("create main".to_string()),
                    },
                    RawModelToolCall {
                        id: "existing-plan-reqs".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "requirements.txt",
                            "contents": ""
                        }),
                        assistant_summary: Some("create requirements".to_string()),
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
                project.join("src/main.py"),
                project.join("requirements.txt"),
            ],
        });

        run_permissive_agent_turn(&provider, &mut session, "please execute the plan");

        assert!(project.join("README.md").is_file());
        assert!(project.join("src/main.py").is_file());
        assert!(project.join("requirements.txt").is_file());
        let trace = session
            .latest_reasoning_trace()
            .expect("reasoning trace should exist");
        assert!(!trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("Create the project plan file first")));
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
            .contains("local file/artifact/plan work"));
        assert!(requests[0].messages[0]
            .content
            .contains("Return compact JSON"));
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
                if message.source == AssistantMessageSource::VerifiedState
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
    fn model_selected_state_route_can_report_recent_changes() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-recent-changes",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider =
            CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"state\",\"answer_kind\":\"recent_changes\"}",
            ));
        let mut session = Session::new("session", &root, &root);
        session.start_reasoning_trace("create a file");
        let action = Action::proposed_create_file("action-file", "latest.txt", "hi\n", "create")
            .approve()
            .mark_applied();
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "latest.txt".to_string(),
            });
        let mut record = ActionRecord::new(action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &action, &result);

        run_permissive_agent_turn(&provider, &mut session, "what changed in the last action?");

        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content.contains("created latest.txt")
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn state_route_without_kind_uses_secondary_classifier_call() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-state-kind-classifier",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // Plain route picks `state` with no kind; the secondary classifier then
        // resolves the precise view.
        let provider = CapturingProvider::new().with_plain_outputs(vec![
            crate::event::ProviderOutput::new("{\"route\":\"state\"}"),
            crate::event::ProviderOutput::new("{\"answer_kind\":\"recent_changes\"}"),
        ]);
        let mut session = Session::new("session", &root, &root);

        // One prior action-producing turn so recent_changes has content.
        session.start_reasoning_trace("create the config");
        let action = Action::proposed_create_file("a1", "next.config.js", "", "create")
            .approve()
            .mark_applied();
        let mut record = ActionRecord::new(action);
        record.verified_result = Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileCreated {
                path: "next.config.js".to_string(),
            },
        ));
        session.push_action(record);

        run_permissive_agent_turn(&provider, &mut session, "what did you just do?");

        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        // The answer-kind menu is only sent on the secondary classifier call,
        // never on the always-sent route prompt.
        assert!(!joined_request_messages(&requests[0]).contains("Valid answer kinds"));
        assert!(joined_request_messages(&requests[1]).contains("Valid answer kinds"));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("plain_state_classifier")
        )));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content.contains("next.config.js")
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
                if message.source == AssistantMessageSource::VerifiedState
                    && message.content
                        == "current session:\n- directory demo\n- file demo/requirements.txt"
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
    fn completed_plan_execution_intent_does_not_skip_local_shell_work() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-completed-plan-shell-work",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("DonePlanShell");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let plan_path = project.join("plan.md");
        std::fs::write(
            &plan_path,
            "# Done Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected paths are present.\n",
        )
        .unwrap();
        std::fs::write(project.join("README.md"), "# Done\n").unwrap();
        std::fs::write(project.join("src/main.py"), "def main():\n    pass\n").unwrap();
        let expected_file = project.join("shell.out");
        let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Running requested command.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "completed-plan-shell".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "printf ok > shell.out",
                            "cwd": project.display().to_string(),
                            "expected_file": expected_file.display().to_string()
                        }),
                        assistant_summary: Some("run shell command".to_string()),
                    }],
                ),
            );
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
            expected_directories: vec![root.join("DonePlanShell/src")],
            expected_files: vec![
                root.join("DonePlanShell/README.md"),
                root.join("DonePlanShell/src/main.py"),
            ],
        });

        run_permissive_agent_turn(
            &provider,
            &mut session,
            "run PYTHONPATH=src python -m done.cli sample.txt inside that project",
        );

        assert_eq!(std::fs::read_to_string(expected_file).unwrap(), "ok");
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
        assert!(session.latest_reasoning_trace().is_some_and(|trace| !trace
            .runtime_checks
            .iter()
            .any(|line| line.contains("already complete; skipped tool loop"))));

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
