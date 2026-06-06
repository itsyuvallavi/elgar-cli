use serde_json::Value;

use crate::{
    agent_prompt_context::{agent_route_location_context, agent_verified_memory_context},
    agent_prompts::{
        AGENT_CHAT_RESPONSE_PROMPT, AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT,
        AGENT_POST_PLAN_CREATION_DECISION_PROMPT, AGENT_STATE_KIND_CLASSIFIER_PROMPT,
    },
    agent_provider_events::push_provider_finished,
    agent_request_mode::{provider_request_metadata_for_mode, AgentProviderRequestMode},
    agent_synthesis::push_synthesis_provider_message_if_visible,
    event::{ErrorEvent, Event, ProviderStarted},
    normal_turn_decision::{
        parse_normal_turn_decision, NormalTurnDecision, NormalTurnExecuteIntent,
    },
    provider::{ChatMessage, ControllerProvider},
    session::Session,
    verified_state_answer::{parse_verified_state_answer_kind, VerifiedStateAnswerKind},
};

pub(crate) fn request_chat_response<P>(provider: &P, session: &mut Session, input: &str)
where
    P: ControllerProvider,
{
    let request =
        provider_request_metadata_for_mode(provider, AgentProviderRequestMode::ChatResponse);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "chat_response", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_CHAT_RESPONSE_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            push_synthesis_provider_message_if_visible(session, assistant_text);
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider chat response request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }
}

pub(crate) fn post_plan_creation_decision_requests_execution<P>(
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
    let request = provider_request_metadata_for_mode(provider, AgentProviderRequestMode::PlainChat);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "plain_post_plan_classifier", 0),
    ));
    let messages = vec![
        ChatMessage::system(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
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

pub(crate) fn classify_verified_state_answer_kind<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> Option<VerifiedStateAnswerKind>
where
    P: ControllerProvider,
{
    let request = provider_request_metadata_for_mode(provider, AgentProviderRequestMode::PlainChat);
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
