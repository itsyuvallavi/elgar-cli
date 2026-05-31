use elgar_core::event::{
    ActionEvent, ActionKind, AssistantMessageSource, Event, ProviderTokenUsage,
};
use elgar_core::policy::ApprovalSource;

use crate::markdown::render_assistant_markdown;

use super::{
    conversation::{ConversationLineStyle, ThinkingPulse},
    verification_rendering::{render_verified_action_result, user_display_path},
};

pub(super) fn render_tui_event(event: &Event) -> Option<(String, ConversationLineStyle)> {
    match event {
        Event::UserMessage(message) => Some((
            render_user_message(&message.content),
            ConversationLineStyle::User,
        )),
        Event::AssistantMessage(message) => {
            if message.source == AssistantMessageSource::Controller
                && is_controller_action_boilerplate(&message.content)
            {
                return None;
            }

            let rendered = render_assistant_output(&message.content);
            let style = match message.source {
                AssistantMessageSource::Controller => ConversationLineStyle::Plain,
                AssistantMessageSource::VerifiedState => ConversationLineStyle::VerifiedState,
                AssistantMessageSource::Provider => ConversationLineStyle::Model,
            };
            Some((rendered, style))
        }
        Event::ProviderStarted(_) => {
            Some((render_thinking_progress(), ConversationLineStyle::Loading))
        }
        Event::ProviderFinished(_) => None,
        Event::ActionProposed(action) => {
            Some((render_action_proposed(action), ConversationLineStyle::Plain))
        }
        Event::ActionApproved(action) => {
            render_action_approved(action).map(|line| (line, ConversationLineStyle::Plain))
        }
        Event::ActionRejected(action) => {
            Some((render_action_rejected(action), ConversationLineStyle::Plain))
        }
        Event::ActionApplied(applied) => Some((
            render_verified_action_result(&applied.result),
            ConversationLineStyle::Plain,
        )),
        Event::ActionFailed(failed) => Some(format!(
            "Action failed: {} {:?} {}",
            failed.action_id, failed.action_kind, failed.reason
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::Error(error) => Some((
            render_error_line(&error.message),
            ConversationLineStyle::Plain,
        )),
    }
}

pub(super) fn is_hidden_policy_approval(event: &Event) -> bool {
    matches!(
        event,
        Event::ActionApproved(action)
            if action
                .approval_source
                .as_ref()
                .is_some_and(ApprovalSource::is_policy)
    )
}

fn render_action_proposed(action: &ActionEvent) -> String {
    if let Some(path) = create_directory_summary_path(&action.summary) {
        return format!(
            "I can create {}. Approve to create it.",
            user_display_path(path)
        );
    }

    match action.action_kind {
        ActionKind::CreateFile => format!(
            "I can write {}. Approve to write it.",
            action
                .target
                .as_deref()
                .or_else(|| action.summary.strip_prefix("write ").map(str::trim))
                .unwrap_or(&action.summary)
        ),
        ActionKind::CreateDirectory => format!(
            "I can create {}. Approve to create it.",
            action.target.as_deref().unwrap_or(&action.summary)
        ),
        ActionKind::ShellCommand => render_shell_action_proposal(action),
        _ => format!(
            "I can apply this action: {}. Approve to continue.",
            action.summary
        ),
    }
}

fn render_action_approved(action: &ActionEvent) -> Option<String> {
    if action
        .approval_source
        .as_ref()
        .is_some_and(ApprovalSource::is_policy)
    {
        return render_policy_approved_action(action);
    }

    if let Some(path) = create_directory_summary_path(&action.summary) {
        return Some(format!("Approved. Creating {}.", user_display_path(path)));
    }

    if action.summary.starts_with("create Markdown plan ") {
        return Some("Approved. Creating the plan.".to_string());
    }

    if action.summary.starts_with("execute Markdown plan in ") {
        return Some("Approved. Creating the project files.".to_string());
    }

    Some("Approved. Applying the action.".to_string())
}

fn render_policy_approved_action(_action: &ActionEvent) -> Option<String> {
    None
}

fn render_action_rejected(action: &ActionEvent) -> String {
    if let Some(path) = create_directory_summary_path(&action.summary) {
        return format!("Rejected. Did not create {}.", user_display_path(path));
    }

    "Rejected. No changes were made.".to_string()
}

fn create_directory_summary_path(summary: &str) -> Option<&str> {
    summary
        .strip_prefix("create directory ")
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

pub(super) fn render_turn_metrics_summary(
    total_duration_millis: u64,
    usage: Option<&ProviderTokenUsage>,
) -> Option<String> {
    let duration = format_duration(total_duration_millis)?;
    let mut parts = vec![format!("response {duration}")];

    if let Some(usage) = usage {
        let input = usage
            .prompt_tokens
            .map(compact_token_count)
            .unwrap_or_else(|| "?".to_string());
        let output = usage
            .completion_tokens
            .map(compact_token_count)
            .unwrap_or_else(|| "?".to_string());
        let total = usage
            .total_tokens
            .or_else(|| {
                usage
                    .prompt_tokens
                    .unwrap_or_default()
                    .checked_add(usage.completion_tokens.unwrap_or_default())
            })
            .map(compact_token_count)
            .unwrap_or_else(|| "?".to_string());
        parts.push(format!("↑{input} ↓{output}"));
        parts.push(format!("{total} provider tokens"));
    }

    Some(parts.join(" · "))
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000 {
        let value = tokens as f64 / 1_000.0;
        if tokens % 1_000 == 0 {
            format!("{value:.0}k")
        } else {
            format!("{value:.1}k")
        }
    } else {
        tokens.to_string()
    }
}

fn format_duration(millis: u64) -> Option<String> {
    if millis == 0 {
        return None;
    }
    if millis < 1_000 {
        Some(format!("{millis}ms"))
    } else {
        let seconds = millis as f64 / 1_000.0;
        Some(format!("{seconds:.1}s"))
    }
}

pub(super) fn render_user_message(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_assistant_output(content: &str) -> String {
    render_assistant_markdown(content)
}

fn render_shell_action_proposal(action: &ActionEvent) -> String {
    if let Some(path) = action.summary.strip_prefix("create Markdown plan ") {
        return format!(
            "I can create the plan at {}. Approve to write it.",
            user_display_path(path.trim())
        );
    }

    if let Some(path) = action.summary.strip_prefix("execute Markdown plan in ") {
        return format!(
            "I can create the project files in {}. Approve to create them.",
            user_display_path(path.trim())
        );
    }

    "I can run this command. Approve to run it.".to_string()
}

fn is_controller_action_boilerplate(content: &str) -> bool {
    let trimmed = content.trim();

    if trimmed.starts_with("Proposed ") && trimmed.contains(" action") {
        return true;
    }

    if trimmed.starts_with("Model-first tool call validated") {
        return true;
    }

    if trimmed.starts_with("I can create ")
        || trimmed.starts_with("I can write ")
        || trimmed.starts_with("I can apply this action:")
        || trimmed == "I can run the shell command. Approve to run it."
    {
        return true;
    }

    if trimmed.starts_with("Approved ") && trimmed.contains("Applying through the controller") {
        return true;
    }

    if matches!(
        trimmed,
        "Executed approved shell command and recorded the verified result."
            | "Applied approved action and recorded the verified result."
    ) {
        return true;
    }

    [
        "Created ",
        "Wrote ",
        "Updated ",
        "Overwrote ",
        "Deleted ",
        "Moved ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn render_thinking_progress() -> String {
    ThinkingPulse::default().label().to_string()
}

fn render_error_line(message: &str) -> String {
    if let Some(provider_error) = parse_provider_error(message) {
        format!(
            "Provider error · {}\n{}",
            provider_error.provider, provider_error.detail
        )
    } else if let Some(tool_error) = render_model_tool_error(message) {
        tool_error
    } else {
        format!("Error: {message}")
    }
}

fn render_model_tool_error(message: &str) -> Option<String> {
    let rest = message.strip_prefix("model tool `")?;
    let (tool, rest) = rest.split_once('`')?;
    let arg = rest
        .split_once("missing required argument `")
        .and_then(|(_prefix, rest)| rest.split_once('`').map(|(arg, _suffix)| arg));

    match arg {
        Some(arg) => Some(format!(
            "Tool call incomplete: {tool} needs {arg}. No action was applied."
        )),
        None => Some(format!(
            "Tool call malformed: {tool}. No action was applied."
        )),
    }
}

pub(super) struct ProviderErrorParts<'a> {
    provider: &'a str,
    detail: &'a str,
}

pub(super) fn parse_provider_error(message: &str) -> Option<ProviderErrorParts<'_>> {
    let (provider, rest) = message.split_once(" provider request ")?;
    let (_request_id, detail) = rest.split_once(" failed: ")?;
    Some(ProviderErrorParts { provider, detail })
}
