use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    action::ActionLifecycleState,
    controller::Controller,
    event::{Event, VerifiedActionResult},
    renderer::render_session,
    router::{route_input, Route},
    session::Session,
};

fn regression_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "elgar-core-regression-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("regression-session", root, root)
}

fn event_count(session: &Session, matches: impl Fn(&Event) -> bool) -> usize {
    session.events.iter().filter(|event| matches(event)).count()
}

#[test]
fn provider_response_is_suggestion_only_and_cannot_mutate_controller_truth() {
    let controller = Controller::default();
    let root = regression_root("provider-suggestion");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert!(session.actions.is_empty());
    assert!(session
        .events
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionProposed(_)
                | Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )),
        0
    );

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert_eq!(session.actions.len(), 1);
    assert_eq!(
        session.actions[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApproved(_))),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn router_classifies_create_file_and_unknown_input_is_safe() {
    let controller = Controller::default();
    let root = regression_root("router-unknown");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    assert_eq!(route_input("create file hello.py"), Route::ProposeWriteFile);
    assert_eq!(controller.turn(&mut session, "   ").route, Route::Unknown);
    assert_eq!(
        controller.turn(&mut session, "run ls").route,
        Route::Unknown
    );

    assert!(!target.exists());
    assert!(session.actions.is_empty());
    assert_eq!(session.provider_metadata, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ProviderStarted(_))),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ProviderFinished(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_file_lifecycle_requires_user_approval_and_records_terminal_states() {
    let controller = Controller::default();
    let root = regression_root("write-file-lifecycle");
    let mut rejected_session = session_at(&root);
    let rejected_target = root.join("rejected.py");

    let proposed = controller.turn(&mut rejected_session, "create file rejected.py");

    assert_eq!(proposed.route, Route::ProposeWriteFile);
    assert!(!rejected_target.exists());
    assert_eq!(
        rejected_session.actions[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert!(proposed
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));

    controller.turn(&mut rejected_session, "reject");
    controller.turn(&mut rejected_session, "approve");

    assert!(!rejected_target.exists());
    assert_eq!(
        rejected_session.actions[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(rejected_session.actions[0].verified_result, None);
    assert!(rejected_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));
    assert!(rejected_session
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let mut approved_session = session_at(&root);
    let approved_target = root.join("approved.py");
    let other_target = root.join("other.py");

    controller.turn(&mut approved_session, "create file approved.py");
    controller.turn(&mut approved_session, "approve");

    assert!(approved_target.exists());
    assert_eq!(fs::read_to_string(&approved_target).unwrap(), "");
    assert!(!other_target.exists());
    assert_eq!(
        approved_session.actions[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        approved_session.actions[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: approved_target.display().to_string()
        })
    );
    assert!(approved_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(approved_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let mut failed_session = session_at(&root);
    let failed_target = root.join("missing").join("failed.py");

    controller.turn(&mut failed_session, "create file missing/failed.py");
    controller.turn(&mut failed_session, "approve");

    assert!(!failed_target.exists());
    assert_eq!(
        failed_session.actions[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(failed_session.actions[0].verified_result, None);
    assert!(failed_session.actions[0].failure_reason.is_some());
    assert!(failed_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn controller_events_and_renderer_report_inspectable_action_states() {
    let controller = Controller::default();
    let root = regression_root("renderer");
    let mut rejected_session = session_at(&root);

    controller.turn(&mut rejected_session, "create file rejected.py");
    controller.turn(&mut rejected_session, "reject");

    assert!(rejected_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(rejected_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));

    let rejected_rendered = render_session(&rejected_session);
    assert!(rejected_rendered.contains("action proposed: action-1 WriteFile write rejected.py"));
    assert!(rejected_rendered.contains("action rejected: action-1 WriteFile write rejected.py"));

    let mut applied_session = session_at(&root);
    let applied_target = root.join("applied.py");

    controller.turn(&mut applied_session, "create file applied.py");
    controller.turn(&mut applied_session, "approve");

    assert!(applied_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(applied_session
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let applied_rendered = render_session(&applied_session);
    assert!(applied_rendered.contains("action proposed: action-1 WriteFile write applied.py"));
    assert!(applied_rendered.contains("action approved: action-1 WriteFile write applied.py"));
    assert!(applied_rendered.contains(&format!(
        "action applied: action-1 WriteFile file written: {}",
        applied_target.display()
    )));

    let _ = fs::remove_dir_all(root);
}
