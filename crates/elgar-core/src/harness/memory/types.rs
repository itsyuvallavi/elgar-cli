//! Durable harness memory data shapes.
//!
//! These types hold compact verified facts, never raw provider prose or full
//! JSONL records.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HarnessMemoryKind {
    ReadFile,
    ListedDirectory,
    FindQuery,
    GrepQuery,
    PermissionDecision,
    ApprovedExecution,
    StopReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessMemoryFact {
    pub kind: HarnessMemoryKind,
    pub key: String,
    pub turn_index: u64,
    pub source_event: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessMemoryIndex {
    pub facts: Vec<HarnessMemoryFact>,
}

impl HarnessMemoryIndex {
    pub fn push_unique(&mut self, fact: HarnessMemoryFact) {
        if self
            .facts
            .iter()
            .any(|existing| existing.kind == fact.kind && existing.key == fact.key)
        {
            return;
        }
        self.facts.push(fact);
    }

    pub fn facts_by_kind(&self, kind: HarnessMemoryKind) -> Vec<&HarnessMemoryFact> {
        self.facts.iter().filter(|fact| fact.kind == kind).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

pub(super) fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}
