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
