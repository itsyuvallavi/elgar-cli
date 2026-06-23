//! Prompt-memory budgets for durable harness memory.
//!
//! The full memory index stays available as audit truth. This module selects a
//! bounded, useful prompt view from that index before provider calls.

use super::types::{HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind};

pub const MEMORY_SELECTION_STRATEGY_RECENT_BY_KIND: &str = "recent_by_kind";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedMemoryStats {
    pub selection_strategy: &'static str,
    pub indexed_fact_count: usize,
    pub rendered_fact_count: usize,
    pub omitted_fact_count: usize,
    pub rendered_memory_chars: usize,
    pub memory_budget_hit: bool,
    pub rendered_read_file_facts: usize,
    pub rendered_listed_directory_facts: usize,
    pub rendered_find_facts: usize,
    pub rendered_grep_facts: usize,
    pub rendered_approved_execution_facts: usize,
    pub omitted_read_file_facts: usize,
    pub omitted_listed_directory_facts: usize,
    pub omitted_find_facts: usize,
    pub omitted_grep_facts: usize,
    pub omitted_approved_execution_facts: usize,
}

impl Default for RenderedMemoryStats {
    fn default() -> Self {
        Self {
            selection_strategy: MEMORY_SELECTION_STRATEGY_RECENT_BY_KIND,
            indexed_fact_count: 0,
            rendered_fact_count: 0,
            omitted_fact_count: 0,
            rendered_memory_chars: 0,
            memory_budget_hit: false,
            rendered_read_file_facts: 0,
            rendered_listed_directory_facts: 0,
            rendered_find_facts: 0,
            rendered_grep_facts: 0,
            rendered_approved_execution_facts: 0,
            omitted_read_file_facts: 0,
            omitted_listed_directory_facts: 0,
            omitted_find_facts: 0,
            omitted_grep_facts: 0,
            omitted_approved_execution_facts: 0,
        }
    }
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
