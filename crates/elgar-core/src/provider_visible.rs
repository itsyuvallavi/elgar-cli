pub fn provider_visible_text_from_text_only_output(message: String) -> Option<String> {
    let cleaned = strip_chat_template_channel_markers(message.trim());
    let text = cleaned.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::provider_visible_text_from_text_only_output;

    #[test]
    fn provider_visible_drops_empty_text() {
        assert_eq!(
            provider_visible_text_from_text_only_output(" \n\t ".to_string()),
            None
        );
    }

    #[test]
    fn provider_visible_keeps_provider_text_after_outer_trim() {
        assert_eq!(
            provider_visible_text_from_text_only_output(" hello \n".to_string()),
            Some("hello".to_string())
        );
    }

    #[test]
    fn provider_visible_strips_chat_template_channel_markers() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "<|channel>thought\n<channel|>Findings:\n- app/page.tsx is minimal.".to_string()
            ),
            Some("Findings:\n- app/page.tsx is minimal.".to_string())
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "<|channel|>final<|message|>Hello.".to_string()
            ),
            Some("Hello.".to_string())
        );
    }
}
