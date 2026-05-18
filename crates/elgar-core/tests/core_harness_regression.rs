use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    action::ActionLifecycleState,
    controller::Controller,
    event::{AssistantMessageSource, Event, VerifiedActionResult},
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
    session
        .events()
        .iter()
        .filter(|event| matches(event))
        .count()
}

fn provider_event_count(session: &Session) -> usize {
    event_count(session, |event| {
        matches!(
            event,
            Event::ProviderStarted(_) | Event::ProviderFinished(_)
        )
    })
}

fn controller_messages(session: &Session) -> Vec<&str> {
    session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller =>
            {
                Some(message.content.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn provider_response_is_suggestion_only_and_cannot_mutate_controller_truth() {
    let controller = Controller::default();
    let root = regression_root("provider-suggestion");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert!(session
        .events()
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
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
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
    assert!(session.actions().is_empty());
    assert_eq!(session.provider_metadata(), None);
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
        rejected_session.actions()[0].action.state,
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
        rejected_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(rejected_session.actions()[0].verified_result, None);
    assert!(rejected_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));
    assert!(rejected_session
        .events()
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
        approved_session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        approved_session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: approved_target.display().to_string()
        })
    );
    assert!(approved_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(approved_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let mut failed_session = session_at(&root);
    let failed_target = root.join("missing").join("failed.py");

    controller.turn(&mut failed_session, "create file missing/failed.py");
    controller.turn(&mut failed_session, "approve");

    assert!(!failed_target.exists());
    assert_eq!(
        failed_session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(failed_session.actions()[0].verified_result, None);
    assert!(failed_session.actions()[0].failure_reason.is_some());
    assert!(failed_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn second_write_file_proposal_is_blocked_while_one_action_is_pending() {
    let controller = Controller::default();
    let root = regression_root("single-pending-proposal");
    let mut session = session_at(&root);

    let first = controller.turn(&mut session, "create file first.py");
    let second = controller.turn(&mut session, "create file second.py");

    assert_eq!(first.route, Route::ProposeWriteFile);
    assert_eq!(second.route, Route::ProposeWriteFile);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        1
    );
    assert!(second
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionProposed(_))));
    assert!(controller_messages(&session).iter().any(|message| message
        .contains("Approve or reject it before requesting another WriteFile action")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approving_after_blocked_second_proposal_applies_only_original_action() {
    let controller = Controller::default();
    let root = regression_root("approve-original-after-blocked");
    let mut session = session_at(&root);
    let original = root.join("original.py");
    let blocked = root.join("blocked.py");

    controller.turn(&mut session, "create file original.py");
    controller.turn(&mut session, "create file blocked.py");
    controller.turn(&mut session, "approve");

    assert!(original.exists());
    assert!(!blocked.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: original.display().to_string()
        })
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        1
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_and_reject_with_no_pending_action_are_safe() {
    let controller = Controller::default();
    let root = regression_root("no-pending-actions");
    let mut session = session_at(&root);

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");

    assert!(session.actions().is_empty());
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionRejected(_)
                | Event::ActionApplied(_)
                | Event::ActionFailed(_)
        )),
        0
    );
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for approval.")));
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for rejection.")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn restored_session_with_multiple_proposed_actions_cannot_apply_hidden_actions() {
    let controller = Controller::default();
    let root = regression_root("ambiguous-restored-actions");
    let first = root.join("first.py");
    let second = root.join("second.py");
    let third = root.join("third.py");
    let root_string = root.display().to_string();
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "restored-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "WriteFile": {
                            "target_path": "first.py",
                            "contents": ""
                        }
                    },
                    "state": "Proposed",
                    "summary": "write first.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-2",
                    "request": {
                        "WriteFile": {
                            "target_path": "second.py",
                            "contents": ""
                        }
                    },
                    "state": "Proposed",
                    "summary": "write second.py"
                },
                "verified_result": null,
                "failure_reason": null
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "create file third.py");

    assert!(!first.exists());
    assert!(!second.exists());
    assert!(!third.exists());
    assert_eq!(session.actions().len(), 2);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Proposed));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        0
    );
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("Multiple proposed actions are waiting")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_action_remains_terminal_after_followup_approval_or_rejection() {
    let controller = Controller::default();
    let root = regression_root("rejected-terminal");
    let mut session = session_at(&root);
    let target = root.join("terminal.py");

    controller.turn(&mut session, "create file terminal.py");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");

    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionRejected(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_)
        )),
        0
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approving_write_file_existing_target_fails_without_overwriting() {
    let controller = Controller::default();
    let root = regression_root("existing-target");
    let mut session = session_at(&root);
    let target = root.join("existing.py");
    fs::write(&target, "original").unwrap();

    controller.turn(&mut session, "create file existing.py");
    controller.turn(&mut session, "approve");

    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("write target already exists")));
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionFailed(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        0
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn approved_write_file_symlink_escape_fails_without_verified_result() {
    use std::os::unix::fs::symlink;

    let controller = Controller::default();
    let root = regression_root("symlink-escape");
    let outside = root.parent().unwrap().join(format!(
        "elgar-core-regression-{}-symlink-outside",
        std::process::id()
    ));
    let outside_target = outside.join("escaped.py");
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    let mut session = session_at(&root);

    controller.turn(&mut session, "create file link/escaped.py");
    controller.turn(&mut session, "approve");

    assert!(!outside_target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("target parent resolves outside the allowed root")));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn controller_events_and_renderer_report_inspectable_action_states() {
    let controller = Controller::default();
    let root = regression_root("renderer");
    let mut rejected_session = session_at(&root);

    controller.turn(&mut rejected_session, "create file rejected.py");
    controller.turn(&mut rejected_session, "reject");

    assert!(rejected_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(rejected_session
        .events()
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
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(applied_session
        .events()
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
