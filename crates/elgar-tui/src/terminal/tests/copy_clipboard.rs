use super::*;

#[test]
fn terminal_copy_prefers_system_clipboard_without_terminal_escape() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = vec![
        "first visible line".to_string(),
        "older scrolled line".to_string(),
    ];
    let mut output = Vec::new();

    copy_conversation_with_clipboards(&mut output, &mut shell, |text| {
        assert_eq!(text, "first visible line\nolder scrolled line");
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert_eq!(shell.copy.render_hint(), "copied conversation (38 bytes)");
}

#[test]
fn terminal_copy_omits_provider_tool_planning_thinking() {
    let mut shell = TuiShell::new();
    shell.consume_event(&Event::UserMessage(elgar_core::event::UserMessage::new(
        "Create a folder on my Desktop called ElgarLiveE2E",
    )));
    shell.consume_event(&Event::ProviderStarted(ProviderStarted::new(
        "stub-provider",
        "request-1",
    )));
    shell.consume_event(&Event::ProviderFinished(ProviderFinished::new(
        "stub-provider",
        "request-1",
        ProviderOutput::new("Done.").with_thinking(
            "Create directory on Desktop.\n\
             Create file plan.md in that directory.\n\
             Create files per plan: package.json, tsconfig.json, vite.config.ts maybe...\n\
             Create files. We don't have content. Should we ask guidance? Probably need to create files with...\n\
             Call create_file for each target_path with contents. Provide minimal starter files.",
        ),
    )));
    shell.consume_event(&Event::AssistantMessage(AssistantMessage::new(
        "Done.",
        AssistantMessageSource::Provider,
    )));
    shell.consume_event(&Event::ActionApplied(
        elgar_core::event::ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            elgar_core::event::VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "Desktop/ElgarLiveE2E".to_string(),
                },
            ),
        ),
    ));
    let mut copied = String::new();
    let mut output = Vec::new();

    copy_conversation_with_clipboards(&mut output, &mut shell, |text| {
        copied = text.to_string();
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert!(copied.contains("> Create a folder on my Desktop called ElgarLiveE2E"));
    assert!(copied.contains("Done."));
    assert!(copied.contains("Created Desktop/ElgarLiveE2E."));
    assert!(!copied.contains("Create directory on Desktop."));
    assert!(!copied.contains("Create file plan.md in that directory."));
    assert!(!copied.contains("Create files per plan"));
    assert!(!copied.contains("We don't have content"));
    assert!(!copied.contains("Should we ask guidance"));
    assert!(!copied.contains("Probably need to create files"));
    assert!(!copied.contains("Call create_file for each target_path"));
    assert!(!copied.contains("Provide minimal starter files"));
    assert!(!copied.contains("Use create_file"));
    assert!(!copied.contains("Provide tool calls"));
    assert!(shell.copy.render_hint().starts_with("copied conversation"));
}

#[test]
fn terminal_copy_marks_policy_auto_create_without_manual_approval_copy() {
    let mut shell = TuiShell::new();
    shell.consume_event(&Event::UserMessage(elgar_core::event::UserMessage::new(
        "create folder called copied-policy",
    )));
    shell.consume_event(&Event::ActionApproved(
        elgar_core::event::ActionEvent::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            "create directory copied-policy",
        )
        .with_target("copied-policy")
        .with_approval_source(elgar_core::policy::ApprovalSource::policy(
            elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
            "validated new directory create",
        )),
    ));
    shell.consume_event(&Event::ActionApplied(
        elgar_core::event::ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            elgar_core::event::VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "copied-policy".to_string(),
                },
            ),
        ),
    ));
    let mut copied = String::new();
    let mut output = Vec::new();

    copy_conversation_with_clipboards(&mut output, &mut shell, |text| {
        copied = text.to_string();
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert!(copied.contains("> create folder called copied-policy"));
    assert!(copied.contains("Created copied-policy."));
    assert!(!copied.contains("Approved."));
    assert!(!copied.contains("Creating copied-policy."));
    assert!(!copied.contains("Approve to"));
}

#[test]
fn terminal_copy_raw_details_preserves_shell_truth_hidden_from_default_copy() {
    let mut shell = TuiShell::new();
    shell.consume_event(&Event::ActionApplied(
        elgar_core::event::ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::ShellCommand,
            elgar_core::event::VerifiedActionResult::Shell(
                elgar_core::event::ShellActionVerification {
                    command: "printf 'hello\\n'".to_string(),
                    cwd: "/repo".to_string(),
                    stdout: "hello\n".to_string(),
                    stderr: "warn\n".to_string(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    exit_code: Some(0),
                    elapsed_millis: 12,
                    timed_out: false,
                    verified_effect: None,
                },
            ),
        ),
    ));
    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("stdout hidden"));
    assert!(rendered.contains("stderr hidden"));
    assert!(!rendered.contains("Command: printf"));
    assert!(!rendered.contains("Cwd: /repo"));
    assert!(!rendered.contains("stdout:\nhello"));
    assert!(!rendered.contains("stderr:\nwarn"));

    let mut copied = String::new();
    let mut output = Vec::new();
    copy_raw_details_with_clipboards(&mut output, &mut shell, |text| {
        copied = text.to_string();
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert!(copied.contains("Command: printf 'hello\\n'"));
    assert!(copied.contains("Cwd: /repo"));
    assert!(copied.contains("Exit code: 0"));
    assert!(copied.contains("Elapsed: 12ms"));
    assert!(copied.contains("stdout:\nhello"));
    assert!(copied.contains("stderr:\nwarn"));
    assert!(shell.copy.render_hint().starts_with("copied raw details"));
}

#[test]
fn terminal_copy_raw_details_preserves_collapsed_assistant_markdown() {
    let mut shell = TuiShell::new();
    let code = (1..=90)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let markdown = format!("Large block:\n```text\n{code}\n```");

    shell.consume_event(&Event::AssistantMessage(AssistantMessage::new(
        markdown,
        AssistantMessageSource::Provider,
    )));

    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("╭─ code (text) · 90 lines · collapsed, showing 40"));
    assert!(rendered.contains("line-040"));
    assert!(!rendered.contains("line-090"));
    assert!(!shell.conversation_copy_text().contains("line-090"));

    let mut copied = String::new();
    let mut output = Vec::new();
    copy_raw_details_with_clipboards(&mut output, &mut shell, |text| {
        copied = text.to_string();
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert!(copied.contains("Assistant message details"));
    assert!(copied.contains("```text"));
    assert!(copied.contains("line-090"));
    assert!(shell.copy.render_hint().starts_with("copied raw details"));
}

#[test]
fn terminal_details_last_appends_collapsed_assistant_raw_markdown() {
    let runtime = AgentRuntime::default();
    let action_gate = ActionGate::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let code = (1..=90)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let markdown = format!("Large block:\n```text\n{code}\n```");

    shell.consume_event(&Event::AssistantMessage(AssistantMessage::new(
        markdown,
        AssistantMessageSource::Provider,
    )));
    handle_inline_submission(
        "/details last",
        &runtime,
        &action_gate,
        &mut session,
        &mut shell,
    )
    .unwrap();

    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("... 50 lines hidden; use /details last or /copy raw"));
    assert!(rendered.contains("Assistant message details"));
    assert!(rendered.contains("```text"));
    assert!(rendered.contains("line-090"));
}

#[test]
fn terminal_details_last_appends_latest_raw_shell_details_on_request() {
    let runtime = AgentRuntime::default();
    let action_gate = ActionGate::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    shell.consume_event(&Event::ActionApplied(
        elgar_core::event::ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::ShellCommand,
            elgar_core::event::VerifiedActionResult::Shell(
                elgar_core::event::ShellActionVerification {
                    command: "printf 'hello\\n'".to_string(),
                    cwd: "/repo".to_string(),
                    stdout: "hello\n".to_string(),
                    stderr: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    exit_code: Some(0),
                    elapsed_millis: 12,
                    timed_out: false,
                    verified_effect: None,
                },
            ),
        ),
    ));

    handle_inline_submission(
        "/details last",
        &runtime,
        &action_gate,
        &mut session,
        &mut shell,
    )
    .unwrap();

    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("details: /details last or /copy raw"));
    assert!(rendered.contains("Shell result details"));
    assert!(rendered.contains("Command: printf 'hello\\n'"));
    assert!(rendered.contains("stdout:\nhello"));
}

#[test]
fn terminal_copy_uses_osc52_for_full_rendered_conversation() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = vec![
        "first visible line".to_string(),
        "older scrolled line".to_string(),
    ];
    let mut output = Vec::new();

    copy_conversation_to_terminal_clipboard(&mut output, &mut shell).unwrap();

    let copied = String::from_utf8(output).unwrap();
    assert_eq!(
        copied,
        osc52_clipboard_sequence("first visible line\nolder scrolled line")
    );
    assert_eq!(shell.copy.render_hint(), "copied conversation (38 bytes)");
}

#[test]
fn terminal_copy_reports_failure_when_system_and_terminal_clipboards_fail() {
    struct FailingWriter;

    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("terminal rejected OSC 52"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut shell = TuiShell::new();
    shell.conversation.lines = vec!["copy target".to_string()];

    let error = copy_conversation_with_clipboards(FailingWriter, &mut shell, |_text| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "pbcopy missing",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("pbcopy missing"));
    assert!(error.to_string().contains("terminal rejected OSC 52"));
    assert!(shell
        .copy
        .render_hint()
        .contains("system clipboard failed: pbcopy missing"));
}

#[cfg(unix)]
#[test]
fn terminal_system_clipboard_command_has_timeout() {
    let started = std::time::Instant::now();

    let error = copy_text_with_command_and_args(
        "/bin/sh",
        &["-c", "cat >/dev/null; sleep 5"],
        "copy target",
        std::time::Duration::from_millis(50),
    )
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn terminal_copy_slash_command_does_not_change_controller_or_scroll_state() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let before_session = session.clone();
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);
    let mut input = TerminalInput::default();

    let mut output = Vec::new();
    for character in "/copy".chars() {
        let exited = handle_terminal_key_with_copy_writer(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
            &mut output,
        );
        assert!(!exited);
    }

    let exited = handle_terminal_key_with_copy_writer(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
        &mut output,
    );

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(input.text(), "");
    assert_eq!(shell.conversation.scroll_offset(4), 1);
    assert!(shell.copy.render_hint().starts_with("copied conversation"));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        osc52_clipboard_sequence(&shell.conversation_copy_text())
    );
}

#[test]
fn inline_plan_preview_is_recorded_for_copy() {
    let root = temp_root("inline-plan-copy");
    std::fs::create_dir_all(root.join("DemoApp")).unwrap();
    let plan = "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Run the smoke check.\n\n## Acceptance Criteria\n- Expected files exist.\n";
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-plan",
        "DemoApp/plan.md",
        plan,
    )]);
    let runtime = AgentRuntime::new(controller.provider.clone());
    let action_gate = ActionGate::new(controller.provider.clone());
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();

    let result = runtime.tool_turn(
        &mut session,
        "create only the project plan",
        PermissionPolicyMode::FullAccess,
    );
    shell.consume_events(&result.events);
    handle_inline_submission("/plan", &runtime, &action_gate, &mut session, &mut shell).unwrap();

    let copied = shell.conversation_copy_text();
    assert!(copied.contains("Wrote "));
    assert!(copied.contains("Plan Preview"));
    assert!(copied.contains("review: draft · approvable yes"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_clipboard_encoding_is_standard_base64() {
    assert_eq!(encode_base64(b""), "");
    assert_eq!(encode_base64(b"f"), "Zg==");
    assert_eq!(encode_base64(b"fo"), "Zm8=");
    assert_eq!(encode_base64(b"foo"), "Zm9v");
    assert_eq!(
        osc52_clipboard_sequence("copy me"),
        "\x1b]52;c;Y29weSBtZQ==\x07"
    );
}
