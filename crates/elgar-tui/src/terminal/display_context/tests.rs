//! Tests for terminal footer context-window labels.
//!
//! These tests keep the visible context-window percentage tied to provider
//! evidence instead of estimates.

use elgar_core::{
    context::ContextAccounting,
    token_accounting::{ContextWindowSnapshot, ProviderTokenUsage, SessionTokenTotals},
};

use super::footer_context_window_label;

#[test]
fn footer_shows_provider_backed_context_percentage() {
    let usage = ProviderTokenUsage {
        prompt_tokens: Some(3_200),
        completion_tokens: Some(800),
        total_tokens: Some(4_000),
    };
    let snapshot = ContextWindowSnapshot::from_provider_usage(&usage, Some(16_000), "request-1");

    assert_eq!(
        footer_context_window_label(Some(&snapshot)),
        Some("3.2k/16k (20%)".to_string())
    );
}

#[test]
fn footer_omits_percentage_for_estimated_context() {
    let mut accounting = ContextAccounting::unknown();
    accounting.max_window_tokens = Some(16_000);
    accounting.estimated_tokens = Some(4_000);
    let snapshot = ContextWindowSnapshot::from_context_estimate(&accounting);

    assert_eq!(
        footer_context_window_label(Some(&snapshot)),
        Some("?/16k".to_string())
    );
}

#[test]
fn footer_omits_percentage_without_provider_current_tokens() {
    let usage = ProviderTokenUsage {
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
    };
    let snapshot = ContextWindowSnapshot::from_provider_usage(&usage, Some(16_000), "request-1");

    assert_eq!(
        footer_context_window_label(Some(&snapshot)),
        Some("?/16k".to_string())
    );
}

#[test]
fn footer_can_show_cumulative_session_context_usage() {
    let totals = SessionTokenTotals {
        input_tokens: 2_000,
        output_tokens: 500,
        reasoning_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        total_tokens: 2_500,
    };
    let snapshot = ContextWindowSnapshot::from_session_totals(&totals, Some(16_000), "request-2");

    assert_eq!(
        footer_context_window_label(Some(&snapshot)),
        Some("2.5k/16k (15%)".to_string())
    );
}
