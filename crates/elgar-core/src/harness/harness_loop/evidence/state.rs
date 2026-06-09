//! Evidence prompt-size accounting for the primitive harness loop.
//!
//! This keeps logging about full evidence versus compact decision evidence in
//! one place. It does not own or mutate evidence.

use crate::harness::harness_loop::state::types::Evidence;

use super::summary::render_compact_evidence_for_decision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct EvidencePromptStats {
    pub item_count: usize,
    pub full_bytes: usize,
    pub compact_bytes: usize,
}

/// Measure exact evidence size and compact decision-context size.
pub(in crate::harness::harness_loop) fn evidence_prompt_stats(
    evidence: &[Evidence],
) -> EvidencePromptStats {
    let full_bytes = evidence.iter().map(|item| item.bytes).sum();
    let compact_bytes = if evidence.is_empty() {
        0
    } else {
        render_compact_evidence_for_decision(evidence).len()
    };

    EvidencePromptStats {
        item_count: evidence.len(),
        full_bytes,
        compact_bytes,
    }
}
