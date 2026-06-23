//! Core event types recorded by Elgar.
//!
//! Events are runtime facts that CLI, TUI, logs, and tests can render or
//! inspect. Provider output is not treated as proof that files changed or
//! commands ran.

mod message;
mod provider;
mod provider_output;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use message::{AssistantMessage, AssistantMessageSource, ErrorEvent, UserMessage};
pub use provider::{
    ProviderFinished, ProviderStarted, ProviderStreamChunkReceived, ProviderStreamTimings,
};
pub use provider_output::{ProviderMetrics, ProviderOutput};

/// A core-recorded fact that can be rendered by CLI, TUI, or tests.
///
/// Events are not provider wishes. In particular, provider output is captured
/// only as provider output; it does not prove that a file changed, a command
/// ran, or an action moved through its lifecycle.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// The runtime recorded input received from the user.
    UserMessage(UserMessage),
    /// The runtime or action gate recorded assistant text for display.
    AssistantMessage(AssistantMessage),
    /// The runtime started a provider request.
    ProviderStarted(ProviderStarted),
    /// The runtime received provider output.
    ProviderFinished(ProviderFinished),
    /// The runtime received a live provider stream chunk before completion.
    ProviderStreamChunk(ProviderStreamChunkReceived),
    /// The core recorded an error.
    Error(ErrorEvent),
}
