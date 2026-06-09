//! Tests for assistant markdown rendering.

use super::super::{
    assistant_markdown_has_hidden_details, render_assistant_markdown,
    render_assistant_markdown_details,
};

#[test]
fn renders_plain_text_without_changing_content() {
    assert_eq!(
        render_assistant_markdown("Plain assistant text.\nSecond line."),
        "Plain assistant text.\nSecond line."
    );
}

#[test]
fn renders_paths_with_double_underscores_without_stripping_them() {
    let rendered =
        render_assistant_markdown("Path: `/Users/yuval/__git/elgar/playground/Nextjs-1`");

    assert!(rendered.contains("/Users/yuval/__git/elgar/playground/Nextjs-1"));
}

#[test]
fn renders_code_blocks_without_fences() {
    let rendered = render_assistant_markdown("Use this:\n```rust\nfn main() {}\n```");

    assert!(rendered.starts_with("Use this:\n ╭─ code (rust) · 1 line"));
    assert!(rendered.contains("│ fn main() {}"));
    assert!(rendered.contains("╰"));
    assert!(!rendered.contains("```"));
}

#[test]
fn renders_code_blocks_with_compact_fence_spacing() {
    let rendered =
        render_assistant_markdown("Use this:\n\n```rust\n\nfn main() {}\n\n```\n\nDone.");

    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(lines.first(), Some(&"Use this:"));
    assert!(lines
        .iter()
        .any(|line| line.starts_with(" ╭─ code (rust) · 1 line")));
    assert!(lines.iter().any(|line| line.contains("│ fn main() {}")));
    assert_eq!(lines.last(), Some(&"Done."));
    assert!(!rendered.contains("\n\n"));
}

#[test]
fn keeps_code_blocks_readable_with_line_count() {
    let rendered = render_assistant_markdown(
        "code:\n```python\nimport json\n\n\ndef main():\n\n    print(\"ok\")\n\n\nmain()\n```",
    );

    assert!(rendered.starts_with("code:\n ╭─ code (python) · 4 lines"));
    assert!(rendered.contains("│ import json"));
    assert!(rendered.contains("│ def main():"));
    assert!(rendered.contains("print(\"ok\")"));
    assert!(rendered.contains("│ main()"));
}

#[test]
fn compacts_blank_lines_between_plain_blocks_and_lists() {
    let rendered = render_assistant_markdown(
        "Sure! Let me suggest a small folder structure.\n\ncode:\n\n    project/\n\n    src/\n\nWhat to do:\n\n1. Create directories.\n\n2. Move files.\n\nOnce you approve, I can generate commands.",
    );

    assert_eq!(
        rendered,
        "Sure! Let me suggest a small folder structure.\ncode:\n    project/\n    src/\nWhat to do:\n1. Create directories.\n2. Move files.\nOnce you approve, I can generate commands."
    );
}

#[test]
fn expands_inline_fenced_code_blocks_into_readable_blocks() {
    let rendered = render_assistant_markdown(
        "Use this: ```bash # 1. Start lm-studio --port 1234 # 2. Check curl http://127.0.0.1:1234/v1/health ``` Done.",
    );

    assert!(rendered.starts_with("Use this:\n ╭─ code (bash) · 2 lines"));
    assert!(rendered.contains("│ # 1. Start lm-studio --port 1234"));
    assert!(rendered.contains("│ # 2. Check curl http://127.0.0.1:1234/v1/health"));
    assert!(rendered.ends_with("\nDone."));
}

#[test]
fn renders_lists_with_clean_markers() {
    let rendered = render_assistant_markdown("- **one**\n  * two\n1. `three`");

    assert_eq!(rendered, "- one\n  - two\n1. `three`");
}

#[test]
fn collapses_long_code_blocks_with_raw_details_hint() {
    let lines = (1..=90)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let markdown = format!("Large block:\n```text\n{lines}\n```");
    let rendered = render_assistant_markdown(&markdown);

    assert!(rendered.contains("╭─ code (text) · 90 lines · collapsed, showing 40"));
    assert!(rendered.contains("│ line-001"));
    assert!(rendered.contains("│ line-040"));
    assert!(rendered.contains("│ ... 50 lines hidden; use /details last or /copy raw"));
    assert!(!rendered.contains("line-090"));
    assert!(assistant_markdown_has_hidden_details(&markdown));

    let details = render_assistant_markdown_details(&markdown);
    assert!(details.contains("Raw markdown:"));
    assert!(details.contains("```text"));
    assert!(details.contains("line-090"));
}

#[test]
fn short_harness_answer_does_not_need_raw_details() {
    assert!(!assistant_markdown_has_hidden_details(
        "Hello! How can I help you today?"
    ));
}

#[test]
fn expands_inline_bullet_markers_into_list_lines() {
    let rendered = render_assistant_markdown(
        "I can: - Answer questions. - Summarise documents. - Generate config files.",
    );

    assert_eq!(
        rendered,
        "I can:\n- Answer questions.\n- Summarise documents.\n- Generate config files."
    );
}

#[test]
fn renders_tables_without_markdown_separator_rows() {
    let rendered = render_assistant_markdown("| File | State |\n| --- | --- |\n| a.rs | ok |");

    assert_eq!(rendered, "  File | State\n  -----+------\n  a.rs | ok   ");
    assert!(!rendered.contains("| --- |"));
}

#[test]
fn leaves_table_like_text_without_separator_as_plain_text() {
    let rendered = render_assistant_markdown("| File | State |\n| a.rs | ok |");

    assert_eq!(rendered, "| File | State |\n| a.rs | ok |");
}

#[test]
fn renders_preformatted_blocks_as_indented_text() {
    let rendered = render_assistant_markdown("tree:\n    src/\n      lib.rs");

    assert_eq!(rendered, "tree:\n    src/\n      lib.rs");
}
