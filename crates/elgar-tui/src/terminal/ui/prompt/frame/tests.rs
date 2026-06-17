//! Tests for inline prompt frame construction.

use serde_json::json;

use elgar_core::{
    event::ProviderStreamChunkReceived,
    harness::{PendingApproval, StructuredRequestKind, ValidatedStructuredRequest},
    provider::ProviderStreamChunk,
};

use crate::terminal::{ui::approval_action::ApprovalAction, TerminalShellContext};

use super::{
    active_working_frame_lines_with_cursor, inline_prompt_frame_lines_with_cursor,
    prompt_separator_line, LiveProviderOutput,
};

#[test]
fn pending_approval_actions_render_inside_prompt_card_not_footer() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approval-card-prompt-frame-{}",
        std::process::id()
    ));
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "create requested file".to_string(),
        arguments: Some(json!({
            "path": "hello-world.md",
            "content": "Hello"
        })),
    };
    let approval = PendingApproval::from_request("approval-1", &request, "write requires approval");
    let mut context =
        TerminalShellContext::new(&root, &root).with_approval_action_selected(ApprovalAction::Deny);
    context.approval_tool = Some("write".to_string());
    context.pending_approval = Some(approval);

    let (top_lines, _, _, footer_lines) =
        inline_prompt_frame_lines_with_cursor(&context, "", 0, 100);
    let top = top_lines.join("\n");
    let footer = footer_lines.join("\n");

    assert!(top.contains("Create file"));
    assert!(top.contains("hello-world.md"));
    assert!(top.contains(" Approve "));
    assert!(top.contains("[Deny]"));
    assert!(!footer.contains("Approval pending"));
    assert!(!footer.contains("Tab switches"));
    assert!(!footer.contains("[Deny]"));
}

#[test]
fn prompt_separator_uses_full_drawable_width() {
    assert_eq!(prompt_separator_line(120).chars().count(), 119);
    assert_eq!(prompt_separator_line(24).chars().count(), 23);
}

#[test]
fn active_frame_renders_streamed_answer_without_close_wait_noise() {
    let root = std::env::temp_dir().join(format!(
        "elgar-active-frame-stream-answer-{}",
        std::process::id()
    ));
    let context = TerminalShellContext::new(&root, &root);
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Text("Hello before close.".to_string()),
    ));

    let (_, reasoning_lines, response_lines, _, _, _, _) =
        active_working_frame_lines_with_cursor(&context, 0, 2, "", 0, &live_output, 100);
    let response = response_lines.join("\n");

    assert!(response.contains("Hello before close."));
    assert!(!response.contains("waiting for provider close"));
    assert!(reasoning_lines.is_empty());
}

#[test]
fn active_frame_hides_reasoning_after_answer_preview_exists() {
    let root = std::env::temp_dir().join(format!(
        "elgar-active-frame-hide-reasoning-{}",
        std::process::id()
    ));
    let context = TerminalShellContext::new(&root, &root);
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Reasoning("Internal reasoning".to_string()),
    ));
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        2,
        ProviderStreamChunk::Text("Visible answer.".to_string()),
    ));

    let (_, reasoning_lines, response_lines, _, _, _, _) =
        active_working_frame_lines_with_cursor(&context, 0, 2, "", 0, &live_output, 100);
    let response = response_lines.join("\n");

    assert!(response.contains("Visible answer."));
    assert!(reasoning_lines.is_empty());
}

#[test]
fn active_frame_hides_raw_tool_protocol_stream_text() {
    let root = std::env::temp_dir().join(format!(
        "elgar-active-frame-hide-protocol-{}",
        std::process::id()
    ));
    let context = TerminalShellContext::new(&root, &root);
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Text("to=filesystem.create_file code".to_string()),
    ));

    let (_, _, response_lines, _, _, _, _) =
        active_working_frame_lines_with_cursor(&context, 0, 2, "", 0, &live_output, 100);

    assert!(response_lines.is_empty());
}
