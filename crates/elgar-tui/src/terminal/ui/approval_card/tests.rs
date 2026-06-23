//! Tests for boxed approval card rendering.

use serde_json::json;

use elgar_core::harness::{PendingApproval, StructuredRequestKind, ValidatedStructuredRequest};

use super::render_pending_approval_card;
use crate::terminal::ui::approval_action::ApprovalAction;

#[test]
fn approval_card_renders_box_and_button_actions() {
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "create requested file".to_string(),
        arguments: Some(json!({
            "path": "hello-world",
            "content": "Hello, world!"
        })),
    };
    let approval = PendingApproval::from_request("approval-1", &request, "write requires approval");

    let lines = render_pending_approval_card(&approval, 100, ApprovalAction::Approve);
    let rendered = lines.join("\n");

    assert!(rendered.contains('╭'));
    assert!(rendered.contains('╯'));
    assert!(rendered.contains("Approval required"));
    assert!(rendered.contains("Create file"));
    assert!(rendered.contains("hello-world"));
    assert!(rendered.contains("Choose one:"));
    assert!(rendered.contains("[Approve]"));
    assert!(rendered.contains(" Deny "));
    assert!(rendered.contains("Enter selects"));
    assert!(!rendered.contains("/approve"));
    assert!(!rendered.contains("/deny"));
    assert!(!rendered.contains("tool: write"));
    assert!(!rendered.contains("approval-1"));
    assert!(!rendered.contains("arguments:"));
}

#[test]
fn approval_card_renders_batch_steps() {
    let requests = vec![
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "create requested file".to_string(),
            arguments: Some(json!({
                "path": "a.txt",
                "content": "A"
            })),
        },
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "create requested file".to_string(),
            arguments: Some(json!({
                "path": "b.txt",
                "content": "B"
            })),
        },
    ];
    let approval = PendingApproval::from_requests_with_launch_cwd(
        "approval-1",
        &requests,
        "batch requires approval",
        std::path::Path::new("/project"),
    )
    .unwrap();

    let lines = render_pending_approval_card(&approval, 100, ApprovalAction::Approve);
    let rendered = lines.join("\n");

    assert!(rendered.contains("Approve actions"));
    assert!(rendered.contains("Choose one:"));
    assert!(rendered.contains("2 actions"));
    assert!(rendered.contains("1. write · a.txt"));
    assert!(rendered.contains("2. write · b.txt"));
    assert!(!rendered.contains("arguments:"));
}
