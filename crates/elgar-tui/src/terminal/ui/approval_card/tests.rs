//! Tests for boxed approval card rendering.

use serde_json::json;

use elgar_core::harness::{PendingApproval, StructuredRequestKind, ValidatedStructuredRequest};

use super::{render_approval_footer_actions, render_pending_approval_card};
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
    assert!(rendered.contains("[Approve]"));
    assert!(rendered.contains(" Deny "));
    assert!(rendered.contains("/approve"));
    assert!(rendered.contains("/deny"));
    assert!(rendered.contains("tool: write"));
}

#[test]
fn approval_footer_actions_name_tool_and_selected_button() {
    let footer = render_approval_footer_actions("write", ApprovalAction::Deny);

    assert!(footer.contains("write"));
    assert!(footer.contains(" Approve "));
    assert!(footer.contains("[Deny]"));
    assert!(footer.contains("Tab switches"));
    assert!(footer.contains("Enter selects"));
}
