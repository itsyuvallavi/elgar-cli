//! Tests for terminal code block rendering.

use super::{render_code_block, CodeBlockInput};

#[test]
fn renders_code_block_metadata() {
    let rendered = render_code_block(CodeBlockInput::new(
        "tsx app/page.tsx",
        vec!["export default function Page() {}".to_string()],
    ));

    assert_eq!(rendered.lines.len(), 3);
    assert!(rendered.lines[0].starts_with(" ╭─ code (tsx) · app/page.tsx · 1 line "));
    assert!(rendered.lines[0].ends_with('╮'));
    assert!(rendered.lines[1].starts_with(" │ export default function Page() {}"));
    assert!(rendered.lines[1].ends_with(" │"));
    assert!(rendered.lines[2].starts_with(" ╰"));
    assert!(rendered.lines[2].ends_with('╯'));
    assert!(
        rendered.lines[0].chars().count() >= 68,
        "code block should read as a full-width terminal panel, not a tiny widget: {:?}",
        rendered.lines
    );
    assert!(!rendered.collapsed);
}

#[test]
fn infers_language_from_path_label() {
    let rendered = render_code_block(CodeBlockInput::new(
        "app/page.tsx",
        vec!["export default function Page() {}".to_string()],
    ));

    assert!(rendered.lines[0].starts_with(" ╭─ code (tsx) · app/page.tsx · 1 line "));
}

#[test]
fn wrapped_code_lines_show_continuation_marker() {
    let rendered = render_code_block(CodeBlockInput::new(
        "tsx app/page.tsx",
        vec![
            r#"<main className="flex min-h-screen flex-col items-center justify-center p-24">"#
                .to_string(),
        ],
    ));

    assert!(rendered
        .lines
        .iter()
        .any(|line| line.contains("justify-center")));
    assert!(rendered.lines.iter().any(|line| line.contains("↳ p-24")));
    assert!(!rendered
        .lines
        .iter()
        .any(|line| line.trim_end().ends_with("justify-cen │")));
}
