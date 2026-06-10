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
    format!(
        "Pending approval\nid: {}\ntool: {}\nstatus: {}\nreason: {}\narguments: {}\n\nNot executed yet. Use /approve to run it or /deny to reject it.",
        approval.id,
        approval.tool,
        approval.status.as_str(),
        approval.reason,
        approval.arguments_preview
    )
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
}
