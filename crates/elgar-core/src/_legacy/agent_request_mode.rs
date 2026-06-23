use crate::provider::{ControllerProvider, ProviderRequestMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentProviderRequestMode {
    PlainChat,
    ChatResponse,
    ToolEnabled,
    ToolResultSynthesis,
}

impl AgentProviderRequestMode {
    pub(crate) fn trace_label(self) -> &'static str {
        match self {
            Self::PlainChat => "plain_chat",
            Self::ChatResponse => "chat_response",
            Self::ToolEnabled => "tool_enabled",
            Self::ToolResultSynthesis => "tool_result_synthesis",
        }
    }
}

pub(crate) fn provider_request_metadata_for_mode<P>(
    provider: &P,
    mode: AgentProviderRequestMode,
) -> ProviderRequestMetadata
where
    P: ControllerProvider,
{
    provider.request_metadata_for_mode(mode.trace_label())
}
