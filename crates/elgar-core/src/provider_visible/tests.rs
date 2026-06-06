//! Tests for provider visible text filtering.

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

#[test]
fn provider_visible_drops_raw_tool_protocol_text() {
    assert_eq!(
        provider_visible_text_from_text_only_output(
            "<tool_call>\n<function=shell_command>\n</function>\n</tool_call>".to_string()
        ),
        None
    );
    assert_eq!(
        provider_visible_text_from_text_only_output("to=filesystem.create_file code".to_string()),
        None
    );
}
