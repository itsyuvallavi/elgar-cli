//! Provider repair calls for invalid primitive harness decisions.
//!
//! Repair calls give the model one chance to fix an invalid protocol response.
//! They do not choose a tool for the model; they only restate the accepted
//! response shapes and include the invalid response as evidence.

use std::time::Instant;

use crate::{
    event::{Event, ProviderFinished, ProviderOutput, ProviderStarted, ProviderStreamTimings},
    harness::{
        harness_loop::{
            provider::context::repair_prompt_context,
            state::{
                logging::{
                    log_decision_context, log_provider_call_canceled, log_provider_call_failed,
                    log_provider_call_finished, log_provider_call_started,
                },
                memory::HarnessWorkingMemory,
                types::Evidence,
            },
        },
        provider_route::HARNESS_TOOL_DECISION_REQUEST_MODE,
        tool_definitions::provider_tool_definitions_for_registry,
        ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ControllerProvider, ProviderCancelToken, ProviderErrorKind},
    session::Session,
};

/// Ask the model to repair one invalid primitive harness decision.
#[allow(clippy::too_many_arguments)]
pub(in crate::harness::harness_loop) fn request_model_choice_repair<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    registry: &PrimitiveToolRegistry,
    evidence: &[Evidence],
    memory: &HarnessWorkingMemory,
    round_index: usize,
    validation_error: &str,
    raw_response: &str,
    cancel: &ProviderCancelToken,
) -> Result<ProviderOutput, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let started = Instant::now();
    let request_mode = HARNESS_TOOL_DECISION_REQUEST_MODE;
    let loop_phase = "tool_decision_repair";
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

    let prompt_context = repair_prompt_context(
        session,
        input,
        registry,
        evidence,
        memory,
        validation_error,
        raw_response,
    );
    log_decision_context(
        session,
        round_index,
        prompt_context.evidence_mode,
        &prompt_context.stats,
        loop_phase,
    );
    let result = provider.chat_messages_with_tools_with_metadata_cancelable(
        prompt_context.messages,
        &request,
        tools,
        cancel,
    );

    match result {
        Ok(output) => {
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            session.push_event(Event::ProviderFinished(
                ProviderFinished::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    output.clone(),
                )
                .with_stream_timings(ProviderStreamTimings::new(
                    None,
                    None,
                    started.elapsed().as_millis() as u64,
                )),
            ));
            log_provider_call_finished(
                session,
                round_index,
                &request.request_id,
                request_mode,
                loop_phase,
                &output,
                &ProviderStreamTimings::new(None, None, started.elapsed().as_millis() as u64),
            );
            Ok(output)
        }
        Err(error) => {
            if error.kind == ProviderErrorKind::Canceled {
                log_provider_call_canceled(
                    session,
                    round_index,
                    &request.request_id,
                    request_mode,
                    loop_phase,
                );
            }
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
