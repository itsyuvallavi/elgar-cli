//! Renders compact verified memory indexes for harness prompts.
//!
//! Output is advisory context for the model. It never includes provider prose
//! or raw JSONL bodies.

use super::types::{HarnessMemoryIndex, HarnessMemoryKind};

/// Render verified session facts as compact prompt text.
pub fn render_verified_memory_for_prompt(index: &HarnessMemoryIndex) -> String {
    if index.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    push_kind_lines(&mut lines, index, HarnessMemoryKind::ReadFile, "read");
    push_kind_lines(
        &mut lines,
        index,
        HarnessMemoryKind::ListedDirectory,
        "listed",
    );
    push_kind_lines(&mut lines, index, HarnessMemoryKind::FindQuery, "find");
    push_kind_lines(&mut lines, index, HarnessMemoryKind::GrepQuery, "grep");
    push_kind_lines(
        &mut lines,
        index,
        HarnessMemoryKind::PermissionDecision,
        "permission",
    );
    push_kind_lines(
        &mut lines,
        index,
        HarnessMemoryKind::ApprovedExecution,
        "executed",
    );
    push_kind_lines(&mut lines, index, HarnessMemoryKind::StopReason, "stop");

    lines.join("\n")
}

fn push_kind_lines(
    lines: &mut Vec<String>,
    index: &HarnessMemoryIndex,
    kind: HarnessMemoryKind,
    label: &str,
) {
    for fact in index.facts_by_kind(kind) {
        lines.push(format!("- {label}: {}", fact.key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{HarnessMemoryFact, HarnessMemoryKind};

    #[test]
    fn renders_empty_index_as_blank() {
        assert!(render_verified_memory_for_prompt(&HarnessMemoryIndex::default()).is_empty());
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

        let rendered = render_verified_memory_for_prompt(&index);
        assert!(rendered.contains("- read: package.json"));
        assert!(rendered.contains("- executed: write:mem-audit.md"));
    }
}
