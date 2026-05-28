use super::*;

#[test]
fn terminal_shell_exit_keys_are_minimal() {
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE
    )));
    assert!(should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL
    )));
    assert!(should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL
    )));
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE
    )));
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('q'),
        crossterm::event::KeyModifiers::NONE
    )));
}

#[test]
fn terminal_commands_are_slash_only() {
    assert_eq!(parse_terminal_command("/help"), TerminalCommand::Help);
    assert_eq!(parse_terminal_command(" /commands "), TerminalCommand::Help);
    assert_eq!(parse_terminal_command("/clear"), TerminalCommand::Clear);
    assert_eq!(parse_terminal_command(" /new "), TerminalCommand::Clear);
    assert_eq!(parse_terminal_command("/approve"), TerminalCommand::Approve);
    assert_eq!(parse_terminal_command("/reject"), TerminalCommand::Reject);
    assert_eq!(parse_terminal_command("/cancel"), TerminalCommand::Cancel);
    assert_eq!(parse_terminal_command("/state"), TerminalCommand::State);
    assert_eq!(parse_terminal_command("/status"), TerminalCommand::Status);
    assert_eq!(parse_terminal_command("/pending"), TerminalCommand::Pending);
    assert_eq!(parse_terminal_command("/created"), TerminalCommand::Created);
    assert_eq!(parse_terminal_command("/memory"), TerminalCommand::Memory);
    assert_eq!(
        parse_terminal_command("/plan"),
        TerminalCommand::PlanPreview
    );
    assert_eq!(
        parse_terminal_command("/plan preview"),
        TerminalCommand::PlanPreview
    );
    assert_eq!(parse_terminal_command("/copy"), TerminalCommand::Copy);
    assert_eq!(parse_terminal_command("/exit"), TerminalCommand::Exit);
    assert_eq!(parse_terminal_command("/quit"), TerminalCommand::Exit);
    assert_eq!(parse_terminal_command("/q"), TerminalCommand::Exit);
    assert_eq!(
        parse_terminal_command("/model"),
        TerminalCommand::Unknown("/model")
    );
    assert_eq!(
        parse_terminal_command("clear"),
        TerminalCommand::Text("clear")
    );
    assert_eq!(
        parse_terminal_command("plan"),
        TerminalCommand::Text("plan")
    );
    assert_eq!(
        parse_terminal_command("state"),
        TerminalCommand::Text("state")
    );
    assert_eq!(
        parse_terminal_command("what did you create?"),
        TerminalCommand::Text("what did you create?")
    );
    assert_eq!(
        parse_terminal_command("preview plan"),
        TerminalCommand::Text("preview plan")
    );
    assert_eq!(parse_terminal_command("new"), TerminalCommand::Text("new"));
    assert_eq!(parse_terminal_command("q"), TerminalCommand::Text("q"));
    assert_eq!(
        parse_terminal_command("quit"),
        TerminalCommand::Text("quit")
    );
    assert_eq!(
        parse_terminal_command("approve"),
        TerminalCommand::Text("approve")
    );
    assert_eq!(
        parse_terminal_command("reject"),
        TerminalCommand::Text("reject")
    );

    let help = render_terminal_help();
    assert!(help.starts_with("Commands\n/commands"));
    assert!(help.contains("/clear"));
    assert!(help.contains("/new"));
    assert!(help.contains("/approve"));
    assert!(help.contains("/reject"));
    assert!(help.contains("/cancel"));
    assert!(help.contains("/state"));
    assert!(help.contains("/status"));
    assert!(help.contains("/pending"));
    assert!(help.contains("/created"));
    assert!(help.contains("/memory"));
    assert!(help.contains("/plan"));
    assert!(help.contains("/plan preview"));
    assert!(help.contains("/copy"));
    assert!(help.contains("/exit"));
    assert!(help.contains("/quit"));
    assert!(help.contains("/q"));
    assert!(help.contains("/help"));
    assert!(!help.contains("/model"));
    assert!(!help.contains("/settings"));
    assert!(!help.contains("/bash"));
    assert!(!help.contains("/api"));
}

#[test]
fn terminal_plain_approval_words_go_to_model_path() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-approved",
        "approved.py",
        "",
    )]);
    let root = temp_root("terminal-plain-approval-blocked");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file approved.py",
    );
    let before_action_count = session.actions().len();

    let exited = submit_text("approve", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert!(!target.exists());
    assert_eq!(session.actions().len(), before_action_count);
    assert!(shell.render().contains("> approve"));
    assert!(shell.render().contains("scripted provider response"));
    assert!(!shell
        .render()
        .contains("Action commands must use /approve or /reject."));
    assert!(input.text().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_clear_slash_commands_clear_only_local_conversation() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-clear",
        "clear.py",
        "",
    )]);
    let root = temp_root("terminal-clear-local");
    let target = root.join("clear.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file clear.py",
    );
    let before_session = session.clone();
    let before_pending = shell.pending_action.clone();

    let exited = submit_text("/clear", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(shell.pending_action, before_pending);
    assert!(shell.conversation.lines.is_empty());
    assert!(!target.exists());
    assert!(input.text().is_empty());

    shell.conversation.lines.push("visible again".to_string());
    let exited = submit_text("/new", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(shell.pending_action, before_pending);
    assert!(shell.conversation.lines.is_empty());
    assert!(!target.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_provider_active_enter_preserves_non_cancel_draft() {
    let mut input = TerminalInput::default();

    for character in "keep this draft".chars() {
        assert_eq!(
            super::super::handle_active_provider_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut input,
            ),
            super::super::ActiveProviderKeyAction::Continue
        );
    }

    assert_eq!(input.text(), "keep this draft");
    assert_eq!(
        super::super::handle_active_provider_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
        ),
        super::super::ActiveProviderKeyAction::Continue
    );
    assert_eq!(input.text(), "keep this draft");
}

#[test]
fn terminal_paste_event_inserts_multiline_text_without_submit() {
    let mut input = TerminalInput::default();

    assert_eq!(
        super::super::handle_terminal_input_event(
            crossterm::event::Event::Paste("first line\nsecond line".to_string()),
            &mut input,
        ),
        TerminalInputAction::Continue
    );

    assert_eq!(input.text(), "first line\nsecond line");
}

#[test]
fn terminal_provider_active_paste_preserves_multiline_draft() {
    let mut input = TerminalInput::default();

    assert_eq!(
        super::super::handle_active_provider_input_event(
            crossterm::event::Event::Paste("/cancel\nactually keep drafting".to_string()),
            &mut input,
        ),
        super::super::ActiveProviderKeyAction::Continue
    );

    assert_eq!(input.text(), "/cancel\nactually keep drafting");
}

#[test]
fn terminal_provider_active_enter_consumes_cancel_command() {
    let mut input = TerminalInput::default();

    for character in "/cancel".chars() {
        assert_eq!(
            super::super::handle_active_provider_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut input,
            ),
            super::super::ActiveProviderKeyAction::Continue
        );
    }

    assert_eq!(
        super::super::handle_active_provider_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
        ),
        super::super::ActiveProviderKeyAction::Cancel
    );
    assert_eq!(input.text(), "");
}

#[test]
fn terminal_page_keys_update_only_ui_scrollback() {
    let session = Session::new("session-1", "/repo", "/repo");
    let before_session = session.clone();
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    let before_lines = shell.conversation.lines.clone();

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut shell,
    ));
    assert_eq!(shell.conversation.scroll_offset(4), 1);

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut shell,
    ));
    assert_eq!(shell.conversation.scroll_offset(4), 6);

    assert_eq!(session, before_session);
    assert_eq!(shell.conversation.lines, before_lines);
    assert!(session.events().is_empty());
}

#[test]
fn terminal_plain_end_edits_input_instead_of_following_latest() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);
    let mut input = TerminalInput::default();

    for code in [
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyCode::End,
        crossterm::event::KeyCode::Char('d'),
    ] {
        handle_terminal_key(
            crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );
    }

    assert_eq!(input.text(), "acd");
    assert_eq!(shell.conversation.scroll_offset(4), 1);
}

#[test]
fn terminal_ctrl_end_follows_latest() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::CONTROL,
        ),
        &mut shell,
    ));

    assert_eq!(shell.conversation.scroll_offset(4), 6);
}

#[test]
fn terminal_enter_submits_input_through_controller_backed_shell() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    for character in "what does the harness do?".chars() {
        let exited = handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );
        assert!(!exited);
    }

    let exited = handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(input.text().is_empty());
    assert!(shell.render().contains("> what does the harness do?"));
    assert!(!shell.render().contains("User\n"));
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Model:"));
    assert_eq!(session.events().len(), 4);
}

#[test]
fn terminal_greeting_uses_stub_chat_with_live_path_guidance() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    let exited = submit_text("hello!", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    let rendered = shell.render();
    assert!(rendered.contains("> hello!"));
    assert!(!rendered.contains("User\n"));
    assert!(!rendered.contains("Model:"));
    assert!(rendered.contains("stub provider response (no-network) to: hello!"));
    assert!(rendered.contains("No live provider call was made"));
    assert!(!rendered.contains("Input was not recognized"));
    assert!(session.actions().is_empty());
}

#[test]
fn terminal_enter_ignores_empty_input_without_controller_turn() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );
    let exited = handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(session.events().is_empty());
    assert!(input.text().is_empty());
}

#[test]
fn terminal_approve_slash_command_approves_pending_action_through_shell() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-approved",
        "approved.py",
        "",
    )]);
    let root = temp_root("terminal-slash-approve");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file approved.py",
    );
    assert!(!target.exists());

    let exited = submit_text(
        "/approve",
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(shell.render().contains("Status: applied and verified"));
    assert!(shell.render().contains("Result: Wrote"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_reject_slash_command_rejects_pending_action_through_shell() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-rejected",
        "rejected.py",
        "",
    )]);
    let root = temp_root("terminal-slash-reject");
    let target = root.join("rejected.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file rejected.py",
    );

    let exited = submit_text("/reject", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert!(shell.render().contains("Status: rejected"));
    assert!(shell
        .render()
        .contains("Result: Rejected. No file was changed."));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_approval_slash_commands_show_no_pending_feedback() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_text(
        "/approve",
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );
    submit_text("/reject", &mut input, &controller, &mut session, &mut shell);

    let rendered = shell.render();
    assert!(rendered.contains("No proposed action is waiting for approval."));
    assert!(rendered.contains("No proposed action is waiting for rejection."));
    assert!(input.text().is_empty());
    assert!(session.actions().is_empty());
}

#[test]
fn terminal_function_keys_and_ctrl_y_are_not_command_actions() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-approved",
        "approved.py",
        "",
    )]);
    let root = temp_root("terminal-no-key-commands");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file approved.py",
    );
    let before_session = session.clone();

    for key in [
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(5),
            crossterm::event::KeyModifiers::NONE,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(6),
            crossterm::event::KeyModifiers::NONE,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        ),
    ] {
        let exited = handle_terminal_key(key, &mut input, &controller, &mut session, &mut shell);
        assert!(!exited);
    }

    assert!(!target.exists());
    assert_eq!(session, before_session);
    assert_eq!(input.text(), "q");
    assert_eq!(shell.copy.render_hint(), "");

    let _ = std::fs::remove_dir_all(root);
}
