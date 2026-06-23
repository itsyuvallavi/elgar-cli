use crate::{
    agent_plain_provider::{classify_verified_state_answer_kind, request_chat_response},
    agent_prompt_context::{agent_route_location_context, agent_verified_memory_context},
    agent_prompts::{
        AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT, AGENT_ROUTE_JSON_REPAIR_PROMPT,
        AGENT_ROUTE_LOCAL_WORK_CHAT_REPAIR_PROMPT, AGENT_ROUTE_RUNTIME_BLOCK_CHAT_REPAIR_PROMPT,
        AGENT_ROUTE_STATE_WITH_PLAN_REPAIR_PROMPT,
    },
    agent_provider_events::push_provider_finished,
    agent_request_mode::{provider_request_metadata_for_mode, AgentProviderRequestMode},
    agent_turn_router::{
        classifier_chat_content_is_bad, has_verified_session_state,
        input_contains_executable_command_shape, input_has_run_prefixed_command_shape,
        latest_structured_plan_has_missing_paths, looks_like_local_work_chat_misroute,
        looks_like_misrouted_artifact_chat, looks_like_misrouted_artifact_chat_after_retry,
        route_failure_can_fall_back_to_chat, state_answer_kind_can_mask_plan_execution_followup,
        AgentExecutionIntent, PlainAgentChatOutcome,
    },
    agent_visibility::{looks_like_raw_tool_protocol, push_plain_provider_message_if_visible},
    event::{AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderStarted},
    normal_turn_decision::{
        parse_normal_turn_decision, NormalTurnDecision, NormalTurnExecuteIntent,
    },
    provider::{ChatMessage, ControllerProvider},
    session::Session,
    verified_state_answer::{
        resolve_state_answer_kind, resolved_state_answer_trace_metadata,
        verified_session_state_answer, VerifiedStateAnswerKind,
    },
};

pub(crate) fn handle_plain_agent_decision<P>(
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
            if answer_kind.is_none()
                && allow_route_retry
                && input_has_run_prefixed_command_shape(input)
            {
                session.push_reasoning_model_decision(
                    "state route without answer kind for command-shaped input; retrying route JSON",
                );
                return retry_plain_agent_chat_for_route_json_with_repair(
                    provider,
                    session,
                    input,
                    AGENT_ROUTE_LOCAL_WORK_CHAT_REPAIR_PROMPT,
                    has_verified_session_state(session),
                );
            }
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
            let run_command_shape = input_has_run_prefixed_command_shape(input);
            let content_for_guards = content.as_deref().unwrap_or_default();
            if allow_route_retry
                && (looks_like_local_work_chat_misroute(input, content_for_guards)
                    || run_command_shape)
            {
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
            if !allow_route_retry
                && (looks_like_local_work_chat_misroute(input, content_for_guards)
                    || run_command_shape
                    || input_contains_executable_command_shape(input))
            {
                session.record_reasoning_route("execute");
                session.push_reasoning_model_decision(
                    "normal turn decision returned chat for local work-shaped input after retry; routed to execute",
                );
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
            }
            if looks_like_misrouted_artifact_chat(content_for_guards) {
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
            if !allow_route_retry
                && looks_like_misrouted_artifact_chat_after_retry(content_for_guards)
            {
                session.record_reasoning_route("execute");
                session.push_reasoning_model_decision(
                    "normal turn decision returned compact artifact-like chat after retry; routed to execute",
                );
                return PlainAgentChatOutcome::Execute(AgentExecutionIntent::default());
            }
            session.record_reasoning_route("chat");
            session.push_reasoning_model_decision("normal turn decision selected chat");
            if classifier_chat_content_is_bad(input, content_for_guards) {
                session.push_reasoning_runtime_check(
                    "classifier chat content was ignored because it echoed input or leaked route instructions",
                );
            } else if content.is_some() {
                session.push_reasoning_runtime_check(
                    "classifier chat content ignored; requesting normal chat response",
                );
            }
            request_chat_response(provider, session, input);
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
            if route_failure_can_fall_back_to_chat(input, &assistant_text)
                && !input_has_run_prefixed_command_shape(input)
            {
                session.record_reasoning_route("chat");
                session.push_reasoning_model_decision(
                    "normal turn decision returned unstructured text for text-only input; requesting normal chat response",
                );
                request_chat_response(provider, session, input);
                return PlainAgentChatOutcome::Finished;
            }
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
    let request = provider_request_metadata_for_mode(provider, AgentProviderRequestMode::PlainChat);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_chat_context", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
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
    let request = provider_request_metadata_for_mode(provider, AgentProviderRequestMode::PlainChat);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_route_retry", 0),
    ));
    let mut messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
    ];
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
