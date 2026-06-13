//! Provider decision calls for the primitive harness loop.
//!
//! This module asks the model what primitive evidence it wants next. Decision
//! calls always use the tool-capable provider method so primitive tool calls
//! have one consistent execution path.

use crate::{
    event::{Event, ProviderFinished, ProviderOutput, ProviderStarted},
    harness::{
        harness_loop::state::logging::{
            log_provider_call_failed, log_provider_call_finished, log_provider_call_started,
        },
        provider_route::HARNESS_TOOL_DECISION_REQUEST_MODE,
        tool_definitions::provider_tool_definitions_for_registry,
        ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ControllerProvider},
    session::Session,
};

/// Ask the model for the next native tool-loop response.
pub(in crate::harness::harness_loop) fn request_native_tool_loop_response<P>(
    provider: &P,
    session: &mut Session,
    messages: &[ChatMessage],
    registry: &PrimitiveToolRegistry,
    round_index: usize,
) -> Result<ProviderOutput, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let request_mode = HARNESS_TOOL_DECISION_REQUEST_MODE;
    let loop_phase = "native_tool_loop";
    let request = provider.request_metadata_for_mode(request_mode);
    let tools = provider_tool_definitions_for_registry(registry);
    let tool_count = tools.len();
    let profile = request.profile.as_ref();

    log_provider_call_started(
        session,
        round_index,
        &request.request_id,
        request_mode,
        loop_phase,
    );
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), request_mode, tool_count)
            .with_provider_profile(
                profile.map(|profile| profile.backend),
                profile.and_then(|profile| profile.reasoning),
                profile.and_then(|profile| profile.context_length),
                profile.and_then(|profile| profile.stats),
            ),
    ));

    let result =
        provider.chat_messages_with_tools_with_metadata(messages.to_vec(), &request, tools);

    match result {
        Ok(output) => {
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                request.provider.clone(),
                request.request_id.clone(),
                output.clone(),
            )));
            log_provider_call_finished(
                session,
                round_index,
                &request.request_id,
                request_mode,
                loop_phase,
                &output,
            );
            Ok(output)
        }
        Err(error) => {
            log_provider_call_failed(
                session,
                round_index,
                &request.request_id,
                request_mode,
                loop_phase,
                &error.to_string(),
            );
            Err(ModelChoiceTurnError::Provider(error))
        }
    }
}
