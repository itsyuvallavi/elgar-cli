//! Boxed approval card rendering for the inline terminal.
//!
//! Cards display core-owned pending approval state and local action hints.
//! They do not approve, deny, or execute anything.

use elgar_core::harness::{ApprovalTargetPreview, PendingApproval};

use super::approval_action::ApprovalAction;

const CARD_MIN_CONTENT_WIDTH: usize = 28;
const CARD_MAX_CONTENT_WIDTH: usize = 72;

/// Render a pending approval as a bordered terminal card.
pub(crate) fn render_pending_approval_card(
    approval: &PendingApproval,
    width: usize,
    selected: ApprovalAction,
) -> Vec<String> {
    let mut body = vec![
        format!("tool: {}   id: {}", approval.tool, approval.id),
        format!("status: {}", approval.status.as_str()),
        format!("reason: {}", approval.reason),
    ];

    if let Some(target) = approval.target_preview.as_ref() {
        body.extend(render_target_lines(target));
    }

    if approval.is_batch() {
        body.push(format!("steps: {}", approval.steps.len()));
        for (index, step) in approval.steps.iter().enumerate() {
            body.push(format!(
                "{}. {} {}",
                index + 1,
                step.tool,
                step.target_preview
                    .as_ref()
                    .map(|target| target.requested_path.as_str())
                    .unwrap_or(step.arguments_preview.as_str())
            ));
        }
    } else {
        body.push(format!("arguments: {}", approval.arguments_preview));
    }
    body.push(String::new());
    body.extend(render_action_lines(selected));

    render_simple_card("Action prepared", &body, width)
}

/// Compact footer hint while an approval is pending.
pub(crate) fn render_approval_footer_actions(tool: &str, selected: ApprovalAction) -> String {
    format!(
        "Approval pending ({tool}) — {}   {}   Tab switches · Enter selects",
        action_button("Approve", selected == ApprovalAction::Approve),
        action_button("Deny", selected == ApprovalAction::Deny)
    )
}

fn render_target_lines(target: &ApprovalTargetPreview) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(warning) = target.warning.as_ref() {
        lines.push(format!("WARNING: {warning}"));
    }
    lines.push(format!("target: {}", target.requested_path));
    lines.push(format!("resolved: {}", target.resolved_preview_path));
    lines.push(format!(
        "path type: {}",
        if target.is_absolute {
            "absolute"
        } else {
            "relative"
        }
    ));
    lines.push(format!("scope: {}", target.scope.as_str()));
    lines
}

fn render_action_lines(selected: ApprovalAction) -> Vec<String> {
    vec![
        "Actions".to_string(),
        format!(
            "  {} execute prepared action",
            action_button("Approve", selected == ApprovalAction::Approve)
        ),
        format!(
            "  {} cancel without executing",
            action_button("Deny", selected == ApprovalAction::Deny)
        ),
        "  /approve and /deny still work as command fallback".to_string(),
    ]
}

fn action_button(label: &str, selected: bool) -> String {
    if selected {
        format!("[{label}]")
    } else {
        format!(" {label} ")
    }
}

fn render_simple_card(title: &str, body_lines: &[String], width: usize) -> Vec<String> {
    let content_width = card_content_width(title, body_lines, width);
    let title = truncate_to_width(title, content_width);
    let mut lines = vec![card_top_line(&title, content_width)];

    for line in body_lines {
        for segment in split_to_width(line, content_width) {
            lines.push(card_body_line(&segment, content_width));
        }
    }

    lines.push(card_bottom_line(content_width));
    lines
}

fn card_content_width(title: &str, body_lines: &[String], terminal_width: usize) -> usize {
    let drawable = drawable_width(terminal_width);
    let natural = body_lines
        .iter()
        .map(|line| line.chars().count())
        .chain(std::iter::once(title.chars().count()))
        .max()
        .unwrap_or(0);
    natural
        .clamp(CARD_MIN_CONTENT_WIDTH, CARD_MAX_CONTENT_WIDTH)
        .min(drawable.saturating_sub(6))
        .max(CARD_MIN_CONTENT_WIDTH)
}

fn card_top_line(title: &str, content_width: usize) -> String {
    let prefix = format!("─ {title} ");
    let rule_width = content_width + 3;
    let fill = "─".repeat(rule_width.saturating_sub(prefix.chars().count()));
    format!(" ╭{prefix}{fill}╮")
}

fn card_body_line(line: &str, content_width: usize) -> String {
    format!(
        " │ {}{} │",
        line,
        " ".repeat(content_width.saturating_sub(line.chars().count()))
    )
}

fn card_bottom_line(content_width: usize) -> String {
    format!(" ╰{}╯", "─".repeat(content_width + 3))
}

fn drawable_width(width: usize) -> usize {
    width.saturating_sub(2).max(20)
}

fn split_to_width(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    for word in line.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if candidate.chars().count() > width && !current.is_empty() {
            segments.push(current);
            current = word.to_string();
        } else if candidate.chars().count() > width {
            segments.extend(hard_split(word, width));
            current.clear();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    if segments.is_empty() {
        segments.push(String::new());
    }
    segments
}

fn hard_split(text: &str, width: usize) -> Vec<String> {
    let mut segments = Vec::new();
    let mut chunk = String::new();
    for ch in text.chars() {
        chunk.push(ch);
        if chunk.chars().count() >= width {
            segments.push(chunk);
            chunk = String::new();
        }
    }
    if !chunk.is_empty() {
        segments.push(chunk);
    }
    segments
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut truncated = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests;
