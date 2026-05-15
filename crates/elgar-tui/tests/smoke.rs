use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    action::ActionLifecycleState,
    controller::Controller,
    event::{Event, VerifiedActionResult},
    provider::ProviderStub,
    router::Route,
    session::Session,
};
use elgar_tui::TuiShell;

fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-tui-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("tui-smoke-session", root, root)
}

#[test]
fn renders_initial_state() {
    let shell = TuiShell::new();
    let rendered = shell.render();

    assert!(rendered.contains("[Conversation]\n(empty conversation)"));
    assert!(rendered.contains("[Pending Action]\nnone"));
    assert!(rendered.contains("[Status]\nready"));
    assert!(rendered.contains("[Input]\n> "));
}

#[test]
fn renders_core_events_from_controller_turns() {
    let controller = Controller::default();
    let root = smoke_root("render-core-events");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what does the harness do?");
    shell.consume_events(&result.events);

    let rendered = shell.render();
    assert!(rendered.contains("user: what does the harness do?"));
    assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
    assert!(rendered.contains("provider finished: stub-provider request stub-request-1"));
    assert!(rendered.contains("assistant Provider: stub provider response"));
    assert!(rendered.contains("[Status]\nassistant message"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn submits_user_input_through_shared_controller() {
    let controller = Controller::new(ProviderStub::new("smoke-provider").with_model("smoke-model"));
    let root = smoke_root("submit-input");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = shell.submit_input(&controller, &mut session, "what does this do?");

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .map(|metadata| metadata.provider.as_str()),
        Some("smoke-provider")
    );
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .and_then(|metadata| metadata.model.as_deref()),
        Some("smoke-model")
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(shell.render().contains("user: what does this do?"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn displays_proposed_write_file_action_without_writing() {
    let controller = Controller::default();
    let root = smoke_root("proposed-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    let result = shell.submit_input(&controller, &mut session, "create file hello.py");

    assert_eq!(result.route, Route::ProposeWriteFile);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );

    let rendered = shell.render();
    assert!(rendered.contains("[Pending Action]\naction id: action-1"));
    assert!(rendered.contains("action type: WriteFile"));
    assert!(rendered.contains("target: hello.py"));
    assert!(rendered.contains("summary: write hello.py"));
    assert!(rendered.contains("state: pending approval"));
    assert!(rendered.contains("instructions: type approve to apply or reject to decline"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approves_write_file_through_controller_and_renders_verified_result() {
    let controller = Controller::default();
    let root = smoke_root("approve-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    shell.submit_input(&controller, &mut session, "create file hello.py");
    let result = shell.submit_approval(&controller, &mut session);

    assert_eq!(result.route, Route::ApproveAction);
    assert!(target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: target.display().to_string()
        })
    );

    let rendered = shell.render();
    assert!(rendered.contains("state: applied"));
    assert!(rendered.contains(&format!("result: file written: {}", target.display())));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_write_file_through_controller_without_writing() {
    let controller = Controller::default();
    let root = smoke_root("reject-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    shell.submit_input(&controller, &mut session, "create file hello.py");
    let result = shell.submit_rejection(&controller, &mut session);

    assert_eq!(result.route, Route::RejectAction);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let rendered = shell.render();
    assert!(rendered.contains("state: rejected"));
    assert!(rendered.contains("result: rejected by user; no filesystem change was made"));
    assert!(rendered.contains("rejected actions are terminal"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rendering_and_rejection_do_not_call_provider_or_mutate_files_directly() {
    let controller =
        Controller::new(ProviderStub::new("should-not-be-called").with_model("unused-model"));
    let root = smoke_root("surface-boundaries");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    let proposed = controller.turn(&mut session, "create file hello.py");
    shell.consume_events(&proposed.events);
    let before_render = session.clone();
    let rendered = shell.render();

    assert!(rendered.contains("state: pending approval"));
    assert_eq!(session, before_render);
    assert!(!target.exists());
    assert_eq!(session.provider_metadata(), None);
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ProviderStarted(_) | Event::ProviderFinished(_)
    )));

    let rejected = shell.submit_rejection(&controller, &mut session);

    assert_eq!(rejected.route, Route::RejectAction);
    assert!(!target.exists());
    assert_eq!(session.provider_metadata(), None);
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ProviderStarted(_) | Event::ProviderFinished(_)
    )));

    let _ = fs::remove_dir_all(root);
}
