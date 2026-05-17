mod config;
mod http;
mod lm_studio;
mod stub;
mod types;

pub use config::{
    ProviderConfig, LM_STUDIO_DEFAULT_BASE_URL, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS,
    LM_STUDIO_PROVIDER_NAME,
};
pub use lm_studio::{
    chat_lm_studio, chat_lm_studio_streaming, format_chat_request, parse_chat_response_json,
    parse_chat_stream_chunks, parse_chat_stream_line, parse_chat_stream_response,
    parse_provider_error_json, LmStudioProvider,
};
pub use stub::{ProviderStub, ProviderStubResponse};
pub use types::{
    ChatChoice, ChatMessage, ChatRequest, ChatResponse, ChatRole, ChatUsage, ControllerProvider,
    ProviderError, ProviderErrorBody, ProviderErrorKind, ProviderErrorResponse,
    ProviderRequestMetadata, ProviderStreamChunk,
};
