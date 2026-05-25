use super::*;

#[test]
fn model_first_create_file_auto_create_policy_writes_and_verifies_without_approve() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-file",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({ "target_path": "model-first.txt", "contents": "created by policy" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-create-file");

    let result = controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create model-first.txt",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let path = root.join("model-first.txt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "created by policy");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(matches!(
        &session.actions()[0].action.request,
        ActionRequest::CreateFile(_)
    ));
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: path.display().to_string()
        })
    );
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.is_policy_approved()));
    assert!(result.events.iter().any(|event| matches!(
        event,
        Event::ActionApproved(action)
            if action
                .approval_source
                .as_ref()
                .is_some_and(ApprovalSource::is_policy)
    )));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));
    assert!(result
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionProposed(_))));
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_create_directory_auto_create_policy_creates_and_verifies_without_approve() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "model-first-dir" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-create-dir");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create model-first-dir",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let path = root.join("model-first-dir");
    assert!(path.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(matches!(
        &session.actions()[0].action.request,
        ActionRequest::CreateDirectory(_)
    ));
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated {
                path: path.display().to_string()
            }
        ))
    );
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.is_policy_approved()));
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_multiple_safe_create_tool_calls_auto_apply_and_verify_all() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-create-dir",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app" }),
            ),
            raw_model_tool_call(
                "call-create-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app/src" }),
            ),
            raw_model_tool_call(
                "call-create-index",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/index.html", "contents": "<div id=\"root\"></div>\n" }),
            ),
            raw_model_tool_call(
                "call-create-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-safe-create-batch");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a static React starter",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(root.join("app").is_dir());
    assert!(root.join("app/src").is_dir());
    assert_eq!(
        std::fs::read_to_string(root.join("app/index.html")).unwrap(),
        "<div id=\"root\"></div>\n"
    );
    assert!(root.join("app/src/App.tsx").is_file());
    assert_eq!(session.actions().len(), 4);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    assert!(session
        .actions()
        .iter()
        .all(|record| record.verified_result.is_some()));
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_create_file_review_all_still_proposes_only() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-file",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({ "target_path": "review-all.txt", "contents": "draft only" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-review-all-create-file");

    let result = controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create review-all.txt",
        PermissionPolicyMode::ReviewAll,
    );

    assert!(!root.join("review-all.txt").exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.user_approval_required));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(result
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_auto_create_existing_file_does_not_overwrite_or_succeed_silently() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-file",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({ "target_path": "existing.txt", "contents": "new contents" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-existing-create-file");
    let path = root.join("existing.txt");
    std::fs::write(&path, "original contents").unwrap();

    let result = controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create existing.txt",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original contents");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0].failure_reason.is_some());
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.is_policy_approved()));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));
    assert!(result
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_batch_existing_file_conflict_records_partial_truth_without_overwrite() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-existing",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "existing.txt", "contents": "new contents" }),
        ),
        raw_model_tool_call(
            "call-new",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "new.txt", "contents": "new file" }),
        ),
    ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-existing-batch");
    let existing = root.join("existing.txt");
    std::fs::write(&existing, "original contents").unwrap();

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create the files",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(
        std::fs::read_to_string(&existing).unwrap(),
        "original contents"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "new file"
    );
    assert_eq!(session.actions().len(), 2);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert!(session.actions()[0].failure_reason.is_some());
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );
    assert!(session.actions()[1].verified_result.is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_shell_command_stays_review_gated_in_auto_create_policy() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-shell",
        RawModelToolName::Known(ModelToolName::ShellCommand),
        json!({ "command": "touch shell-created.txt", "cwd": "." }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-shell");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "run touch shell-created.txt",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!root.join("shell-created.txt").exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert!(matches!(
        &session.actions()[0].action.request,
        ActionRequest::ShellCommand(_)
    ));
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.user_approval_required));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_edit_delete_and_move_stay_review_gated_in_auto_create_policy() {
    let cases = [
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-overwrite",
                RawModelToolName::Known(ModelToolName::OverwriteFile),
                json!({ "target_path": "existing.txt", "contents": "replacement" }),
            )]),
            "overwrite existing.txt",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-patch",
                RawModelToolName::Known(ModelToolName::PatchFile),
                json!({ "target_path": "existing.txt", "find": "original", "replace": "patched" }),
            )]),
            "patch existing.txt",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-delete",
                RawModelToolName::Known(ModelToolName::DeleteFile),
                json!({ "target_path": "existing.txt" }),
            )]),
            "delete existing.txt",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-move",
                RawModelToolName::Known(ModelToolName::MoveFile),
                json!({ "source_path": "existing.txt", "target_path": "moved.txt" }),
            )]),
            "move existing.txt",
        ),
    ];

    for (index, (output, input)) in cases.into_iter().enumerate() {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) =
            rooted_session(&format!("model-first-auto-create-review-gated-{index}"));
        std::fs::write(root.join("existing.txt"), "original").unwrap();

        controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            input,
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            "original"
        );
        assert!(!root.join("moved.txt").exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.user_approval_required));
        assert!(session
            .events()
            .iter()
            .any(|event| { matches!(event, Event::ActionProposed(_)) }));
        assert!(session
            .events()
            .iter()
            .all(|event| { !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_)) }));

        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn model_first_workspace_write_policy_auto_applies_typed_file_writes() {
    let cases = [
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-overwrite",
                RawModelToolName::Known(ModelToolName::OverwriteFile),
                json!({ "target_path": "existing.txt", "contents": "replacement" }),
            )]),
            "overwrite existing.txt",
            "replacement",
            "FileOverwritten",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-patch",
                RawModelToolName::Known(ModelToolName::PatchFile),
                json!({ "target_path": "existing.txt", "find": "original", "replace": "patched" }),
            )]),
            "patch existing.txt",
            "patched",
            "FilePatched",
        ),
    ];

    for (index, (output, input, expected_contents, expected_result)) in
        cases.into_iter().enumerate()
    {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session(&format!("model-first-workspace-write-{index}"));
        std::fs::write(root.join("existing.txt"), "original").unwrap();

        let result = controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            input,
            PermissionPolicyMode::WorkspaceWriteWithReview,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            expected_contents
        );
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.is_policy_approved()));
        assert!(format!("{:?}", session.actions()[0].verified_result).contains(expected_result));
        assert!(result.events.iter().any(|event| matches!(
            event,
            Event::ActionApproved(action)
                if action
                    .approval_source
                    .as_ref()
                    .is_some_and(ApprovalSource::is_policy)
        )));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionProposed(_))));

        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn model_first_workspace_write_policy_keeps_risky_actions_review_gated() {
    let cases = [
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-delete",
                RawModelToolName::Known(ModelToolName::DeleteFile),
                json!({ "target_path": "existing.txt" }),
            )]),
            "delete existing.txt",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-move",
                RawModelToolName::Known(ModelToolName::MoveFile),
                json!({ "source_path": "existing.txt", "target_path": "moved.txt" }),
            )]),
            "move existing.txt",
        ),
        (
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-shell",
                RawModelToolName::Known(ModelToolName::ShellCommand),
                json!({ "command": "touch shell-created.txt", "cwd": "." }),
            )]),
            "run setup",
        ),
    ];

    for (index, (output, input)) in cases.into_iter().enumerate() {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) =
            rooted_session(&format!("model-first-workspace-risky-gated-{index}"));
        std::fs::write(root.join("existing.txt"), "original").unwrap();

        controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            input,
            PermissionPolicyMode::WorkspaceWriteWithReview,
        );

        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            "original"
        );
        assert!(!root.join("moved.txt").exists());
        assert!(!root.join("shell-created.txt").exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.user_approval_required));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn model_first_mixed_batch_does_not_auto_apply_unsafe_actions() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "app" }),
        ),
        raw_model_tool_call(
            "call-create-file",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "app/index.html", "contents": "<div></div>\n" }),
        ),
        raw_model_tool_call(
            "call-shell",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({ "command": "touch shell-created.txt", "cwd": "." }),
        ),
        raw_model_tool_call(
            "call-overwrite",
            RawModelToolName::Known(ModelToolName::OverwriteFile),
            json!({ "target_path": "existing.txt", "contents": "replacement" }),
        ),
        raw_model_tool_call(
            "call-delete",
            RawModelToolName::Known(ModelToolName::DeleteFile),
            json!({ "target_path": "existing.txt" }),
        ),
        raw_model_tool_call(
            "call-move",
            RawModelToolName::Known(ModelToolName::MoveFile),
            json!({ "source_path": "existing.txt", "target_path": "moved.txt" }),
        ),
    ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-mixed-batch");
    std::fs::write(root.join("existing.txt"), "original").unwrap();

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create app files and run setup",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(root.join("app").is_dir());
    assert!(root.join("app/index.html").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("existing.txt")).unwrap(),
        "original"
    );
    assert!(!root.join("shell-created.txt").exists());
    assert!(!root.join("moved.txt").exists());
    assert!(session.actions().iter().any(|record| {
        matches!(
            record.action.request,
            ActionRequest::CreateDirectory(_) | ActionRequest::CreateFile(_)
        ) && record.action.state == ActionLifecycleState::Applied
    }));
    assert!(session.actions().iter().any(|record| {
        matches!(record.action.request, ActionRequest::ShellCommand(_))
            && record.action.state == ActionLifecycleState::Proposed
    }));
    assert!(session.actions().iter().all(|record| {
        !matches!(
            record.action.request,
            ActionRequest::OverwriteFile(_)
                | ActionRequest::DeleteFile(_)
                | ActionRequest::MoveFile(_)
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_unknown_and_malformed_tool_calls_fail_safely() {
    let cases = [
        ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-unknown",
            RawModelToolName::Unknown("unknown_tool".to_string()),
            json!({}),
        )]),
        ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-malformed",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "contents": "missing target" }),
        )]),
    ];

    for output in cases {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let mut session = session();

        controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "draft a tool call",
            PermissionPolicyMode::ReviewAll,
        );

        assert!(session.actions().is_empty());
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::Error(_))));
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
            )
        }));
    }
}
