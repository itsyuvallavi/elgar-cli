use super::*;

#[test]
fn terminal_memory_command_is_local_and_empty_without_provider_call() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-empty");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_none());
    let rendered = shell.render();
    assert!(rendered.contains("Memory\n(empty)"));
    assert!(!rendered.contains("stub provider response"));
    assert!(!rendered.contains("lm-studio"));
    assert!(!rendered.contains("Input was not recognized"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_state_commands_are_local_and_empty_without_provider_call() {
    let controller = Controller::default();
    let root = temp_root("terminal-state-empty");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    for command in ["/state", "/status", "/pending", "/created", "/plan"] {
        assert!(!handle_submitted_terminal_input_for_loop(
            command,
            &controller,
            &mut session,
            &mut shell,
            &mut pending_turn,
        ));
        assert!(pending_turn.is_none());
    }

    let rendered = shell.render();
    assert!(rendered.contains("State\npending: none\napplied actions: 0\ncreated: (none)"));
    assert!(rendered.contains("memory: (none)"));
    assert!(rendered.contains("Status\nactions: 0\npending: none"));
    assert!(rendered.contains("Pending\nnone"));
    assert!(rendered.contains("Created\n(none)"));
    assert!(rendered.contains("Plan Preview\n(none)"));
    assert!(!rendered.contains("stub provider response"));
    assert!(!rendered.contains("lm-studio"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_pending_command_reports_pending_action_without_plain_words() {
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-pending",
        "pending.py",
        "",
    )]);
    let root = temp_root("terminal-pending-state");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    submit_review_tool_input(
        &mut shell,
        &controller,
        &mut session,
        "create file pending.py",
    );

    assert!(!handle_submitted_terminal_input_for_loop(
        "/pending",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("Pending\ncreate_file action-1 at pending.py"));
    assert!(rendered.contains("write pending.py"));
    assert!(!root.join("pending.py").exists());
    assert!(pending_turn.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_memory_command_reports_verified_project_state() {
    let controller = scripted_tool_controller(vec![
        scripted_create_directory_output("create-src", "src"),
        scripted_create_file_output("create-plan", "project-plan.md", "# Plan\n"),
    ]);
    let root = temp_root("terminal-memory-project");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called src",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create file project-plan.md",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(root.join("project-plan.md").is_file());
    assert!(pending_turn.is_none());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("Memory"));
    assert!(rendered.contains("folders\n- ok "));
    assert!(rendered.contains("plans\n- ok "));
    assert!(!rendered.contains("lm-studio"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_plan_preview_command_reports_structured_plan_state() {
    let plan =
        "# Plan\n\n```text\nDemoApp/\n├── src/\n│   └── main.py\n└── requirements.txt\n```\n";
    let controller = scripted_tool_controller(vec![scripted_create_file_output(
        "create-plan",
        "DemoApp/project-plan.md",
        plan,
    )]);
    let root = temp_root("terminal-plan-preview");
    std::fs::create_dir_all(root.join("DemoApp")).unwrap();
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create project plan",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(session.project_memory().latest_structured_plan().is_some());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/plan preview",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("Plan Preview"));
    assert!(rendered.contains("status: verified"));
    assert!(rendered.contains("stage: verified-plan"));
    assert!(rendered.contains("source action: action-1"));
    assert!(rendered.contains("plan: DemoApp/project-plan.md"));
    assert!(rendered.contains("root: DemoApp"));
    assert!(rendered.contains("directories: 1/2 present"));
    assert!(rendered.contains("files: 0/2 present"));
    assert!(rendered.contains("- missing DemoApp/src/main.py"));
    assert!(rendered.contains("contract review:"));
    assert!(rendered.contains("- approvable: no"));
    assert!(rendered.contains("missing Verification section"));
    assert!(rendered.contains("missing Acceptance Criteria section"));
    assert!(!rendered.contains("stub provider response"));
    assert!(pending_turn.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_created_command_reports_verified_creations() {
    let controller =
        scripted_tool_controller(vec![scripted_create_directory_output("create-src", "src")]);
    let root = temp_root("terminal-created-state");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called src",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "/created",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("Created\n- directory src"));
    assert!(root.join("src").is_dir());
    assert!(pending_turn.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_state_command_reports_verified_creations_and_memory() {
    let controller = scripted_tool_controller(vec![
        scripted_create_directory_output("create-app", "tui-capability-test"),
        scripted_create_file_output(
            "create-plan",
            "tui-capability-test/PROJECT_PLAN.md",
            "# Project Plan\n\n```text\ntui-capability-test/\n├── src/\n│   └── main.py\n└── requirements.txt\n```\n",
        ),
    ]);
    let root = temp_root("terminal-state-verified-memory");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called tui-capability-test",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create project plan",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "/state",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("State"));
    assert!(rendered.contains("applied actions: 2"));
    assert!(rendered.contains("created:\n- directory tui-capability-test"));
    assert!(rendered.contains("- file tui-capability-test/PROJECT_PLAN.md"));
    assert!(rendered.contains("verified folders:"));
    assert!(rendered.contains("- ok tui-capability-test ("));
    assert!(rendered.contains("verified plans:"));
    assert!(rendered.contains("- ok tui-capability-test/PROJECT_PLAN.md ("));
    assert!(rendered.contains("latest structured plan:"));
    assert!(rendered.contains("files 0/2"));
    assert!(!rendered.contains("stub provider response"));
    assert!(pending_turn.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_memory_command_reports_latest_provider_prompt_memory_trace() {
    let controller = scripted_tool_controller(vec![
        scripted_create_directory_output("create-workspace", "workspace"),
        scripted_create_file_output("create-plan", "workspace/project-plan.md", "# Plan\n"),
    ]);
    let root = temp_root("terminal-memory-provider-trace");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called workspace",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create a plan in that folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    let completed = wait_for_completed_provider_turn(&pending_turn.take().unwrap());
    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert!(session.latest_provider_prompt_memory_selection().is_some());
    assert!(!handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("provider prompt memory"));
    assert!(rendered.contains("selected"));
    assert!(rendered.contains("verified folder ok "));
    assert!(!rendered.contains("Verified memory selected by Elgar controller:"));
    assert!(!rendered.contains("User request:"));

    let _ = std::fs::remove_dir_all(root);
}
