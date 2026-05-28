use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    context::ContextAccounting,
    event::{ProviderMetrics, ProviderTokenUsage},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowSource {
    Provider,
    Estimate,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextWindowSnapshot {
    pub current_tokens: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub used_percent: Option<u64>,
    pub remaining_percent: Option<u64>,
    pub source: ContextWindowSource,
    pub last_request_id: Option<String>,
    pub updated_at_unix_seconds: Option<u64>,
}

impl ContextWindowSnapshot {
    pub fn from_provider_usage(
        usage: &ProviderTokenUsage,
        context_window_tokens: Option<u64>,
        request_id: impl Into<String>,
    ) -> Self {
        let current_tokens = usage.total_tokens.or_else(|| {
            usage
                .prompt_tokens
                .unwrap_or_default()
                .checked_add(usage.completion_tokens.unwrap_or_default())
        });
        Self::new(
            current_tokens,
            context_window_tokens,
            ContextWindowSource::Provider,
            Some(request_id.into()),
        )
    }

    pub fn from_context_estimate(context: &ContextAccounting) -> Self {
        Self::new(
            context.estimated_tokens,
            context.max_window_tokens,
            ContextWindowSource::Estimate,
            None,
        )
    }

    pub fn unknown(context_window_tokens: Option<u64>) -> Self {
        Self::new(
            None,
            context_window_tokens,
            ContextWindowSource::Unknown,
            None,
        )
    }

    fn new(
        current_tokens: Option<u64>,
        context_window_tokens: Option<u64>,
        source: ContextWindowSource,
        last_request_id: Option<String>,
    ) -> Self {
        let used_percent = percentage(current_tokens, context_window_tokens);
        let remaining_percent = used_percent.map(|used| 100_u64.saturating_sub(used));
        Self {
            current_tokens,
            context_window_tokens,
            used_percent,
            remaining_percent,
            source,
            last_request_id,
            updated_at_unix_seconds: unix_seconds(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionTokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
}

impl SessionTokenTotals {
    pub fn add_provider_usage(&mut self, usage: &ProviderTokenUsage) {
        let input = usage.prompt_tokens.unwrap_or_default();
        let output = usage.completion_tokens.unwrap_or_default();
        let total = usage.total_tokens.unwrap_or_else(|| input + output);
        self.input_tokens = self.input_tokens.saturating_add(input);
        self.output_tokens = self.output_tokens.saturating_add(output);
        self.total_tokens = self.total_tokens.saturating_add(total);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastTurnTokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub request_id: String,
    pub source: ContextWindowSource,
}

impl LastTurnTokenUsage {
    pub fn from_provider_metrics(metrics: &ProviderMetrics) -> Option<Self> {
        let usage = metrics.usage.as_ref()?;
        Some(Self {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            request_id: metrics.request_id.clone(),
            source: ContextWindowSource::Provider,
        })
    }
}

fn percentage(current: Option<u64>, window: Option<u64>) -> Option<u64> {
    let current = current?;
    let window = window?;
    (window > 0).then_some(current.saturating_mul(100) / window)
}

fn unix_seconds() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs()
        .into()
}
