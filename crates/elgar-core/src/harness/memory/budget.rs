//! Prompt-memory budgets for durable harness memory.
//!
//! The full memory index stays available as audit truth. This module selects a
//! bounded, useful prompt view from that index before provider calls.

use super::types::{HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessMemoryPromptBudget {
    pub max_rendered_chars: usize,
    pub read_file_facts: usize,
    pub listed_directory_facts: usize,
    pub find_facts: usize,
    pub grep_facts: usize,
    pub approved_execution_facts: usize,
    pub permission_facts: usize,
    pub stop_reason_facts: usize,
}

impl Default for HarnessMemoryPromptBudget {
    fn default() -> Self {
        Self {
            max_rendered_chars: 3_000,
            read_file_facts: 12,
            listed_directory_facts: 8,
            find_facts: 4,
            grep_facts: 4,
            approved_execution_facts: 8,
            permission_facts: 0,
            stop_reason_facts: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderedMemoryStats {
    pub indexed_fact_count: usize,
    pub rendered_fact_count: usize,
    pub omitted_fact_count: usize,
    pub rendered_memory_chars: usize,
    pub memory_budget_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedPromptMemory {
    pub facts: Vec<HarnessMemoryFact>,
    pub omitted_fact_count: usize,
    pub budget_hit: bool,
}

/// Select prompt-useful facts under per-kind budgets.
pub fn select_facts_for_prompt(
    index: &HarnessMemoryIndex,
    budget: &HarnessMemoryPromptBudget,
) -> SelectedPromptMemory {
    let mut facts = Vec::new();
    let mut useful_omitted_count = 0usize;
    useful_omitted_count += push_recent_kind(
        &mut facts,
        index,
        HarnessMemoryKind::ReadFile,
        budget.read_file_facts,
    );
    useful_omitted_count += push_recent_kind(
        &mut facts,
        index,
        HarnessMemoryKind::ListedDirectory,
        budget.listed_directory_facts,
    );
    useful_omitted_count += push_recent_kind(
        &mut facts,
        index,
        HarnessMemoryKind::FindQuery,
        budget.find_facts,
    );
    useful_omitted_count += push_recent_kind(
        &mut facts,
        index,
        HarnessMemoryKind::GrepQuery,
        budget.grep_facts,
    );
    useful_omitted_count += push_recent_kind(
        &mut facts,
        index,
        HarnessMemoryKind::ApprovedExecution,
        budget.approved_execution_facts,
    );

    let omitted_fact_count = index.facts.len().saturating_sub(facts.len());
    SelectedPromptMemory {
        facts,
        omitted_fact_count,
        budget_hit: useful_omitted_count > 0,
    }
}

fn push_recent_kind(
    facts: &mut Vec<HarnessMemoryFact>,
    index: &HarnessMemoryIndex,
    kind: HarnessMemoryKind,
    limit: usize,
) -> usize {
    let all_kind_facts = index.facts_by_kind(kind);
    let omitted_count = all_kind_facts.len().saturating_sub(limit);
    if limit == 0 {
        return omitted_count;
    }

    let mut kind_facts = all_kind_facts.into_iter().cloned().collect::<Vec<_>>();
    kind_facts.sort_by(|left, right| {
        right
            .turn_index
            .cmp(&left.turn_index)
            .then_with(|| left.key.cmp(&right.key))
    });
    facts.extend(kind_facts.into_iter().take(limit));
    omitted_count
}
