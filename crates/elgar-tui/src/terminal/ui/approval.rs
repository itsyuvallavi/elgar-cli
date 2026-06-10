//! Pending approval display for the terminal prompt.
//!
//! This renderer shows core-owned pending approval state. It does not approve,
//! deny, or execute anything.

use std::io;

use elgar_core::harness::PendingApproval;

use crate::terminal::ui::{
    approval_card::{render_approval_footer_actions, render_pending_approval_card},
    prompt::terminal_width,
    render::print_plain_block,
};

/// Print the current pending approval card, if one exists.
pub(crate) fn print_pending_approval(approval: Option<&PendingApproval>) -> io::Result<()> {
    let Some(approval) = approval else {
        return Ok(());
    };

    print_plain_block(&render_pending_approval_text(approval))
}

pub(in crate::terminal) fn render_pending_approval_text(approval: &PendingApproval) -> String {
    render_pending_approval_card(approval, terminal_width()).join("\n")
}

pub(in crate::terminal) fn render_pending_approval_footer_actions(
    approval: &PendingApproval,
) -> String {
    render_approval_footer_actions(approval)
}

#[cfg(test)]
mod tests {
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
        let approval =
            PendingApproval::from_request("approval-1", &request, "write requires approval");

        let rendered = render_pending_approval_text(&approval);

        assert!(rendered.contains("Approval required"));
        assert!(rendered.contains("id: approval-1"));
        assert!(rendered.contains("tool: write"));
        assert!(rendered.contains("status: pending"));
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

        let rendered = render_pending_approval_text(&approval);

        assert!(rendered.contains("target: /tmp/hello-world"));
        assert!(rendered.contains("path type: absolute"));
        assert!(rendered.contains("scope: outside_launch_folder"));
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
        use crate::terminal::ui::prompt::drawable_width;
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

        let lines = crate::terminal::ui::approval_card::render_pending_approval_card(&approval, 40);
        let max_width = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        assert!(max_width <= drawable_width(40) + 2);
    }
}
