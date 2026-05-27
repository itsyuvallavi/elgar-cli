use super::*;

fn push_proposed_action(session: &mut Session, request: ActionRequest, summary: &str) {
    let action_id = format!("action-{}", session.actions().len() + 1);
    session.push_action(ActionRecord::new(Action::proposed(
        action_id, request, summary,
    )));
}

fn propose_create_file(session: &mut Session, target_path: impl Into<PathBuf>, contents: &str) {
    push_proposed_action(
        session,
        ActionRequest::CreateFile(CreateFileAction {
            target_path: target_path.into(),
            contents: contents.to_string(),
        }),
        "create file",
    );
}

fn propose_patch_file(
    session: &mut Session,
    target_path: impl Into<PathBuf>,
    find: &str,
    replace: &str,
) {
    push_proposed_action(
        session,
        ActionRequest::PatchFile(PatchFileAction {
            target_path: target_path.into(),
            find: find.to_string(),
            replace: replace.to_string(),
        }),
        "patch file",
    );
}

fn propose_overwrite_file(session: &mut Session, target_path: impl Into<PathBuf>, contents: &str) {
    push_proposed_action(
        session,
        ActionRequest::OverwriteFile(OverwriteFileAction {
            target_path: target_path.into(),
            contents: contents.to_string(),
        }),
        "overwrite file",
    );
}

#[test]
fn typed_proposed_write_file_records_action_without_creating_file() {
    let (mut session, root) = rooted_session("proposed");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    propose_create_file(&mut session, "hello.py", "");

    assert!(!path.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rejected_write_file_action_does_not_create_file_and_is_terminal() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("rejected");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    propose_create_file(&mut session, "hello.py", "");
    let rejected = gate.reject(&mut session);
    let approved_after_rejection = gate.approve(&mut session);

    assert_eq!(rejected.route, Route::RejectAction);
    assert_eq!(approved_after_rejection.route, Route::ApproveAction);
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

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_write_file_action_writes_target_and_records_verified_result() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("approved");
    let path = root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    propose_create_file(&mut session, "hello.py", "");
    let approved = gate.approve(&mut session);

    assert_eq!(approved.route, Route::ApproveAction);
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
fn approved_absolute_write_file_action_fails_without_writing() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("absolute");
    let path = std::env::temp_dir().join(format!(
        "elgar-controller-{}-absolute.py",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    propose_create_file(&mut session, &path, "");
    gate.approve(&mut session);

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
fn approved_absolute_home_create_file_action_writes_and_records_verified_result() {
    let gate = ActionGate::default();
    let (mut session, project_root) = rooted_session("absolute-home-project");
    let home_root = std::env::temp_dir().join(format!(
        "elgar-controller-{}-absolute-home",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home_root);
    std::fs::create_dir_all(home_root.join("ElgarPermissionTest")).unwrap();
    let _home = EnvGuard::set_home(&home_root);
    let path = home_root.join("ElgarPermissionTest").join("test.txt");

    propose_create_file(&mut session, &path, "");
    gate.approve(&mut session);

    assert!(path.exists());
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
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(&home_root);
}

#[test]
fn approved_parent_traversal_write_file_action_fails_without_writing() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("traversal");
    let outside = root.parent().unwrap().join(format!(
        "elgar-controller-{}-outside.py",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&outside);

    propose_create_file(
        &mut session,
        format!("../{}", outside.file_name().unwrap().to_string_lossy()),
        "",
    );
    gate.approve(&mut session);

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
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("missing-parent");
    let path = root.join("missing").join("hello.py");

    propose_create_file(&mut session, "missing/hello.py", "");
    gate.approve(&mut session);

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
fn typed_proposed_patch_file_records_action_without_changing_file() {
    let (mut session, root) = rooted_session("proposed-patch");
    let path = root.join("notes.txt");
    std::fs::write(&path, "old contents").unwrap();

    propose_patch_file(&mut session, "notes.txt", "old", "new");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old contents");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approved_patch_file_action_updates_target_and_records_verified_result() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("approved-patch");
    let path = root.join("notes.txt");
    std::fs::write(&path, "old contents").unwrap();

    propose_patch_file(&mut session, "notes.txt", "old", "new");
    gate.approve(&mut session);

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
fn rejected_overwrite_file_action_does_not_change_file() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("rejected-overwrite");
    let path = root.join("notes.txt");
    std::fs::write(&path, "original").unwrap();

    propose_overwrite_file(&mut session, "notes.txt", "replacement");
    gate.reject(&mut session);
    gate.approve(&mut session);

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
fn approved_overwrite_file_action_replaces_target_and_records_verified_result() {
    let gate = ActionGate::default();
    let (mut session, root) = rooted_session("approved-overwrite");
    let path = root.join("notes.txt");
    std::fs::write(&path, "original").unwrap();

    propose_overwrite_file(&mut session, "notes.txt", "replacement");
    gate.approve(&mut session);

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
    let (mut session, root) = rooted_session("provider");
    let path = session.project_root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    propose_create_file(&mut session, "hello.py", "");
    controller.model_turn(&mut session, "explain how to write the file");

    assert!(!path.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let _ = std::fs::remove_dir_all(&root);
}
