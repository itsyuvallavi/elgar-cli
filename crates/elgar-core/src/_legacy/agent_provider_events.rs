use crate::{
    event::{Event, ProviderFinished, ProviderOutput},
    session::Session,
};

pub(crate) fn push_provider_finished(
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
