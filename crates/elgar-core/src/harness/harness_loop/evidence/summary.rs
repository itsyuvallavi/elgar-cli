//! Compact evidence rendering for primitive harness decision prompts.
//!
//! The loop stores exact verified evidence locally, but future decision calls
//! only need enough context to avoid repeating work and choose the next
//! primitive. This module renders that smaller decision view.

use crate::harness::harness_loop::state::types::Evidence;

const MAX_COMPACT_BODY_CHARS: usize = 1_200;
const MAX_COMPACT_LINES: usize = 24;

/// Render evidence for a later decision call without replaying every full body.
pub(in crate::harness::harness_loop) fn render_compact_evidence_for_decision(
    evidence: &[Evidence],
) -> String {
    if evidence.is_empty() {
        return "(none)".to_string();
    }

    let mut rendered = String::new();
    for item in evidence {
        rendered.push_str("\n--- Evidence Summary: ");
        rendered.push_str(&item.label);
        rendered.push_str(" ---\n");
        rendered.push_str("verified: true\n");
        rendered.push_str("full_evidence_bytes: ");
        rendered.push_str(&item.bytes.to_string());
        rendered.push('\n');
        rendered.push_str("truncated: ");
        rendered.push_str(if item.truncated { "true" } else { "false" });
        rendered.push('\n');
        rendered.push_str("compact_body:\n");
        rendered.push_str(&compact_body(&item.body));
        rendered.push('\n');
    }
    rendered
}

fn compact_body(body: &str) -> String {
    let mut chars = 0usize;
    let mut compact = String::new();
    let mut omitted_lines = 0usize;

    for (index, line) in body.lines().enumerate() {
        if index >= MAX_COMPACT_LINES || chars >= MAX_COMPACT_BODY_CHARS {
            omitted_lines += 1;
            continue;
        }

        let remaining = MAX_COMPACT_BODY_CHARS.saturating_sub(chars);
        if line.chars().count() > remaining {
            compact.extend(line.chars().take(remaining));
            chars = MAX_COMPACT_BODY_CHARS;
        } else {
            compact.push_str(line);
            chars += line.chars().count();
        }
        compact.push('\n');
    }

    if body.lines().count() > MAX_COMPACT_LINES || body.chars().count() > MAX_COMPACT_BODY_CHARS {
        if omitted_lines == 0 {
            omitted_lines = body.lines().count().saturating_sub(MAX_COMPACT_LINES);
        }
        compact.push_str("[compact summary: full verified evidence retained locally");
        if omitted_lines > 0 {
            compact.push_str("; omitted_lines: ");
            compact.push_str(&omitted_lines.to_string());
        }
        compact.push_str("]\n");
    }

    compact
}
