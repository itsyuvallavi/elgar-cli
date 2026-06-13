//! Safety guards for the primitive harness loop.
//!
//! Elgar does not cap useful evidence collection here. These guards only track
//! duplicate evidence and repair attempts so the loop does not repeat the same
//! no-op work.

use std::collections::HashSet;

use crate::harness::harness_loop::state::types::{Evidence, EvidenceKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) enum BudgetCheck {
    Accept,
    RepeatedEvidence(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct PrimitiveLoopBudget {
    pub max_repair_attempts: usize,
    pub max_target_mismatches: usize,
}

impl Default for PrimitiveLoopBudget {
    fn default() -> Self {
        Self {
            max_repair_attempts: 1,
            max_target_mismatches: 2,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct PrimitiveLoopBudgetState {
    pub decision_calls: usize,
    pub repair_attempts: usize,
    pub target_mismatches: usize,
    evidence_keys: HashSet<String>,
}

impl PrimitiveLoopBudgetState {
    /// Return whether this request repeats evidence already collected this turn.
    pub fn check_request(
        &self,
        _budget: &PrimitiveLoopBudget,
        key: &EvidenceKey,
    ) -> Result<BudgetCheck, String> {
        let label = key.as_label();
        if self.evidence_keys.contains(&label) {
            return Ok(BudgetCheck::RepeatedEvidence(label));
        }
        Ok(BudgetCheck::Accept)
    }

    /// Record one verified evidence item for duplicate detection.
    pub fn record(&mut self, evidence: &Evidence) {
        self.evidence_keys.insert(evidence.label.clone());
    }
}
