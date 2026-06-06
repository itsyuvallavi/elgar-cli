//! Converts core events into conversation pane lines.
//!
//! This is TUI-specific event rendering. Plain core rendering lives in
//! `elgar-core::renderer`.

use elgar_core::{
    event::{AssistantMessageSource, Event},
    token_accounting::ProviderTokenUsage,
};

use crate::markdown::render_assistant_markdown;

use super::conversation::{ConversationLineStyle, ThinkingPulse};

/// Render one core event as one visible conversation line, when appropriate.
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
        Event::Error(error) => Some((
            render_error_line(&error.message),
            ConversationLineStyle::Plain,
        )),
    }
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
        if tokens.is_multiple_of(1_000) {
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
