//! Provider-facing controller helpers.
//!
//! These functions keep provider metadata and visible provider text handling
//! out of the main turn-flow module.

use crate::{
    controller_reporting::truth_guard_visible_message,
    event::{AssistantMessage, AssistantMessageSource, Event, ProviderMetrics},
    provider::ProviderRequestMetadata,
    provider_visible::provider_visible_text_from_text_only_output,
    session::{ProviderMetadata, Session},
};

pub(crate) fn record_provider_request_metadata(
    session: &mut Session,
    request: &ProviderRequestMetadata,
) {
    let mut metadata = ProviderMetadata::new(request.provider.clone());
    metadata.model = request.model.clone();
    metadata.request_id = Some(request.request_id.clone());
    session.set_provider_metadata(metadata);
}

pub(crate) fn set_provider_metrics_metadata(
    session: &mut Session,
    request: &ProviderRequestMetadata,
    metrics: ProviderMetrics,
) {
    let mut metadata = ProviderMetadata::new(request.provider.clone());
    metadata.model = request.model.clone();
    metadata.request_id = Some(request.request_id.clone());
    metadata.metrics = Some(metrics);
    session.set_provider_metadata(metadata);
    if let Some(metrics) = session
        .provider_metadata()
        .and_then(|metadata| metadata.metrics.as_ref())
        .cloned()
    {
        session.record_provider_metrics(&metrics);
    }
}

pub(crate) fn push_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message = truth_guard_visible_message(session, message.into());
    let Some(message) = provider_visible_text_from_text_only_output(message) else {
        return;
    };

    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Provider,
    )));
}
