use super::*;

#[test]
fn model_first_simple_desktop_folder_can_apply_model_tool_path() {
    let (mut session, root) = rooted_session("model-first-desktop-folder");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let desktop_target = desktop.join("ElgarRetest-267");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": desktop_target.display().to_string() }),
    )]);
    let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called ElgarRetest-267 in the desktop",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(desktop_target.is_dir());
    assert!(!root.join("ElgarRetest-267").exists());
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(matches!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated { .. }
        ))
    ));
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(desktop_target.as_path())
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_tilde_folder_request_creates_under_home_not_repo_literal_tilde() {
    let (mut session, root) = rooted_session("model-first-home-tilde-folder");
    let home = root.join("home");
    let target = home.join("myfirstproject");
    std::fs::create_dir_all(&home).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "~/myfirstproject" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "i want you to create a folder in ~/ call it myfirstproject",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!root.join("~").join("myfirstproject").exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated {
                path: target.display().to_string()
            }
        ))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_tilde_folder_guidance_text_uses_controller_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-home-tilde-folder-guidance");
    let home = root.join("home");
    let target = home.join("myfirstproject");
    std::fs::create_dir_all(&home).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output =
        ProviderOutput::new("Do you want the folder in your home directory or project root?");
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "i want you to create a folder in ~/ call it myfirstproject",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!root.join("~").join("myfirstproject").exists());
    let rendered = render_session(&session);
    assert!(!rendered.contains("home directory or project root"));
    assert!(rendered.contains("Created"));
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_existing_tilde_folder_is_verified_idempotent_success() {
    let (mut session, root) = rooted_session("model-first-home-existing-tilde-folder");
    let home = root.join("home");
    let target = home.join("myfirstproject");
    std::fs::create_dir_all(&target).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output =
        ProviderOutput::new("Do you want the folder in your home directory or project root?");
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "i want you to create a folder in ~/ call it myfirstproject",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated {
                path: target.display().to_string()
            }
        ))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_guidance_only_for_called_folder_uses_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-desktop-called-guidance");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("test");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new(
        "Do you mean the Desktop directory in your home folder, e.g. /Users/yuval/Desktop?",
    );
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called test in the desktop",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    let rendered = render_session(&session);
    assert!(!rendered.contains("Do you mean the Desktop directory"));
    assert!(rendered.contains("Created Desktop/test."));
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_no_tool_empty_response_uses_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-desktop-empty-no-tool-fallback");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("test");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("");
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called test in the desktop",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    let rendered = render_session(&session);
    assert!(!rendered.contains("did not receive a tool call"));
    assert!(rendered.contains("Created Desktop/test."));
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_request_retargets_relative_tool_paths_to_desktop() {
    let (mut session, root) = rooted_session("model-first-desktop-relative-tool-paths");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let project_root = desktop.join("Demo123");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-create-root",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "Demo123" }),
        ),
        raw_model_tool_call(
            "call-create-app",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": "Demo123/calculator.py",
                "contents": "print('calculator')\n"
            }),
        ),
        raw_model_tool_call(
            "call-create-readme",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": "Demo123/README.md",
                "contents": "# Demo123\n"
            }),
        ),
    ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI.",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

    assert!(project_root.is_dir());
    assert!(project_root.join("calculator.py").is_file());
    assert!(project_root.join("README.md").is_file());
    assert!(!root.join("Demo123").exists());
    assert!(!root.join("Demo123/calculator.py").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_request_does_not_duplicate_desktop_relative_prefix() {
    let (mut session, root) = rooted_session("model-first-desktop-prefixed-relative-path");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T002646Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "Desktop/ElgarLiveE2E-20260524T002646Z" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T002646Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!desktop
        .join("Desktop")
        .join("ElgarLiveE2E-20260524T002646Z")
        .exists());
    let ActionRequest::CreateDirectory(action) = &session.actions()[0].action.request else {
        panic!("expected CreateDirectory action");
    };
    assert_eq!(action.target_path, target);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_request_does_not_duplicate_repeated_desktop_relative_prefix() {
    let (mut session, root) = rooted_session("model-first-desktop-repeated-prefixed-relative-path");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("test");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "Desktop/Desktop/test" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called test in the desktop",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!desktop.join("Desktop").join("test").exists());
    let ActionRequest::CreateDirectory(action) = &session.actions()[0].action.request else {
        panic!("expected CreateDirectory action");
    };
    assert_eq!(action.target_path, target);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_prompt_marks_explicit_desktop_create_as_actionable() {
    let (mut session, root) = rooted_session("model-first-desktop-contract");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T111953Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "Desktop/ElgarLiveE2E-20260524T111953Z" }),
    )]);
    let (provider, _received_tools, _chat_calls, prompts) =
        ToolEnabledFakeProvider::new_with_prompt_capture(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T111953Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    let prompt = prompts.lock().unwrap().join("\n");
    assert!(prompt.contains("explicitly names Desktop"));
    assert!(prompt.contains("that target is clear"));
    assert!(prompt.contains("do not ask whether Desktop means the user's home Desktop"));
    assert!(prompt.contains(
        "User request:\nCreate a folder on my Desktop called ElgarLiveE2E-20260524T111953Z"
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_observed_desktop_guidance_prose_uses_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-desktop-observed-prose");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T111953Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output =
        ProviderOutput::new("Do you want the folder created in your home Desktop directory?");
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T111953Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(matches!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated { .. }
        ))
    ));
    let visible = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible.contains("Do you want the folder created"));
    assert!(!visible.to_ascii_lowercase().contains("guidance"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_guidance_with_safe_create_does_not_ask_clarification() {
    let (mut session, root) = rooted_session("model-first-desktop-guidance-safe-create");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T002646Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new(
        "Path? Project-relative path: likely the repository root. Use create_directory tool.",
    )
    .with_tool_calls(vec![
        raw_model_tool_call(
            "call-guidance",
            RawModelToolName::Known(ModelToolName::AskGuidance),
            json!({
                "question": "Is the Desktop location relative to this project root?",
                "reason": "The tool schema says project-relative path."
            }),
        ),
        raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "ElgarLiveE2E-20260524T002646Z" }),
        ),
    ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T002646Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!root.join("ElgarLiveE2E-20260524T002646Z").exists());
    let visible = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible.contains("Is the Desktop location relative"));
    assert!(!visible.to_ascii_lowercase().contains("clarification"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_prose_only_uncertainty_uses_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-desktop-prose-fallback");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T095523Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new(
            "Create directory. Use create_directory with target_path relative? Desktop path likely ~/Desktop. What is the absolute path of your Desktop folder?",
        );
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T095523Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    let visible = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible.contains("Create directory. Use create_directory"));
    assert!(!visible.contains("What is the absolute path"));
    assert!(visible.contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_desktop_guidance_only_uses_safe_create_fallback() {
    let (mut session, root) = rooted_session("model-first-desktop-guidance-only-fallback");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T101725Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new(
            "Create directory on Desktop. Use create_directory tool. Path relative? Probably project root is unclear.",
        )
        .with_tool_calls(vec![raw_model_tool_call(
            "call-guidance",
            RawModelToolName::Known(ModelToolName::AskGuidance),
            json!({
                "question": "What is the absolute path to your Desktop directory?",
                "reason": "The tool schema says project-relative path."
            }),
        )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T101725Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    let visible = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible.contains("absolute path to your Desktop"));
    assert!(!visible.contains("Use create_directory tool"));
    assert!(visible.contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_absolute_desktop_request_does_not_duplicate_relative_absolute_prefix() {
    let (mut session, root) = rooted_session("model-first-desktop-relative-absolute-prefix");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T002646Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let provider_target = target
        .strip_prefix(std::path::Path::new("/"))
        .unwrap()
        .display()
        .to_string();
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": provider_target }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        &format!(
            "Create exactly this absolute folder path: {}",
            target.display()
        ),
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(target.is_dir());
    assert!(!desktop
        .join(target.strip_prefix(std::path::Path::new("/")).unwrap())
        .exists());
    let ActionRequest::CreateDirectory(action) = &session.actions()[0].action.request else {
        panic!("expected CreateDirectory action");
    };
    assert_eq!(action.target_path, target);

    let _ = std::fs::remove_dir_all(root);
}
