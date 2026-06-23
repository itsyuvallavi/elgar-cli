//! Types for the primitive harness model-choice protocol.
//!
//! These types describe what the model requested and whether Elgar accepted the
//! request as a known, enabled primitive tool.

use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::harness::PrimitiveToolId;

pub type StructuredRequestKind = PrimitiveToolId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelChoice {
    Message {
        content: String,
    },
    AnswerNow {
        reason: String,
        evidence_depth: EvidenceDepth,
    },
    StructuredRequest(ValidatedStructuredRequest),
    StructuredRequests(Vec<ValidatedStructuredRequest>),
    InvalidStructuredRequest {
        error: StructuredRequestValidationError,
        raw: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceDepth {
    Enough,
    Limited,
}

impl EvidenceDepth {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "enough" => Some(Self::Enough),
            "limited" => Some(Self::Limited),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enough => "enough",
            Self::Limited => "limited",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedStructuredRequest {
    pub kind: StructuredRequestKind,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuredRequestValidationError {
    MalformedJson,
    MissingType,
    UnknownType(String),
    MissingKind,
    UnknownKind(String),
    DisabledKind(String),
    MissingReason,
    MissingRequests,
    EmptyRequests,
    TooManyRequests(usize),
    MixedMessageAndStructuredRequest,
    InvalidEvidenceDepth(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelChoiceTurnError {
    Provider(crate::provider::ProviderError),
    ProjectContext(String),
    ProjectFile(String),
}

impl fmt::Display for ModelChoiceTurnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider(error) => write!(formatter, "{error}"),
            Self::ProjectContext(error) => write!(formatter, "{error}"),
            Self::ProjectFile(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for ModelChoiceTurnError {}

impl From<crate::provider::ProviderError> for ModelChoiceTurnError {
    fn from(error: crate::provider::ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl StructuredRequestValidationError {
    pub fn as_str(&self) -> String {
        match self {
            Self::MalformedJson => "malformed_json".to_string(),
            Self::MissingType => "missing_type".to_string(),
            Self::UnknownType(value) => format!("unknown_type:{value}"),
            Self::MissingKind => "missing_kind".to_string(),
            Self::UnknownKind(value) => format!("unknown_kind:{value}"),
            Self::DisabledKind(value) => format!("disabled_kind:{value}"),
            Self::MissingReason => "missing_reason".to_string(),
            Self::MissingRequests => "missing_requests".to_string(),
            Self::EmptyRequests => "empty_requests".to_string(),
            Self::TooManyRequests(limit) => format!("too_many_requests:{limit}"),
            Self::MixedMessageAndStructuredRequest => {
                "mixed_message_and_structured_request".to_string()
            }
            Self::InvalidEvidenceDepth(value) => format!("invalid_evidence_depth:{value}"),
        }
    }
}
