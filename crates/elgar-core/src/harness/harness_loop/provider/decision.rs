//! Provider decision calls for the primitive harness loop.
//!
//! This module asks the model what primitive evidence it wants next. Decision
//! calls always use the tool-capable provider method so primitive tool calls
//! have one consistent execution path.

use crate::{
    event::{
        Event, ProviderFinished, ProviderOutput, ProviderStarted, ProviderStreamChunkReceived,
        ProviderStreamTimings,
    },
    harness::{
        harness_loop::state::logging::{
            log_provider_call_canceled, log_provider_call_failed, log_provider_call_finished,
            log_provider_call_started, log_provider_stream_chunk,
        },
        provider_route::HARNESS_TOOL_DECISION_REQUEST_MODE,
        tool_definitions::provider_tool_definitions_for_registry,
        ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{
        ChatMessage, ControllerProvider, ProviderCancelToken, ProviderErrorKind,
        ProviderStreamChunk,
    },
    session::Session,
};

/// Ask the model for the next native tool-loop response.
pub(in crate::harness::harness_loop) fn request_native_tool_loop_response<P>(
    provider: &P,
    session: &mut Session,
    messages: &[ChatMessage],
    registry: &PrimitiveToolRegistry,
    round_index: usize,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<ProviderOutput, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let started = std::time::Instant::now();
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

    let mut sequence = 0_u64;
    let mut first_reasoning_ms = None;
    let mut first_text_ms = None;
    let mut last_reasoning_ms = None;
    let mut last_text_ms = None;
    let mut last_chunk_ms = None;
    let result = provider.chat_messages_with_tools_streaming_with_metadata_cancelable(
        messages.to_vec(),
        &request,
        tools,
        &mut |chunk| {
            sequence = sequence.saturating_add(1);
            record_stream_timing(
                &chunk,
                started.elapsed().as_millis() as u64,
                &mut first_reasoning_ms,
                &mut first_text_ms,
                &mut last_reasoning_ms,
                &mut last_text_ms,
                &mut last_chunk_ms,
            );
            let event = Event::ProviderStreamChunk(
                ProviderStreamChunkReceived::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    sequence,
                    chunk.clone(),
                )
                .with_context(request_mode, loop_phase, round_index),
            );
            session.push_event(event.clone());
            stream_events(event);
            log_provider_stream_chunk(
                session,
                round_index,
                &request.request_id,
                request_mode,
                loop_phase,
                sequence,
                &chunk,
            );
        },
        cancel,
    );

    match result {
        Ok(output) => {
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            let stream_timings = ProviderStreamTimings::from_stream_marks(
                first_reasoning_ms,
                first_text_ms,
                last_reasoning_ms,
                last_text_ms,
                last_chunk_ms,
                started.elapsed().as_millis() as u64,
            );
            session.push_event(Event::ProviderFinished(
                ProviderFinished::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    output.clone(),
                )
                .with_stream_timings(stream_timings.clone()),
            ));
            log_provider_call_finished(
                session,
                round_index,
                &request.request_id,
                request_mode,
                loop_phase,
                &output,
                &stream_timings,
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

fn record_stream_timing(
    chunk: &ProviderStreamChunk,
    elapsed_ms: u64,
    first_reasoning_ms: &mut Option<u64>,
    first_text_ms: &mut Option<u64>,
    last_reasoning_ms: &mut Option<u64>,
    last_text_ms: &mut Option<u64>,
    last_chunk_ms: &mut Option<u64>,
) {
    *last_chunk_ms = Some(elapsed_ms);
    match chunk {
        ProviderStreamChunk::Reasoning(_) => {
            if first_reasoning_ms.is_none() {
                *first_reasoning_ms = Some(elapsed_ms);
            }
            *last_reasoning_ms = Some(elapsed_ms);
        }
        ProviderStreamChunk::Text(_) => {
            if first_text_ms.is_none() {
                *first_text_ms = Some(elapsed_ms);
            }
            *last_text_ms = Some(elapsed_ms);
        }
        _ => {}
    }
}
