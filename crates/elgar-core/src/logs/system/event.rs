//! Defines typed records for Elgar's local JSONL log.
//!
//! These structs are deliberately generic. Runtime, TUI, and provider code pass
//! facts into them without making logging own any product behavior.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogPhase {
    Input,
    Tui,
    Worker,
    Runtime,
    Provider,
    Session,
    Render,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEvent {
    pub session_id: String,
    pub turn_id: u64,
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,
    pub timestamp_unix_ms: u128,
    pub phase: LogPhase,
    pub file: String,
    pub function: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogInput {
    pub turn_id: u64,
    pub phase: LogPhase,
    pub file: &'static str,
    pub function: &'static str,
    pub summary: &'static str,
    pub duration_ms: Option<u64>,
    pub metadata: Value,
}

impl LogInput {
    pub fn new(
        turn_id: u64,
        phase: LogPhase,
        file: &'static str,
        function: &'static str,
        summary: &'static str,
    ) -> Self {
        Self {
            turn_id,
            phase,
            file,
            function,
            summary,
            duration_ms: None,
            metadata: Value::Null,
        }
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}
