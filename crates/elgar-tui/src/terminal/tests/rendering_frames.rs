use super::*;

#[test]
fn terminal_user_message_renders_as_padded_block_without_prompt_marker() {
    let mut conversation = ConversationPane::default();
    conversation.push_pending_provider_turn("hello");
    let styled = style_terminal_conversation("startup", &conversation, 12);
    let user_line = styled
        .lines
        .iter()
        .find(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "hello       ")
        })
        .unwrap();

    assert_eq!(user_line.style, crate::theme::user_input_block());
}

#[test]
fn live_and_completed_provider_transcript_styles_match() {
    assert_eq!(live_response_ansi(), transcript_output_ansi());

    let mut conversation = ConversationPane::default();
    conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "completed response",
        AssistantMessageSource::Provider,
    )));
    let styled = style_terminal_conversation("startup", &conversation, 32);
    let completed_line = styled
        .lines
        .iter()
        .find(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "completed response")
        })
        .unwrap();

    assert_eq!(completed_line.style, crate::theme::model_output());
}

#[test]
fn completed_provider_response_gets_model_label() {
    let mut conversation = ConversationPane::default();
    conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "completed response",
        AssistantMessageSource::Provider,
    )));

    let styled = style_terminal_conversation("startup", &conversation, 32);
    let rendered = styled
        .lines
        .iter()
        .map(|line| {
            line.spans
                .first()
                .map(|span| span.content.to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    assert!(rendered
        .windows(2)
        .any(|window| window == ["model", "completed response"]));
}

#[test]
fn terminal_markdown_code_blocks_print_with_compact_spacing() {
    let rendered = crate::markdown::render_assistant_markdown(
        "Use:\n\n```rust\n\nfn main() {}\n\n```\n\nDone.",
    );

    assert_eq!(
        plain_block_lines(&rendered, 80),
        vec!["Use:", "code (rust):", "    fn main() {}", "Done."]
    );
}

#[test]
fn inline_prompt_frame_matches_old_elgar_runtime_shape() {
    let context = TerminalShellContext::new("/Users/yuval/__git/elgar", "/Users/yuval/__git/elgar")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()));

    let (top, input, bottom, footer) = inline_prompt_frame_lines(&context, "hello", 86);

    assert_eq!(top.len(), 2);
    assert_eq!(top[0], "");
    assert!(top[1].starts_with("────"));
    assert_eq!(input, vec!["▸ hello▌"]);
    assert_eq!(bottom, vec![top[1].clone()]);
    assert_eq!(footer.len(), 1);
    assert!(footer[0].contains("openai/gpt-oss-20b"));
    assert!(!footer.join("\n").contains("select visible"));
    assert!(!footer.join("\n").contains("PgUp"));
    assert!(!footer.join("\n").contains("context:"));
}

#[test]
fn active_working_frame_keeps_prompt_and_footer_visible() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()))
        .with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: Some(4_000),
            max_window_tokens: Some(16_000),
        });

    let live_output = LiveProviderOutput::default();
    let (progress, reasoning, response, top, input, bottom, footer) =
        active_working_frame_lines(&context, 1, 7, "/cancel", &live_output, 80);

    assert_eq!(progress, vec!["", "Thinking. 7s · /cancel"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
    assert_eq!(top[0], "");
    assert!(top[1].starts_with("────"));
    assert_eq!(input, vec!["▸ /cancel▌"]);
    assert_eq!(bottom, vec![top[1].clone()]);
    assert_eq!(footer.len(), 1);
    assert!(footer[0].contains("repo"));
    assert!(footer[0].contains("model-a"));
    assert!(!footer.join("\n").contains("~25%/16k"));
    assert!(!footer.join("\n").contains('↑'));
    assert!(!footer.join("\n").contains('↓'));
    assert!(!footer.join("\n").contains("context:"));
}

#[test]
fn inline_prompt_frame_shows_cursor_for_empty_input() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) = inline_prompt_frame_lines(&context, "", 80);

    assert_eq!(input, vec!["▸ ▌"]);
}

#[test]
fn inline_prompt_frame_renders_cursor_at_edit_position() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) =
        inline_prompt_frame_lines_with_cursor(&context, "hello! 2", "hello! ".len(), 80);

    assert_eq!(input, vec!["▸ hello! ▌2"]);
}

#[test]
fn inline_prompt_frame_preserves_spaces_around_cursor() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) =
        inline_prompt_frame_lines_with_cursor(&context, "hello!   x", "hello! ".len(), 80);

    assert_eq!(input, vec!["▸ hello! ▌  x"]);
}

#[test]
fn inline_prompt_frame_preserves_trailing_spaces_before_cursor() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) =
        inline_prompt_frame_lines_with_cursor(&context, "hello!  ", "hello!  ".len(), 80);

    assert_eq!(input, vec!["▸ hello!  ▌"]);
}

#[test]
fn inline_prompt_frame_cursor_is_utf8_safe() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) =
        inline_prompt_frame_lines_with_cursor(&context, "a🙂b", "a🙂".len(), 80);

    assert_eq!(input, vec!["▸ a🙂▌b"]);
}

#[test]
fn active_working_frame_shows_initial_progress_before_provider_chunks() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let live_output = LiveProviderOutput::default();

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 0, "hello", &live_output, 80);

    assert_eq!(progress, vec!["", "Thinking · /cancel"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_renders_cursor_at_edit_position() {
    let context = TerminalShellContext::new("/repo", "/repo");
    let live_output = LiveProviderOutput::default();

    let (_progress, _reasoning, _response, _top, input, _bottom, _footer) =
        active_working_frame_lines_with_cursor(
            &context,
            0,
            0,
            "/cancel please",
            "/cancel ".len(),
            &live_output,
            80,
        );

    assert_eq!(input, vec!["▸ /cancel ▌please"]);
}

#[test]
fn active_working_frame_uses_neutral_provider_progress_for_project_requests() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let live_output = LiveProviderOutput::default();

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(
            &context,
            1,
            1,
            "create a TS Next.js and Tailwind project in ~/Demo",
            &live_output,
            80,
        );

    assert_eq!(progress, vec!["", "Thinking. 1s · /cancel"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_shows_live_reasoning_and_partial_response() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("Need greet.".to_string()));
    live_output.push_chunk(ProviderStreamChunk::Text("Hello".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert!(progress.is_empty());
    assert_eq!(reasoning, vec!["", "Need greet."]);
    assert_eq!(response, vec!["", "Hello"]);
    assert!(!reasoning.join("\n").contains("thinking"));
}

#[test]
fn active_working_frame_hides_live_tool_contract_response_preview() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "Use create_directory tool. Path? Project-relative path: likely the repository root."
            .to_string(),
    ));

    let (progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "create a folder", &live_output, 80);

    assert!(progress.is_empty());
    assert!(response.join(" ").contains("Use create_directory tool"));
}

#[test]
fn active_working_frame_polishes_common_live_reasoning_prefixes() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to answer briefly.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Need to answer briefly."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_removes_instruction_filler_from_reasoning() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to respond as Elgar, short.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Need to respond as Elgar, short."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_hides_incomplete_need_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("Need".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 2, 1, "hello", &live_output, 80);

    assert!(progress.is_empty());
    assert_eq!(reasoning, vec!["", "Need"]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_hides_incomplete_we_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("We".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 3, 1, "hello", &live_output, 80);

    assert!(progress.is_empty());
    assert_eq!(reasoning, vec!["", "We"]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_polishes_we_just_reasoning() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("We just greet.".to_string()));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert_eq!(reasoning, vec!["", "We just greet."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_polishes_we_need_live_reasoning_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "We need to inspect the prompt renderer tests.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert_eq!(
        reasoning,
        vec!["", "We need to inspect the prompt renderer tests."]
    );
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_does_not_turn_action_reasoning_into_action_claims() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to write hello.py.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Need to write hello.py."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_keeps_live_reasoning_summary_short() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(format!(
        "Need to explain {}",
        "local provider chunk handling ".repeat(20)
    )));

    let (_progress, reasoning, _response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 240);

    assert_eq!(reasoning.len(), 2);
    assert!(reasoning[1].chars().count() <= 160);
    assert!(reasoning[1].ends_with('…'));
}

#[test]
fn active_working_frame_reveals_response_before_completion() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "Hello! How can I help you today?".to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);
    assert_eq!(response, vec!["", "Hello! How can I help you today?"]);
}

#[test]
fn active_working_frame_can_suppress_provider_turn_response_preview() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.suppress_response_preview();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "CALL_CREATE_FILE_NOW_RANDOM".to_string(),
    ));

    let (progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "create files", &live_output, 80);

    assert_eq!(progress, vec!["", "Thinking 1s · /cancel"]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_can_suppress_provider_turn_reasoning_preview() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.suppress_reasoning_preview();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Create directory. Use create_directory tool.".to_string(),
    ));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "create files", &live_output, 80);

    assert_eq!(progress, vec!["", "Thinking 1s · /cancel"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_suppresses_provider_turn_tool_stream_with_neutral_progress() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.suppress_reasoning_preview();
    live_output.suppress_response_preview();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "to=functions create_directory? Let's continue. Need package.json and app files."
            .to_string(),
    ));
    live_output.push_chunk(ProviderStreamChunk::Text(
        "write ~/ElgarManualSmoke-20260526/package.json\n{\"name\":\"demo\"}".to_string(),
    ));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(
            &context,
            0,
            1,
            "can you please create a TS Next.js and Tailwind simple project in ~/ElgarManualSmoke-20260526",
            &live_output,
            100,
        );
    let rendered = [progress.clone(), reasoning.clone(), response.clone()]
        .concat()
        .join("\n");

    assert_eq!(progress, vec!["", "Thinking 1s · /cancel"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
    assert!(!rendered.contains("to=functions"));
    assert!(!rendered.contains("write ~/"));
    assert!(!rendered.contains("package.json"));
    assert!(!rendered.contains("\"name\""));
    assert!(!rendered.contains("setup"));
}

#[test]
fn active_working_frame_preserves_streamed_markdown_structure() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "I can:\n\n- Answer questions.\n- Summarise documents.".to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "what can you do?", &live_output, 80);

    assert_eq!(
        response,
        vec![
            "",
            "I can:",
            "- Answer questions.",
            "- Summarise documents."
        ]
    );
}

#[test]
fn live_and_completed_markdown_blocks_share_compact_spacing() {
    let response = "Sure! Let me suggest a small, clean folder structure.\n\ncode:\n\n    project/\n\n    src/\n\nWhat to do:\n\n1. Create directories.\n\n2. Move files.\n\nOnce you approve, I can generate commands.";
    let rendered = crate::markdown::render_assistant_markdown(response);
    let completed_lines = plain_block_lines(&rendered, 80);

    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(response.to_string()));

    let (_progress, _reasoning, live_lines, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "i meant create folders", &live_output, 80);

    assert_eq!(
        live_lines.into_iter().skip(1).collect::<Vec<_>>(),
        completed_lines
    );
    assert!(!rendered.contains("\n\n"));
}

#[test]
fn completed_terminal_transcript_groups_plain_lines_into_one_print_block() {
    let blocks = conversation_print_blocks(
        vec![
            (
                "Project Plan".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
            (
                "- First step.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
            (
                "- Second step.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
        ],
        false,
        false,
    );

    assert_eq!(
        blocks,
        vec![(
            "Project Plan\n- First step.\n- Second step.".to_string(),
            crate::panes::ConversationLineStyle::Plain
        )]
    );
}

#[test]
fn completed_provider_turn_transcript_can_skip_provider_thinking_block() {
    let blocks = conversation_print_blocks(
        vec![
            (
                "Create directory. Use create_directory tool.".to_string(),
                crate::panes::ConversationLineStyle::Thinking,
            ),
            (
                "Approved. Applying the action.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
            (
                "Created Desktop/Demo.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
        ],
        true,
        true,
    );

    assert_eq!(
        blocks,
        vec![(
            "Approved. Applying the action.\nCreated Desktop/Demo.".to_string(),
            crate::panes::ConversationLineStyle::Plain
        )]
    );
}

#[test]
fn active_working_frame_expands_inline_markdown_artifacts() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "Use this: ```bash # 1. Start lm-studio --port 1234 # 2. Check curl http://127.0.0.1:1234/v1/health ``` Done."
            .to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "help", &live_output, 120);

    assert!(response.contains(&"Use this:".to_string()));
    assert!(response.contains(&"code (bash):".to_string()));
    assert!(response.contains(&"    # 1. Start lm-studio --port 1234".to_string()));
    assert!(response.contains(&"    # 2. Check curl http://127.0.0.1:1234/v1/health".to_string()));
    assert!(response.contains(&"Done.".to_string()));
}

#[test]
fn live_provider_output_caps_reasoning_and_response_preview_buffers() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "r".repeat(LIVE_REASONING_PREVIEW_BYTES + 512),
    ));
    live_output.push_chunk(ProviderStreamChunk::Text(
        "x".repeat(LIVE_RESPONSE_PREVIEW_BYTES + 512),
    ));

    assert!(live_output.reasoning_preview_bytes() <= LIVE_REASONING_PREVIEW_BYTES);
    assert!(live_output.response_preview_bytes() <= LIVE_RESPONSE_PREVIEW_BYTES);
}

#[test]
fn live_stream_redraws_are_throttled_to_ten_fps() {
    let last_render = std::time::Instant::now();

    assert!(!live_render_due(
        last_render,
        last_render + LIVE_RENDER_INTERVAL - std::time::Duration::from_millis(1)
    ));
    assert!(live_render_due(
        last_render,
        last_render + LIVE_RENDER_INTERVAL
    ));
}
