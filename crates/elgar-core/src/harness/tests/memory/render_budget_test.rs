//! Tests for bounded durable-memory prompt rendering.

use crate::harness::{
    render_verified_memory_for_prompt_with_budget, HarnessMemoryFact, HarnessMemoryIndex,
    HarnessMemoryKind, HarnessMemoryPromptBudget,
};

#[test]
fn bounded_memory_render_keeps_empty_memory_blank() {
    let rendered = render_verified_memory_for_prompt_with_budget(
        &HarnessMemoryIndex::default(),
        &HarnessMemoryPromptBudget::default(),
    );

    assert!(rendered.text.is_empty());
    assert_eq!(rendered.stats.indexed_fact_count, 0);
    assert_eq!(rendered.stats.rendered_fact_count, 0);
}

#[test]
fn bounded_memory_render_caps_per_kind_and_prefers_newer_facts() {
    let mut index = HarnessMemoryIndex::default();
    for turn in 0..5 {
        index.push_unique(fact(
            HarnessMemoryKind::ReadFile,
            format!("file-{turn}.txt"),
            turn,
        ));
    }
    let budget = HarnessMemoryPromptBudget {
        read_file_facts: 2,
        ..HarnessMemoryPromptBudget::default()
    };

    let rendered = render_verified_memory_for_prompt_with_budget(&index, &budget);

    assert!(rendered.text.contains("read:\n- file-4.txt"));
    assert!(rendered.text.contains("- file-3.txt"));
    assert!(!rendered.text.contains("file-2.txt"));
    assert_eq!(rendered.stats.indexed_fact_count, 5);
    assert_eq!(rendered.stats.rendered_fact_count, 2);
    assert_eq!(rendered.stats.omitted_fact_count, 3);
    assert_eq!(rendered.stats.selection_strategy, "recent_by_kind");
    assert_eq!(rendered.stats.rendered_read_file_facts, 2);
    assert_eq!(rendered.stats.omitted_read_file_facts, 3);
    assert!(rendered.stats.memory_budget_hit);
}

#[test]
fn bounded_memory_render_excludes_permission_and_stop_facts() {
    let mut index = HarnessMemoryIndex::default();
    index.push_unique(fact(
        HarnessMemoryKind::PermissionDecision,
        "write".to_string(),
        1,
    ));
    index.push_unique(fact(
        HarnessMemoryKind::StopReason,
        "answer_now".to_string(),
        2,
    ));

    let rendered = render_verified_memory_for_prompt_with_budget(
        &index,
        &HarnessMemoryPromptBudget::default(),
    );

    assert!(rendered.text.is_empty());
    assert_eq!(rendered.stats.indexed_fact_count, 2);
    assert_eq!(rendered.stats.rendered_fact_count, 0);
    assert_eq!(rendered.stats.omitted_fact_count, 2);
    assert!(!rendered.stats.memory_budget_hit);
}

#[test]
fn bounded_memory_render_prunes_to_char_budget_and_adds_omission_line() {
    let mut index = HarnessMemoryIndex::default();
    for turn in 0..4 {
        index.push_unique(fact(
            HarnessMemoryKind::ReadFile,
            format!("very-long-file-name-{turn}.tsx"),
            turn,
        ));
    }
    let budget = HarnessMemoryPromptBudget {
        max_rendered_chars: 115,
        read_file_facts: 4,
        ..HarnessMemoryPromptBudget::default()
    };

    let rendered = render_verified_memory_for_prompt_with_budget(&index, &budget);

    assert!(rendered.text.chars().count() <= 115);
    assert!(rendered.text.contains("older verified facts omitted"));
    assert!(rendered.stats.rendered_fact_count < 4);
    assert!(rendered.stats.omitted_fact_count > 0);
    assert!(rendered.stats.memory_budget_hit);
}

#[test]
fn bounded_memory_render_keeps_under_budget_recall_facts() {
    let mut index = HarnessMemoryIndex::default();
    index.push_unique(fact(
        HarnessMemoryKind::ReadFile,
        "package.json".to_string(),
        0,
    ));
    index.push_unique(fact(
        HarnessMemoryKind::ListedDirectory,
        "app".to_string(),
        1,
    ));
    index.push_unique(fact(
        HarnessMemoryKind::ApprovedExecution,
        "write:mem-audit.md".to_string(),
        2,
    ));

    let rendered = render_verified_memory_for_prompt_with_budget(
        &index,
        &HarnessMemoryPromptBudget::default(),
    );

    assert!(rendered.text.contains("read:\n- package.json"));
    assert!(rendered.text.contains("listed:\n- app"));
    assert!(rendered.text.contains("executed:\n- write:mem-audit.md"));
    assert_eq!(rendered.stats.rendered_fact_count, 3);
    assert_eq!(rendered.stats.omitted_fact_count, 0);
    assert!(!rendered.stats.memory_budget_hit);
}

fn fact(kind: HarnessMemoryKind, key: String, turn_index: u64) -> HarnessMemoryFact {
    HarnessMemoryFact {
        kind,
        key,
        turn_index,
        source_event: "harness_tool_result_verified".to_string(),
    }
}
