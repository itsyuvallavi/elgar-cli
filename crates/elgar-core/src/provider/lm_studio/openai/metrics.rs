//! Metrics and timeout helpers for OpenAI-compatible LM Studio requests.

use std::time::Duration;

use crate::{
    event::ProviderMetrics,
    provider::{
        config::ProviderConfig,
        http::HttpTimeouts,
        types::{ChatRequest, ProviderBackendKind, ProviderRequestProfile},
    },
};

pub(super) fn http_timeouts(config: &ProviderConfig) -> HttpTimeouts {
    HttpTimeouts::from_millis(
        config.connect_timeout_millis(),
        config.read_timeout_millis(),
        config.write_timeout_millis(),
        config.request_timeout_millis(),
    )
}

pub(in crate::provider::lm_studio) fn metrics_for_request(
    request_id: &str,
    request: &ChatRequest,
    body_len: usize,
    profile: Option<&ProviderRequestProfile>,
) -> ProviderMetrics {
    let mut metrics = ProviderMetrics::new(
        request_id,
        Some(request.model.clone()),
        request.stream,
        request.messages.len(),
        body_len,
    );
    metrics.backend = Some(
        profile
            .map(|profile| profile.backend)
            .unwrap_or(ProviderBackendKind::OpenAiChatCompletions),
    );
    if let Some(profile) = profile {
        metrics.reasoning = profile.reasoning;
        metrics.context_length = profile.context_length;
        metrics.stats = profile.stats;
    }
    metrics
}

pub(super) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
