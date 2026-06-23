use std::collections::HashSet;

use serde_json::json;

use crate::{
    action::{ActionLifecycleState, ActionRequest},
    agent_display_intent::should_stop_after_verified_display_only_shell,
    agent_plain_decision::handle_plain_agent_decision,
    agent_plain_provider::post_plan_creation_decision_requests_execution,
    agent_plan_creation::{
        guard_plan_creation_tool_outputs, is_latest_verified_plan_file_action,
        latest_plan_contract_needs_repair, no_tool_action_repair_message,
        plan_creation_needs_revision_notice, plan_creation_non_plan_repair_skip_message,
        plan_creation_repair_message, plan_creation_root_for_action,
        plan_execution_blocked_by_contract_repair_message, prioritize_plan_creation_tool_outputs,
        resolved_outputs_touch_structured_plan,
    },
    agent_plan_execution::{
        guard_plan_execution_tool_outputs, missing_expected_plan_path_counts,
        missing_expected_plan_paths_message, plan_execution_incomplete_after_partial_batch_message,
        plan_execution_no_progress_message, plan_execution_repair_message_or_mark_complete,
        preflight_verified_plan_tool_outputs, resolved_outputs_complete_missing_plan_paths,
        should_preflight_verified_plan_tool_outputs,
    },
    agent_policy_flow::{apply_agent_action_with_policy, review_required_action_to_propose},
    agent_prompt_context::{
        agent_local_runtime_context, agent_recent_conversation_context,
        agent_route_location_context, agent_verified_memory_context,
    },
    agent_prompts::{AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT, AGENT_SYSTEM_PROMPT},
    agent_provider_events::push_provider_finished,
    agent_provider_trace::record_provider_planning_trace,
    agent_request_mode::{provider_request_metadata_for_mode, AgentProviderRequestMode},
    agent_shell_execution::guard_shell_execution_tool_outputs,
    agent_shell_inspection::{
        guard_shell_inspection_tool_outputs, shell_command_is_direct_file_read,
        shell_command_is_project_listing,
    },
    agent_shell_turn::ShellTransaction,
    agent_synthesis::{
        request_explicit_shell_tool_result_synthesis,
        request_shell_transaction_tool_result_synthesis, verified_shell_result_digest,
    },
    agent_tool_anchors::{
        anchor_bare_plan_artifacts_to_batch_project_root, anchor_prompt_project_root_tool_outputs,
        anchor_verified_plan_tool_outputs,
    },
    agent_tool_feedback::{
        append_tool_feedback_message, explicit_tool_command_input,
        explicit_tool_command_instruction, explicit_tool_completed_shell_feedback,
        explicit_tool_read_only_stall_limit_message, explicit_tool_repeated_inspection_feedback,
        is_read_only_shell_action, record_validated_tool_output_trace,
        repeated_shell_command_feedback, shell_command_signature,
    },
    agent_tool_output::{
        all_skipped_tool_result_signature, repeated_identical_skip_breaker_message,
        resolve_agent_tool_outputs, resolved_outputs_tool_call_ids, AllSkippedToolResultSignature,
        ResolvedAgentToolOutput,
    },
    agent_tool_scope::{tool_definitions_for_intent, validate_tool_calls_in_scope},
    agent_tool_validation::{tool_validation_recovery, ToolValidationRecovery},
    agent_turn_router::{
        explicit_project_root_from_input, input_contains_local_work_syntax, AgentExecutionIntent,
        PlainAgentChatOutcome,
    },
    agent_verified_folder::{
        anchor_verified_folder_tool_outputs, guard_redundant_directory_tool_outputs,
    },
    agent_visibility::{
        chat_assistant_tool_call_message, push_provider_message_after_tool_turn_if_visible,
        push_provider_message_if_visible,
    },
    controller::TurnResult,
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderStarted, UserMessage,
        VerifiedActionResult,
    },
    model_runtime::validate_model_tool_outputs,
    path_resolution::AgentPathResolution,
    policy::PermissionPolicyMode,
    provider::{ChatMessage, ChatRole, ControllerProvider, ProviderErrorKind},
    router::Route,
    session::{PendingActionSelection, Session, StructuredProjectPlanStatus},
};

const MAX_AGENT_TOOL_ROUNDS: usize = 16;
const REPEATED_IDENTICAL_SKIP_BREAKER_LIMIT: usize = 2;
const EXPLICIT_TOOL_READ_ONLY_STALL_LIMIT: usize = 3;

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
        AgentExecutionIntent {
            explicit_tool_command: true,
            ..AgentExecutionIntent::default()
        },
    )
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
    if intent.explicit_tool_command {
        messages.push(ChatMessage::system(explicit_tool_command_instruction()));
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
    let mut completed_shell_command_signatures_this_turn = HashSet::new();
    let mut explicit_tool_edit_guidance_active = false;
    let mut explicit_tool_read_only_stall_count = 0usize;
    let mut shell_transaction =
        ShellTransaction::new(intent.shell_execution, intent.explicit_tool_command);
    for _round in 0..MAX_AGENT_TOOL_ROUNDS {
        let request =
            provider_request_metadata_for_mode(provider, AgentProviderRequestMode::ToolEnabled);
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
        let resolved_outputs = guard_shell_inspection_tool_outputs(session, resolved_outputs);
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
        let mut stop_tool_loop_after_round = false;
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
                    let shell_signature = shell_command_signature(&action.request);
                    if intent.explicit_tool_command
                        && explicit_tool_edit_guidance_active
                        && is_read_only_shell_action(&action.request)
                    {
                        tool_results_need_provider_followup = true;
                        explicit_tool_read_only_stall_count += 1;
                        let message = explicit_tool_repeated_inspection_feedback();
                        session.push_reasoning_runtime_check(message.clone());
                        session.record_runtime_block(message.clone());
                        append_tool_feedback_message(&mut messages, action.tool_call_id, message);
                        if explicit_tool_read_only_stall_count
                            >= EXPLICIT_TOOL_READ_ONLY_STALL_LIMIT
                        {
                            let message = explicit_tool_read_only_stall_limit_message();
                            session.push_reasoning_runtime_check(message.clone());
                            session.record_runtime_block(message.clone());
                            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                                message,
                                AssistantMessageSource::Controller,
                            )));
                            stop_tool_loop_after_round = true;
                        }
                        continue;
                    }
                    if shell_signature.as_ref().is_some_and(|signature| {
                        completed_shell_command_signatures_this_turn.contains(signature)
                    }) {
                        tool_results_need_provider_followup = true;
                        if !intent.explicit_tool_command {
                            stop_tool_loop_after_round = true;
                        }
                        explicit_tool_edit_guidance_active |= intent.explicit_tool_command;
                        if intent.explicit_tool_command {
                            explicit_tool_read_only_stall_count += 1;
                        }
                        let message = repeated_shell_command_feedback(intent.explicit_tool_command);
                        session.push_reasoning_runtime_check(message.clone());
                        session.record_runtime_block(message.clone());
                        append_tool_feedback_message(&mut messages, action.tool_call_id, message);
                        if explicit_tool_read_only_stall_count
                            >= EXPLICIT_TOOL_READ_ONLY_STALL_LIMIT
                        {
                            let message = explicit_tool_read_only_stall_limit_message();
                            session.push_reasoning_runtime_check(message.clone());
                            session.record_runtime_block(message.clone());
                            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                                message,
                                AssistantMessageSource::Controller,
                            )));
                            stop_tool_loop_after_round = true;
                        }
                        continue;
                    }
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
                    let is_shell_action = matches!(action.request, ActionRequest::ShellCommand(_));
                    let is_read_only_shell_action = is_read_only_shell_action(&action.request);
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
                        if !is_shell_action {
                            explicit_tool_edit_guidance_active = false;
                            explicit_tool_read_only_stall_count = 0;
                        }
                        if let Some(signature) = shell_signature {
                            if session
                                .actions()
                                .last()
                                .and_then(|record| record.verified_result.as_ref())
                                .is_some_and(|result| {
                                    matches!(result, VerifiedActionResult::Shell(_))
                                })
                            {
                                completed_shell_command_signatures_this_turn.insert(signature);
                            }
                        }
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
                    let latest_verified_shell = session
                        .actions()
                        .last()
                        .and_then(|record| record.verified_result.as_ref())
                        .and_then(|result| match result {
                            VerifiedActionResult::Shell(shell) => Some(shell.clone()),
                            _ => None,
                        });
                    let verified_shell_digest = latest_verified_shell
                        .as_ref()
                        .map(verified_shell_result_digest);
                    let model_feedback = if intent.explicit_tool_command && is_shell_action {
                        explicit_tool_completed_shell_feedback(result)
                    } else if let Some(digest) = verified_shell_digest.clone() {
                        digest
                    } else {
                        result
                    };
                    append_tool_feedback_message(
                        &mut messages,
                        action.tool_call_id,
                        model_feedback,
                    );
                    if let Some(shell) = latest_verified_shell {
                        if verified_shell_digest.is_some()
                            && !intent.explicit_tool_command
                            && should_stop_after_verified_display_only_shell(
                                input,
                                shell_command_is_project_listing(&shell.command),
                                shell_command_is_direct_file_read(&shell.command),
                            )
                        {
                            session.push_reasoning_runtime_check(
                                "verified display-only shell result rendered; skipped final provider synthesis",
                            );
                            stop_tool_loop_after_round = true;
                        } else {
                            shell_transaction.observe_verified_shell(&shell);
                        }
                        if verified_shell_digest.is_some()
                            && shell_transaction.should_synthesize_now()
                        {
                            session.push_reasoning_runtime_check(
                                "report-only shell result verified; switching to no-tool synthesis",
                            );
                            request_shell_transaction_tool_result_synthesis(
                                provider,
                                session,
                                input,
                                &shell,
                                shell_command_is_direct_file_read(&shell.command),
                            );
                            stop_tool_loop_after_round = true;
                        }
                    }
                    if intent.explicit_tool_command
                        && is_shell_action
                        && !is_read_only_shell_action
                        && matches!(
                            session.pending_action_selection(),
                            PendingActionSelection::None
                        )
                    {
                        request_explicit_shell_tool_result_synthesis(provider, session, &messages);
                        stop_tool_loop_after_round = true;
                    }
                    if !matches!(
                        session.pending_action_selection(),
                        PendingActionSelection::None
                    ) {
                        return finish_agent_turn_result(session, start_index, Route::AskModel);
                    }
                }
            }
        }
        if stop_tool_loop_after_round {
            break;
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

fn run_plain_agent_chat<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> PlainAgentChatOutcome
where
    P: ControllerProvider,
{
    if input_contains_local_work_syntax(input) {
        session.record_reasoning_route("execute");
        session.push_reasoning_runtime_check(
            "local-work-shaped input routed directly to execute without route classifier",
        );
        return PlainAgentChatOutcome::Execute(AgentExecutionIntent {
            shell_execution: true,
            ..AgentExecutionIntent::default()
        });
    }

    let request = provider_request_metadata_for_mode(provider, AgentProviderRequestMode::PlainChat);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_chat", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            handle_plain_agent_decision(provider, session, input, assistant_text, true, true)
        }
        Err(error) => {
            if input_contains_local_work_syntax(input) {
                session.record_reasoning_route("execute");
                session.push_reasoning_runtime_check(format!(
                    "{} provider route request {} failed for local-work-shaped input: {error}; routed to execute",
                    request.provider, request.request_id
                ));
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
            }
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
            PlainAgentChatOutcome::Finished
        }
    }
}

#[cfg(test)]
mod tests;
