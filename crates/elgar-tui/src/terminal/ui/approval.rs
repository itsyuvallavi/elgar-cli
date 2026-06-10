//! Pending approval display for the terminal prompt.
//!
//! This renderer shows core-owned pending approval state. It does not approve,
//! deny, or execute anything.

use std::io;

use elgar_core::harness::PendingApproval;

use crate::terminal::ui::render::print_plain_block;

/// Print the current pending approval, if one exists.
pub(crate) fn print_pending_approval(approval: Option<&PendingApproval>) -> io::Result<()> {
    let Some(approval) = approval else {
        return Ok(());
    };

    print_plain_block(&render_pending_approval(approval))
}

fn render_pending_approval(approval: &PendingApproval) -> String {
    let target = approval
        .target_preview
        .as_ref()
        .map(render_target_preview)
        .unwrap_or_default();
    format!(
        "Pending approval\nid: {}\ntool: {}\nstatus: {}\nreason: {}\n{}arguments: {}\n\nNot executed yet. Use /approve to run it or /deny to reject it.",
        approval.id,
        approval.tool,
        approval.status.as_str(),
        approval.reason,
        target,
        approval.arguments_preview
    )
}

fn render_target_preview(approval: &elgar_core::harness::ApprovalTargetPreview) -> String {
    let mut rendered = format!(
        "target: {}\nresolved preview: {}\npath type: {}\nscope: {}\n",
        approval.requested_path,
        approval.resolved_preview_path,
        if approval.is_absolute {
            "absolute"
        } else {
            "relative"
        },
        approval.scope.as_str()
    );
    if let Some(warning) = approval.warning.as_ref() {
        rendered.push_str(&format!("warning: {warning}\n"));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use elgar_core::harness::{PendingApproval, StructuredRequestKind, ValidatedStructuredRequest};

    use super::render_pending_approval;

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
        let approval =
            PendingApproval::from_request("approval-1", &request, "write requires approval");

        let rendered = render_pending_approval(&approval);

        assert!(rendered.contains("Pending approval"));
        assert!(rendered.contains("id: approval-1"));
        assert!(rendered.contains("tool: write"));
        assert!(rendered.contains("status: pending"));
        assert!(rendered.contains("Not executed yet."));
        assert!(rendered.contains("/approve"));
        assert!(rendered.contains("/deny"));
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

        let rendered = render_pending_approval(&approval);

        assert!(rendered.contains("target: /tmp/hello-world"));
        assert!(rendered.contains("path type: absolute"));
        assert!(rendered.contains("scope: outside_launch_folder"));
        assert!(rendered.contains("warning: Approving may modify files outside the launch folder."));
    }
}
