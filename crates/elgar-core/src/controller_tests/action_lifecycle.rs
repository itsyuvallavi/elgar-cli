use super::*;

#[test]
fn proposed_write_file_turn_records_action_without_creating_file() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("proposed");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    let result = controller.turn(&mut session, "create hello.py");

    assert_eq!(result.route, Route::ProposeWriteFile);
    assert!(!path.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
}

#[test]
fn rejected_write_file_turn_does_not_create_file_and_is_terminal() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("rejected");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");

    assert!(!path.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));
}

#[test]
fn approved_write_file_turn_writes_target_and_records_verified_result() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("approved");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "approve");

    assert!(path.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: path.display().to_string()
        })
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_absolute_write_file_turn_fails_without_writing() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("absolute");
    let path = std::env::temp_dir().join(format!(
        "elgar-controller-{}-absolute.py",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    controller.turn(&mut session, &format!("create {}", path.display()));
    controller.turn(&mut session, "approve");

    assert!(!path.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("absolute paths are not allowed")));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_parent_traversal_write_file_turn_fails_without_writing() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("traversal");
    let outside = root.parent().unwrap().join(format!(
        "elgar-controller-{}-outside.py",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&outside);

    controller.turn(
        &mut session,
        &format!(
            "create ../{}",
            outside.file_name().unwrap().to_string_lossy()
        ),
    );
    controller.turn(&mut session, "approve");

    assert!(!outside.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("parent directory traversal is not allowed")));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_write_file_creates_missing_parent_and_records_verified_result() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("missing-parent");
    let path = root.join("missing").join("hello.py");

    controller.turn(&mut session, "create missing/hello.py");
    controller.turn(&mut session, "approve");

    assert!(path.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(session.actions()[0].verified_result.is_some());
    assert_eq!(session.actions()[0].failure_reason, None);
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn proposed_patch_file_turn_records_action_without_changing_file() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("proposed-patch");
    let path = root.join("notes.txt");
    std::fs::write(&path, "old contents").unwrap();

    let result = controller.turn(&mut session, "edit file notes.txt replace old with new");

    assert_eq!(result.route, Route::ProposePatchFile);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old contents");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_patch_file_turn_updates_target_and_records_verified_result() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("approved-patch");
    let path = root.join("notes.txt");
    std::fs::write(&path, "old contents").unwrap();

    controller.turn(&mut session, "edit file notes.txt replace old with new");
    controller.turn(&mut session, "approve");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new contents");
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FilePatched {
                path: path.display().to_string()
            }
        ))
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rejected_overwrite_file_turn_does_not_change_file() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("rejected-overwrite");
    let path = root.join("notes.txt");
    std::fs::write(&path, "original").unwrap();

    controller.turn(&mut session, "overwrite file notes.txt with replacement");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_overwrite_file_turn_replaces_target_and_records_verified_result() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("approved-overwrite");
    let path = root.join("notes.txt");
    std::fs::write(&path, "original").unwrap();

    let proposed = controller.turn(&mut session, "overwrite file notes.txt with replacement");
    controller.turn(&mut session, "approve");

    assert_eq!(proposed.route, Route::ProposeOverwriteFile);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FileOverwritten {
                path: path.display().to_string()
            }
        ))
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn provider_text_cannot_apply_existing_action_or_create_file() {
    let controller = Controller::default();
    let (mut session, _root) = rooted_session("provider");
    let path = session.project_root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "explain how to write the file");

    assert!(!path.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
}
