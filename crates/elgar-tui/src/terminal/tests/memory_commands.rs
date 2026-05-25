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
fn terminal_memory_command_reports_verified_project_state() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-project");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called src",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(!handle_submitted_terminal_input_for_loop(
        "create file project-plan.md",
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
fn terminal_memory_command_reports_latest_provider_prompt_memory_trace() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-provider-trace");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called workspace",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "where did you put that folder?",
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
