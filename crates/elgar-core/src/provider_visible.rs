pub fn provider_visible_text_from_text_only_output(message: String) -> Option<String> {
    let text = message.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
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
}
