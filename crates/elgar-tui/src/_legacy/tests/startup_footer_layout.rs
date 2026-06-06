//! Legacy startup footer layout tests.

use super::*;

#[test]
fn default_terminal_shell_is_empty_and_no_network() {
    let text = default_shell_text();

    assert!(text.contains("elgar v0.10"));
    assert!(text.contains("/commands · /permissions · /clear · /approve · /reject · /copy · /exit"));
    assert!(text.contains("Elgar is running with the default no-network stub provider."));
    assert!(text.contains("[Context]"));
    assert!(text.contains("[Provider]\n  stub-provider · none"));
    assert!(text.contains("[Policy]\n  auto_create_review_modify"));
    assert!(text.contains("(empty conversation)"));
    assert!(text.contains("> "));
    assert!(!text.contains("context:"));
    let footer = TerminalShellContext::new(".", ".")
        .with_provider("stub-provider", None)
        .footer_body(
            "ready",
            "select visible text natively | PgUp/PgDn scroll | /copy conversation",
        );
    assert!(!footer.contains("select visible text natively"));
    assert!(!footer.contains("PgUp/PgDn"));
    assert!(!footer.contains("/copy conversation"));
    assert!(!footer.contains("repo:"));
    assert!(!footer.contains("cwd:"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains('|'));
    assert!(!text.contains("Ctrl+Y copy conversation"));
    assert!(!text.contains("br:"));
    assert!(text.contains("default no-network stub"));
    assert!(!text.contains("lm-studio"));
    assert!(!text.contains("Commands:"));
    assert!(!text.contains("Skills"));
    assert!(!text.contains("MCP"));
    assert!(!text.contains("Bash"));
    assert!(!text.contains("API"));
    assert!(!text.contains("settings"));
}

#[test]
fn terminal_startup_block_lists_real_context_files_and_configured_provider() {
    let root = temp_root("terminal-startup-context");
    std::fs::write(root.join("AGENTS.md"), "instructions").unwrap();
    std::fs::write(root.join("elgar-provider.json"), "{}").unwrap();
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(&root, &root)
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting::from_default_local_files(
            &root, &root, None,
        ));

    let text = draw_to_text(&shell, &context);

    assert!(text.contains("[Context]"));
    assert!(text.contains("AGENTS.md"));
    assert!(text.contains("elgar-provider.json"));
    assert!(text.contains("[Provider]"));
    assert!(text.contains("lm-studio · openai/gpt-oss-20b"));
    assert!(!text.contains("AGENTS.md, elgar-provider.json"));
    assert!(!text.contains("lm-studio / openai/gpt-oss-20b"));
    assert!(!text.contains("Commands:"));
    assert!(!text.contains("Skills"));
    assert!(!text.contains("MCP"));
    assert!(!text.contains("Bash"));
    assert!(!text.contains("API"));
    assert!(!text.contains("settings"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_layout_renders_default_shell_regions() {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new("/repo", "/repo/crates");
    let text = draw_to_text(&shell, &context);

    assert!(text.contains("(empty conversation)"));
    assert!(text.contains("> "));
    assert!(text.contains("repo/crates"));
    assert!(!text.contains("context:"));
    assert!(!text.contains("br:"));
    assert!(!text.contains("select visible text"));
    assert!(!text.contains("provider:"));
    assert!(!text.contains("model:"));
    assert!(!text.contains("review action"));
    assert!(!text.contains("┌"));
    assert!(!text.contains("┐"));
    assert!(!text.contains("└"));
    assert!(!text.contains("┘"));
}

#[test]
fn terminal_layout_renders_provider_thinking_and_answer_in_same_tui_path() {
    let mut shell = TuiShell::new();
    shell.consume_events(&[
        Event::ProviderStarted(ProviderStarted::new("stub-provider", "request-1")),
        Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("The visible answer.")
                .with_thinking("Compact reasoning summary from provider."),
        )),
        Event::AssistantMessage(AssistantMessage::new(
            "The visible answer.",
            AssistantMessageSource::Provider,
        )),
    ]);

    let text = draw_to_text(&shell, &TerminalShellContext::new("/repo", "/repo"));

    assert!(text.contains("Compact reasoning summary from provider."));
    assert!(text.contains("The visible answer."));
    assert!(!text.contains("request-1"));
    assert!(!text.contains("Thinking:"));
}

#[test]
fn terminal_layout_renders_pending_action_only_when_present() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-hello",
        "hello.py",
        "",
    )]);
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file hello.py",
    );

    let text = draw_to_text(&shell, &TerminalShellContext::from_session(&session));

    assert!(text.contains("I can write hello.py. Approve to write it."));
    assert!(text.contains("review action"));
    assert!(text.contains("File: hello.py"));
    assert!(text.contains("Status: waiting for approval"));
    assert!(text.contains("[ Approve ]  [ Reject ]"));
    assert!(text.contains("No changes have been made yet"));
    assert!(text.contains("Use /approve to apply or /reject"));
    assert!(!text.contains("Action: action-1 CreateFile"));
    assert!(text.contains("> "));
    assert!(text.contains("review action"));
}

#[test]
fn terminal_footer_uses_provider_model_metadata_when_available() {
    let controller =
        Controller::new(elgar_core::provider::ProviderStub::new("local").with_model("model-a"));
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what does the harness do?");
    shell.consume_events(&result.events);

    let context = TerminalShellContext::from_session(&session);
    let text = draw_to_text(&shell, &context);
    let footer = context.footer_body("reply ready", "select visible text");

    assert!(text.contains("model-a"));
    assert!(footer.contains("model-a"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("reply ready"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("provider configured"));
    assert!(!footer.contains("stub/no-network"));
    assert!(!text.contains("Provider progress:"));
}

#[test]
fn terminal_footer_formats_compact_repo_cwd_and_right_aligned_model() {
    let root = temp_root("terminal-footer-git-context");
    let cwd = root.join("crates").join("elgar-tui");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        root.join(".git").join("HEAD"),
        "ref: refs/heads/feature/footer\n",
    )
    .unwrap();
    let context = TerminalShellContext::new(&root, &cwd)
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()));

    let footer = context.footer_body("ready", "select visible text");

    assert!(footer.contains(root.file_name().unwrap().to_str().unwrap()));
    assert!(footer.contains("crates/elgar-tui"));
    assert!(footer.contains("openai/gpt-oss-20b"));
    assert_eq!(footer.lines().count(), 1);
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("repo:"));
    assert!(!footer.contains("cwd:"));
    assert!(!footer.contains("branch:"));
    assert!(!footer.contains("(feature/footer)"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains('|'));
    assert!(!footer.contains('%'));
    assert!(!footer.contains("tokens"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_footer_shows_unknown_window_usage_for_estimated_context_accounting() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting {
            loaded_files: vec![LoadedContextFile {
                display_path: "AGENTS.md".to_string(),
                bytes: 1284,
                estimated_tokens: 321,
                truncated: false,
            }],
            omitted_files: Vec::new(),
            estimated_tokens: Some(321),
            max_window_tokens: Some(128_000),
        });

    let footer = context.footer_body("ready", "copy");
    let lines: Vec<_> = footer.lines().collect();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("repo"));
    assert!(lines[0].contains("openai/gpt-oss-20b"));
    assert!(footer.contains("?/128k"));
    assert!(!footer.contains("~0.3%/128k"));
    assert!(!footer.contains("321/128k"));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("ctx "));
    assert!(!footer.contains("TBD"));
}

#[test]
fn terminal_footer_does_not_treat_provider_metrics_as_window_usage_without_snapshot() {
    let mut metrics = ProviderMetrics::new(
        "request-usage",
        Some("openai/gpt-oss-20b".to_string()),
        false,
        1,
        128,
    );
    metrics.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(7),
        completion_tokens: Some(3),
        total_tokens: Some(10),
    });
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: Some(321),
            max_window_tokens: Some(128_000),
        })
        .with_provider_metrics(metrics);

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(footer.contains("openai/gpt-oss-20b"));
    assert!(footer.contains("?/128k"));
    assert!(!footer.contains("~0.3%/128k"));
    assert!(!footer.contains("ctx "));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("↑7 ↓3"));
    assert!(!footer.contains("10/128k"));
}

#[test]
fn terminal_footer_shows_provider_backed_window_usage_pair() {
    let usage = ProviderTokenUsage {
        prompt_tokens: Some(2_200),
        completion_tokens: Some(24),
        total_tokens: Some(2_224),
    };
    let mut metrics = ProviderMetrics::new(
        "request-usage",
        Some("openai/gpt-oss-20b".to_string()),
        false,
        1,
        128,
    );
    metrics.usage = Some(usage.clone());
    let mut context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_provider_metrics(metrics);
    context.context_window_snapshot = Some(ContextWindowSnapshot::from_provider_usage(
        &usage,
        Some(128_000),
        "request-usage",
    ));

    let footer = context.footer_body("ready", "copy");
    let lines: Vec<_> = footer.lines().collect();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("repo"));
    assert!(lines[0].contains("openai/gpt-oss-20b"));
    assert!(footer.contains("2.2k/128k"));
    assert!(!footer.contains("↑2.2k ↓24 1.7%/128k"));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("ctx "));
    assert!(!footer.contains("tokens"));
}

#[test]
fn terminal_context_from_session_carries_model_but_not_usage_to_footer() {
    #[derive(Clone)]
    struct UsageProvider;

    impl ControllerProvider for UsageProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "usage-provider",
                Some("model-a".to_string()),
                "usage-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            let mut metrics =
                ProviderMetrics::new("usage-request-1", Some("model-a".to_string()), false, 1, 64);
            metrics.usage = Some(ProviderTokenUsage {
                prompt_tokens: Some(11),
                completion_tokens: Some(5),
                total_tokens: Some(16),
            });
            Ok(ProviderOutput::new("measured").with_metrics(metrics))
        }
    }

    let controller = Controller::new(UsageProvider);
    let mut session = Session::new("session-1", "/repo", "/repo");

    controller.turn(&mut session, "hello");

    let context = TerminalShellContext::from_session(&session);
    let footer = context.footer_body("ready", "copy");

    assert!(footer.contains("model-a"));
    assert!(!footer.contains("↑11 ↓5 ?%/?"));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("ctx "));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("16 tokens"));
}

#[test]
fn terminal_footer_hides_unknown_context_when_provider_usage_is_absent() {
    let metrics = ProviderMetrics::new(
        "request-no-usage",
        Some("openai/gpt-oss-20b".to_string()),
        false,
        1,
        128,
    );
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: None,
        })
        .with_provider_metrics(metrics);

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(!footer.contains("?%/?"));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("ctx "));
    assert!(!footer.contains("TBD"));
}

#[test]
fn terminal_footer_shows_unknown_usage_with_configured_window() {
    let context =
        TerminalShellContext::new("/repo", "/repo").with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: Some(128_000),
        });

    let footer = context.footer_body("ready", "copy");

    assert!(footer.contains("?/128k"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("?%/128k"));
    assert!(!footer.contains('↑'));
    assert!(!footer.contains('↓'));
    assert!(!footer.contains("ctx "));
}

#[test]
fn terminal_footer_context_pressure_uses_documented_thresholds() {
    let context = TerminalShellContext::new(".", ".").with_context_accounting(ContextAccounting {
        loaded_files: Vec::new(),
        omitted_files: Vec::new(),
        estimated_tokens: Some(7_999),
        max_window_tokens: Some(16_000),
    });
    assert_eq!(
        context_window_pressure(context.context_window_snapshot.as_ref()),
        ContextWindowPressure::Normal
    );

    let context = TerminalShellContext::new(".", ".").with_context_accounting(ContextAccounting {
        loaded_files: Vec::new(),
        omitted_files: Vec::new(),
        estimated_tokens: Some(8_000),
        max_window_tokens: Some(16_000),
    });
    assert_eq!(
        context_window_pressure(context.context_window_snapshot.as_ref()),
        ContextWindowPressure::Mild
    );

    let context = TerminalShellContext::new(".", ".").with_context_accounting(ContextAccounting {
        loaded_files: Vec::new(),
        omitted_files: Vec::new(),
        estimated_tokens: Some(11_200),
        max_window_tokens: Some(16_000),
    });
    assert_eq!(
        context_window_pressure(context.context_window_snapshot.as_ref()),
        ContextWindowPressure::Warning
    );

    let context = TerminalShellContext::new(".", ".").with_context_accounting(ContextAccounting {
        loaded_files: Vec::new(),
        omitted_files: Vec::new(),
        estimated_tokens: Some(13_760),
        max_window_tokens: Some(16_000),
    });
    assert_eq!(
        context_window_pressure(context.context_window_snapshot.as_ref()),
        ContextWindowPressure::Danger
    );
}

#[test]
fn terminal_status_uses_named_theme_styles_by_state() {
    assert_eq!(status_style("ready"), crate::theme::success());
    assert_eq!(status_style("reply ready"), crate::theme::success());
    assert_eq!(status_style("working"), crate::theme::thinking());
    assert_eq!(
        status_style("review action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(
        status_style("approved action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(
        status_style("rejected action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(status_style("failed action-1"), crate::theme::error());
    assert_eq!(status_style("provider error"), crate::theme::error());
    assert_eq!(status_style("sent"), crate::theme::muted());
}

#[test]
fn terminal_footer_shows_lm_studio_provider_and_model_without_usage_claims() {
    let mut context = TerminalShellContext::new("/repo", "/repo");
    context.provider = Some("lm-studio".to_string());
    context.model = Some("openai/gpt-oss-20b".to_string());

    let footer = context.footer_body("ready", "select visible text");

    assert!(footer.contains("openai/gpt-oss-20b"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains("live/local"));
    assert!(!footer.contains("stub/no-network"));
}

#[test]
fn terminal_conversation_scrollback_keeps_input_status_and_pending_visible() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-hello",
        "hello.py",
        "",
    )]);
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    shell.conversation.lines = (0..20).map(|index| format!("line {index}")).collect();
    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file hello.py",
    );
    shell.conversation.scroll_up(100);

    let text = draw_to_text(&shell, &TerminalShellContext::from_session(&session));

    assert!(text.contains("elgar v0.10"));
    assert!(!text.contains("Review needed: action-1 CreateFile write hello.py"));
    assert!(text.contains("review action"));
    assert!(text.contains("File: hello.py"));
    assert!(!text.contains("Action: action-1 CreateFile"));
    assert!(text.contains("> "));
    assert!(text.contains("repo"));
}
