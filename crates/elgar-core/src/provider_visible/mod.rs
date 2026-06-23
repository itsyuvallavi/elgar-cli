//! Cleans provider text before Elgar records it as visible assistant output.
//!
//! Providers sometimes return chat-template markers or raw tool protocol text.
//! This module keeps normal assistant text and drops text that should not be
//! shown as a user-facing answer.

#[cfg(test)]
mod tests;

/// Return the provider text that is safe to show as assistant output.
///
/// This is intentionally conservative: empty text and raw tool protocol text
/// are filtered out, while normal provider-authored text is preserved.
pub fn provider_visible_text_from_text_only_output(message: String) -> Option<String> {
    let cleaned = strip_chat_template_channel_markers(message.trim());
    let text = cleaned.trim();
    if text.is_empty() || looks_like_raw_tool_protocol_text(text) {
        None
    } else {
        Some(text.to_string())
    }
}

fn looks_like_raw_tool_protocol_text(text: &str) -> bool {
    [
        "<tool_call>",
        "</tool_call>",
        "<function=",
        "</function>",
        "<parameter=",
        "</parameter>",
        "to=filesystem.",
        "filesystem.create",
        "filesystem.write",
        "filesystem.patch",
        "filesystem.move",
        "filesystem.delete",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn strip_chat_template_channel_markers(text: &str) -> String {
    let mut cleaned = text.to_string();
    for marker in [
        "<|channel|>final<|message|>",
        "<|channel|>commentary<|message|>",
        "<|channel|>analysis<|message|>",
        "<|channel|>thought<|message|>",
        "<|channel|>final",
        "<|channel|>commentary",
        "<|channel|>analysis",
        "<|channel|>thought",
        "<|channel>final",
        "<|channel>commentary",
        "<|channel>analysis",
        "<|channel>thought",
        "<channel|>",
        "<|message|>",
    ] {
        cleaned = cleaned.replace(marker, "");
    }
    cleaned
}
