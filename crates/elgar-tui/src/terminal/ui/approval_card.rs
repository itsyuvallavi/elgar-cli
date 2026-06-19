//! Boxed approval card rendering for the inline terminal.
//!
//! Cards display core-owned pending approval state and local action hints.
//! They do not approve, deny, or execute anything.

use elgar_core::harness::{ApprovalTargetPreview, PendingApproval, PendingApprovalStep};

use super::{approval_action::ApprovalAction, approval_card_style::color_card_line};

const CARD_MIN_CONTENT_WIDTH: usize = 28;
const CARD_MAX_CONTENT_WIDTH: usize = 72;

/// Render a pending approval as a bordered terminal card.
pub(crate) fn render_pending_approval_card(
    approval: &PendingApproval,
    width: usize,
    selected: ApprovalAction,
) -> Vec<String> {
    let mut body = approval_summary_lines(approval);

    if let Some(target) = approval.target_preview.as_ref() {
        body.extend(render_warning_lines(target));
    }

    if approval.is_batch() {
        body.push(format!("{} actions", approval.steps.len()));
        for (index, step) in approval.steps.iter().enumerate() {
            body.push(format!("{}. {}", index + 1, step_summary(step)));
        }
    }

    body.push(String::new());
    body.extend(render_action_lines(selected));

    render_simple_card("Approval required", &body, width)
}

/// Render an ANSI-colored pending approval card for the inline terminal prompt.
pub(crate) fn render_pending_approval_card_ansi(
    approval: &PendingApproval,
    width: usize,
    selected: ApprovalAction,
) -> Vec<String> {
    render_pending_approval_card(approval, width, selected)
        .into_iter()
        .map(|line| color_card_line(&line, selected))
        .collect()
}

fn approval_summary_lines(approval: &PendingApproval) -> Vec<String> {
    if approval.is_batch() {
        return vec![format!(
            "{}: {} actions",
            action_label(approval),
            approval.steps.len()
        )];
    }

    let target = approval
        .target_preview
        .as_ref()
        .map(|target| target.requested_path.clone())
        .or_else(|| argument_value(approval, "command"))
        .or_else(|| argument_value(approval, "path"))
        .unwrap_or_else(|| approval.tool.clone());
    vec![format!("{}: {target}", action_label(approval))]
}

fn action_label(approval: &PendingApproval) -> &'static str {
    match approval.tool.as_str() {
        "write" => "Create file",
        "edit" => "Edit file",
        "bash" => "Run command",
        "batch" => "Approve actions",
        _ => "Approve action",
    }
}

fn step_summary(step: &PendingApprovalStep) -> String {
    let target = step
        .target_preview
        .as_ref()
        .map(|target| target.requested_path.clone())
        .or_else(|| {
            step.request
                .arguments
                .as_ref()
                .and_then(|arguments| arguments.get("command"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            step.request
                .arguments
                .as_ref()
                .and_then(|arguments| arguments.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| step.tool.clone());
    format!("{} · {target}", step.tool)
}

fn argument_value(approval: &PendingApproval, key: &str) -> Option<String> {
    approval
        .request
        .arguments
        .as_ref()?
        .get(key)?
        .as_str()
        .map(ToString::to_string)
}

fn render_warning_lines(target: &ApprovalTargetPreview) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(warning) = target.warning.as_ref() {
        lines.push(format!("WARNING: {warning}"));
    }
    lines
}

fn render_action_lines(selected: ApprovalAction) -> Vec<String> {
    vec![
        format!(
            "Choose one: {} execute   {} cancel",
            action_button("Approve", selected == ApprovalAction::Approve),
            action_button("Deny", selected == ApprovalAction::Deny)
        ),
        "Enter selects · Tab switches".to_string(),
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
