//! Provider reasoning visibility rules for the conversation pane.

use elgar_core::event::ProviderStarted;

pub(super) fn provider_reasoning_should_stay_hidden(started: &ProviderStarted) -> bool {
    matches!(
        started.request_mode.as_deref(),
        Some("harness_tool_decision" | "harness_synthesis")
    )
}
