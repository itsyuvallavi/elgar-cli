//! Public entry point for provider code.
//!
//! This module gathers provider configuration, shared provider types, the
//! LM Studio implementation, and the no-network stub behind one import surface.

mod cancel;
mod config;
mod http;
mod lm_studio;
mod stub;
mod types;

pub use cancel::ProviderCancelToken;
pub use config::{
    ProviderCompatibility, ProviderConfig, ReasoningCompatibility, LM_STUDIO_DEFAULT_BASE_URL,
    LM_STUDIO_DEFAULT_TIMEOUT_MILLIS, LM_STUDIO_PROVIDER_NAME,
};
pub use lm_studio::{
    chat_lm_studio, chat_lm_studio_streaming, format_chat_request, format_chat_request_body,
    format_chat_request_body_with_tools, format_chat_request_with_tools, parse_chat_response_json,
    parse_chat_response_json_with_metrics, parse_chat_stream_chunks, parse_chat_stream_line,
    parse_chat_stream_response, parse_provider_error_json, LmStudioProvider,
};
pub use stub::{ProviderStub, ProviderStubResponse};
pub use types::{
    ChatChoice, ChatMessage, ChatRequest, ChatResponse, ChatRole, ChatToolCall, ChatToolCallDelta,
    ChatToolCallFunction, ChatToolDefinition, ChatToolFunctionDefinition, ChatToolType, ChatUsage,
    ControllerProvider, ProviderBackendKind, ProviderError, ProviderErrorBody, ProviderErrorKind,
    ProviderErrorResponse, ProviderReasoningLevel, ProviderRequestMetadata, ProviderRequestProfile,
    ProviderStreamChunk,
};
