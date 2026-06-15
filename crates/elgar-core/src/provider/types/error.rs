//! Provider error types.
//!
//! These errors normalize configuration failures, HTTP/provider failures,
//! response parsing failures, empty responses, and network failures.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorResponse {
    pub error: ProviderErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub code: Option<String>,
}

/// Coarse category for provider failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderErrorKind {
    Configuration,
    ResponseParse,
    Provider,
    EmptyResponse,
    Network,
    Canceled,
}

/// Error returned by provider formatting, HTTP, or response parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub status_code: Option<u16>,
    pub code: Option<String>,
}

impl ProviderError {
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Configuration, message)
    }

    pub fn response_parse(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::ResponseParse, message)
    }

    pub fn provider(
        message: impl Into<String>,
        status_code: Option<u16>,
        code: Option<String>,
    ) -> Self {
        Self::new(ProviderErrorKind::Provider, message)
            .with_status(status_code)
            .with_code(code)
    }

    pub fn empty_response(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::EmptyResponse, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Network, message)
    }

    pub fn canceled(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Canceled, message)
    }

    fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status_code: None,
            code: None,
        }
    }

    pub(crate) fn with_status(mut self, status_code: Option<u16>) -> Self {
        self.status_code = status_code;
        self
    }

    fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status_code, self.code.as_deref()) {
            (Some(status), Some(code)) => write!(
                formatter,
                "{:?} provider error ({status}, {code}): {}",
                self.kind, self.message
            ),
            (Some(status), None) => {
                write!(
                    formatter,
                    "{:?} provider error ({status}): {}",
                    self.kind, self.message
                )
            }
            (None, Some(code)) => write!(
                formatter,
                "{:?} provider error ({code}): {}",
                self.kind, self.message
            ),
            (None, None) => write!(
                formatter,
                "{:?} provider error: {}",
                self.kind, self.message
            ),
        }
    }
}

impl std::error::Error for ProviderError {}
