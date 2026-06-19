//! Renders compact verified memory indexes for harness prompts.
//!
//! Output is advisory context for the model. It never includes provider prose
//! or raw JSONL bodies.

use super::{
    budget::{
        select_facts_for_prompt, HarnessMemoryPromptBudget, RenderedMemoryStats,
        SelectedPromptMemory, MEMORY_SELECTION_STRATEGY_RECENT_BY_KIND,
    },
    types::{HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMemoryPrompt {
    pub text: String,
    pub stats: RenderedMemoryStats,
}

/// Render a bounded verified-memory prompt view with stats.
pub fn render_verified_memory_for_prompt_with_budget(
    index: &HarnessMemoryIndex,
    budget: &HarnessMemoryPromptBudget,
) -> RenderedMemoryPrompt {
    if index.is_empty() {
        return RenderedMemoryPrompt {
            text: String::new(),
            stats: RenderedMemoryStats::default(),
        };
    }

    let mut selected = select_facts_for_prompt(index, budget);
    prune_to_char_budget(&mut selected, budget.max_rendered_chars);
    let text = render_selected_facts(&selected);
    let stats = rendered_memory_stats(index, &selected, text.chars().count());
    RenderedMemoryPrompt { stats, text }
}

fn rendered_memory_stats(
    index: &HarnessMemoryIndex,
    selected: &SelectedPromptMemory,
    rendered_memory_chars: usize,
) -> RenderedMemoryStats {
    RenderedMemoryStats {
        selection_strategy: MEMORY_SELECTION_STRATEGY_RECENT_BY_KIND,
        indexed_fact_count: index.facts.len(),
        rendered_fact_count: selected.facts.len(),
        omitted_fact_count: selected.omitted_fact_count,
        rendered_memory_chars,
        memory_budget_hit: selected.budget_hit,
        rendered_read_file_facts: count_selected_kind(selected, HarnessMemoryKind::ReadFile),
        rendered_listed_directory_facts: count_selected_kind(
            selected,
            HarnessMemoryKind::ListedDirectory,
        ),
        rendered_find_facts: count_selected_kind(selected, HarnessMemoryKind::FindQuery),
        rendered_grep_facts: count_selected_kind(selected, HarnessMemoryKind::GrepQuery),
        rendered_approved_execution_facts: count_selected_kind(
            selected,
            HarnessMemoryKind::ApprovedExecution,
        ),
        omitted_read_file_facts: omitted_kind_count(index, selected, HarnessMemoryKind::ReadFile),
        omitted_listed_directory_facts: omitted_kind_count(
            index,
            selected,
            HarnessMemoryKind::ListedDirectory,
        ),
        omitted_find_facts: omitted_kind_count(index, selected, HarnessMemoryKind::FindQuery),
        omitted_grep_facts: omitted_kind_count(index, selected, HarnessMemoryKind::GrepQuery),
        omitted_approved_execution_facts: omitted_kind_count(
            index,
            selected,
            HarnessMemoryKind::ApprovedExecution,
        ),
    }
}

fn count_selected_kind(selected: &SelectedPromptMemory, kind: HarnessMemoryKind) -> usize {
    selected
        .facts
        .iter()
        .filter(|fact| fact.kind == kind)
        .count()
}

fn omitted_kind_count(
    index: &HarnessMemoryIndex,
    selected: &SelectedPromptMemory,
    kind: HarnessMemoryKind,
) -> usize {
    index
        .facts_by_kind(kind)
        .len()
        .saturating_sub(count_selected_kind(selected, kind))
}

fn prune_to_char_budget(selected: &mut SelectedPromptMemory, max_chars: usize) {
    if max_chars == 0 {
        selected.omitted_fact_count += selected.facts.len();
        selected.facts.clear();
        selected.budget_hit = selected.omitted_fact_count > 0;
        return;
    }

    while !selected.facts.is_empty() && render_selected_facts(selected).chars().count() > max_chars
    {
        remove_oldest_fact(&mut selected.facts);
        selected.omitted_fact_count += 1;
        selected.budget_hit = true;
    }
}

fn remove_oldest_fact(facts: &mut Vec<HarnessMemoryFact>) {
    if let Some((index, _)) = facts
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.turn_index.cmp(&right.turn_index))
    {
        facts.remove(index);
    }
}

fn render_selected_facts(selected: &SelectedPromptMemory) -> String {
    if selected.facts.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    push_kind_section(
        &mut lines,
        &selected.facts,
        HarnessMemoryKind::ReadFile,
        "read",
    );
    push_kind_section(
        &mut lines,
        &selected.facts,
        HarnessMemoryKind::ListedDirectory,
        "listed",
    );
    push_kind_section(
        &mut lines,
        &selected.facts,
        HarnessMemoryKind::FindQuery,
        "find",
    );
    push_kind_section(
        &mut lines,
        &selected.facts,
        HarnessMemoryKind::GrepQuery,
        "grep",
    );
    push_kind_section(
        &mut lines,
        &selected.facts,
        HarnessMemoryKind::ApprovedExecution,
        "executed",
    );

    if selected.omitted_fact_count > 0 {
        lines.push(format!(
            "+ {} older verified facts omitted from prompt memory; full audit remains in JSONL",
            selected.omitted_fact_count
        ));
    }

    lines.join("\n")
}

fn push_kind_section(
    lines: &mut Vec<String>,
    facts: &[HarnessMemoryFact],
    kind: HarnessMemoryKind,
    label: &str,
) {
    let kind_facts = facts
        .iter()
        .filter(|fact| fact.kind == kind)
        .collect::<Vec<_>>();
    if kind_facts.is_empty() {
        return;
    }

    lines.push(format!("{label}:"));
    for fact in kind_facts {
        lines.push(format!("- {}", fact.key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{HarnessMemoryFact, HarnessMemoryKind};

    #[test]
    fn renders_empty_index_as_blank() {
        assert!(render_verified_memory_for_prompt_with_budget(
            &HarnessMemoryIndex::default(),
            &HarnessMemoryPromptBudget::default()
        )
        .text
        .is_empty());
    }

    #[test]
    fn renders_grouped_fact_lines() {
        let mut index = HarnessMemoryIndex::default();
        index.push_unique(HarnessMemoryFact {
            kind: HarnessMemoryKind::ReadFile,
            key: "package.json".to_string(),
            turn_index: 0,
            source_event: "harness_tool_result_verified".to_string(),
        });
        index.push_unique(HarnessMemoryFact {
            kind: HarnessMemoryKind::ApprovedExecution,
            key: "write:mem-audit.md".to_string(),
            turn_index: 2,
            source_event: "harness_write_execution_finished".to_string(),
        });

        let rendered = render_verified_memory_for_prompt_with_budget(
            &index,
            &HarnessMemoryPromptBudget::default(),
        )
        .text;
        assert!(rendered.contains("read:\n- package.json"));
        assert!(rendered.contains("executed:\n- write:mem-audit.md"));
    }
}
