//! Tests for pending approval text rendering.

use serde_json::json;

use elgar_core::harness::{PendingApproval, StructuredRequestKind, ValidatedStructuredRequest};

use super::render_pending_approval_text;

#[test]
fn pending_approval_display_names_commands_and_execution_state() {
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "create requested file".to_string(),
        arguments: Some(json!({
            "path": "hello-world",
            "content": "Hello, world!"
        })),
    };
    let approval = PendingApproval::from_request("approval-1", &request, "write requires approval");

    let rendered = render_pending_approval_text(&approval);

    assert!(rendered.contains("Create file"));
    assert!(rendered.contains("hello-world"));
    assert!(rendered.contains("[Approve]"));
    assert!(rendered.contains(" Deny "));
    assert!(!rendered.contains("id: approval-1"));
    assert!(!rendered.contains("tool: write"));
    assert!(!rendered.contains("status: pending"));
    assert!(!rendered.contains("arguments:"));
}

#[test]
fn pending_approval_display_warns_for_outside_target() {
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "create requested file".to_string(),
        arguments: Some(json!({
            "path": "/tmp/hello-world",
            "content": "Hello, world!"
        })),
    };
    let approval = PendingApproval::from_request_with_launch_cwd(
        "approval-1",
        &request,
        "write requires approval",
        std::path::Path::new("/project"),
    );

    let rendered = render_pending_approval_text(&approval);

    assert!(rendered.contains("/tmp/hello-world"));
    assert!(rendered.contains("WARNING: Approving may modify files outside the launch folder."));
    assert_eq!(
        rendered
            .matches("Approving may modify files outside the launch folder.")
            .count(),
        1
    );
}

#[test]
fn approval_card_respects_terminal_width() {
    use crate::terminal::{ui::approval_action::ApprovalAction, ui::prompt::drawable_width};
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "create requested file".to_string(),
        arguments: Some(json!({
            "path": "hello-world",
            "content": "Hello, world!"
        })),
    };
    let approval = PendingApproval::from_request("approval-1", &request, "write requires approval");

    let lines = crate::terminal::ui::approval_card::render_pending_approval_card(
        &approval,
        40,
        ApprovalAction::Approve,
    );
    let max_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    assert!(max_width <= drawable_width(40) + 2);
}
