//! MCP error types.
//!
//! These errors cover config, transport, and JSON-RPC failures without exposing
//! secret header values.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    Configuration(String),
    Network(String),
    HttpStatus { status: u16, body: String },
    ResponseParse(String),
    JsonRpc { code: i64, message: String },
    UnsupportedTransport(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "MCP configuration failed: {message}")
            }
            Self::Network(message) => write!(formatter, "MCP network failed: {message}"),
            Self::HttpStatus { status, body } => {
                write!(formatter, "MCP HTTP failed with status {status}: {body}")
            }
            Self::ResponseParse(message) => {
                write!(formatter, "MCP response parse failed: {message}")
            }
            Self::JsonRpc { code, message } => {
                write!(formatter, "MCP JSON-RPC failed ({code}): {message}")
            }
            Self::UnsupportedTransport(message) => {
                write!(formatter, "MCP transport unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for McpError {}
