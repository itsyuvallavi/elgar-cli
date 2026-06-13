//! Shared result and evidence types for the primitive harness loop.
//!
//! These types are intentionally small data containers. Loop behavior belongs
//! in `coordinator.rs`, budget rules in `budget.rs`, and rendering/logging in
//! their own modules.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimitiveHarnessLoopResult {
    pub final_text: Option<String>,
    pub rounds: Vec<PrimitiveHarnessLoopRound>,
    pub stopped_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimitiveHarnessLoopRound {
    pub round_index: usize,
    pub tool: Option<String>,
    pub evidence_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct Evidence {
    pub label: String,
    pub body: String,
    pub bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) enum EvidenceKey {
    Read(String),
    Ls(String),
    Find(String, String),
    Grep(String, String),
    Mcp(String, String, String),
    InvalidMcp(String),
    Primitive(String),
}

impl EvidenceKey {
    pub(in crate::harness::harness_loop) fn as_label(&self) -> String {
        match self {
            Self::Read(path) => format!("read:{path}"),
            Self::Ls(path) => format!("ls:{path}"),
            Self::Find(path, pattern) => format!("find:{path}:{pattern}"),
            Self::Grep(path, query) => format!("grep:{path}:{query}"),
            Self::Mcp(server, tool, fingerprint) => format!("mcp:{server}:{tool}:{fingerprint}"),
            Self::InvalidMcp(fingerprint) => format!("invalid_mcp_call:{fingerprint}"),
            Self::Primitive(name) => name.clone(),
        }
    }
}
