use super::*;

#[test]
fn model_first_prose_only_implement_plan_does_not_fake_success() {
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
        ProviderOutput::new("Done, I implemented the plan and created package.json."),
    );
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-prose-implement-plan");
    let project_root = root.join("planned-app");
    std::fs::create_dir_all(&project_root).unwrap();
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: project_root.clone(),
        source_action_id: "action-folder".to_string(),
    });

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "implement the plan",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!project_root.join("package.json").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
    )));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_plan_request_uses_provider_tool_call_and_verified_folder_context() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-plan",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({
            "target_path": "react-ts-project-plan.md",
            "contents": "# React TS Project Plan\n\n- Use model tools.\n"
        }),
    )]);
    let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-plan-same-folder");
    let project_root = seed_verified_folder(&mut session, &root, "verified-react");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a plan for a simple React TS project in the same folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let plan_path = project_root.join("react-ts-project-plan.md");
    assert!(plan_path.is_file());
    assert!(!root.join("react-ts-project-plan.md").exists());
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    let ActionRequest::CreateFile(action) = &session.actions()[0].action.request else {
        panic!("expected CreateFile plan action");
    };
    assert_eq!(
        action.target_path,
        PathBuf::from("verified-react/react-ts-project-plan.md")
    );
    assert!(session.actions()[0].verified_result.is_some());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_same_folder_plan_after_desktop_folder_targets_verified_desktop_folder() {
    let (mut session, root) = rooted_session("model-first-desktop-folder-plan-followup");
    let home = root.parent().unwrap().join(format!(
        "elgar-controller-home-{}-model-first-desktop-folder-plan-followup",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    let desktop = home.join("Desktop");
    let project_root = desktop.join("ElgarLiveE2E-20260524T002646Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);

    let create_folder_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "Desktop/ElgarLiveE2E-20260524T002646Z" }),
    )]);
    let (create_folder_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(create_folder_output);
    let create_folder_controller = Controller::new(create_folder_provider);
    create_folder_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T002646Z",
        PermissionPolicyMode::AutoCreateReviewModify,
    );
    assert!(project_root.is_dir());

    let write_plan_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-plan",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({
            "target_path": "react-ts-project-plan.md",
            "contents": "# React TypeScript Project Plan\n"
        }),
    )]);
    let (write_plan_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(write_plan_output);
    let write_plan_controller = Controller::new(write_plan_provider);
    write_plan_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a Markdown plan for a simple React TypeScript project in that same folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let plan_path = project_root.join("react-ts-project-plan.md");
    assert!(plan_path.is_file());
    assert!(!root.join("react-ts-project-plan.md").exists());
    let ActionRequest::CreateFile(action) = &session.actions()[1].action.request else {
        panic!("expected CreateFile plan action");
    };
    assert_eq!(action.target_path, plan_path);
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn model_first_same_folder_implement_prompt_includes_verified_readme_plan_content() {
    let (mut session, root) = rooted_session("model-first-desktop-readme-plan-content-followup");
    let home = root.parent().unwrap().join(format!(
        "elgar-controller-home-{}-model-first-desktop-readme-plan-content-followup",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    let desktop = home.join("Desktop");
    let project_root = desktop.join("ElgarLiveE2E-PlanContent");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);

    let create_folder_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "Desktop/ElgarLiveE2E-PlanContent" }),
    )]);
    let (create_folder_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(create_folder_output);
    let create_folder_controller = Controller::new(create_folder_provider);
    create_folder_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Create a folder on my Desktop called ElgarLiveE2E-PlanContent",
        PermissionPolicyMode::AutoCreateReviewModify,
    );
    assert!(project_root.is_dir());

    let plan_contents = format!(
            "# React TypeScript Project Plan\n\nProject root: {}\n\n- Create package.json.\n- Create tsconfig.json.\n- Create vite.config.ts.\n- Create src/main.tsx.\n- Create src/App.tsx.\n- Defer package installation.\n",
            project_root.display()
        );
    let write_plan_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-plan",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({ "target_path": "README.md", "contents": plan_contents }),
    )]);
    let (write_plan_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(write_plan_output);
    let write_plan_controller = Controller::new(write_plan_provider);
    write_plan_controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "Create a Markdown plan for a simple React TypeScript project in that same folder as README.md",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

    let plan_path = project_root.join("README.md");
    assert!(plan_path.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );

    let (implement_provider, received_tools, _chat_calls, prompts) =
            ToolEnabledFakeProvider::new_with_prompt_capture(ProviderOutput::new(
                "What files and structure should the React TypeScript project contain according to the plan?",
            ));
    let implement_controller = Controller::new(implement_provider);
    implement_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Implement the plan in that same folder.",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 1);
    let captured = prompts.lock().unwrap().join("\n");
    assert!(
        captured.contains("When verified memory includes a latest verified plan content excerpt")
    );
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("latest verified plan:"));
    assert!(captured.contains("latest verified plan content excerpt"));
    assert!(captured.contains(&plan_path.display().to_string()));
    assert!(captured.contains(&project_root.display().to_string()));
    for expected in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "src/main.tsx",
        "src/App.tsx",
    ] {
        assert!(
            captured.contains(expected),
            "provider prompt omitted plan item {expected}"
        );
    }

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn model_first_live_react_ts_plan_incomplete_batches_fall_back_to_controller_scaffold() {
    let (mut session, root) = rooted_session("incomplete-live-shape");
    let project_root = seed_verified_live_react_ts_plan(&mut session, &root, "ElgarLiveE2E");

    let first_output = ProviderOutput::new(
        "Create files per plan: package.json, tsconfig.json, src/main.tsx, App.tsx.",
    )
    .with_tool_calls(vec![raw_model_tool_call(
        "call-src",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "src" }),
    )]);
    let second_output = ProviderOutput::new("I still need create_file tool calls.");
    let third_output = ProviderOutput::new("I still cannot call create_file.");
    let (provider, received_tools, prompts) =
        ToolEnabledSequenceProvider::new(vec![first_output, second_output, third_output]);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Implement the plan in that same folder.",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let received_tools = received_tools.lock().unwrap();
    assert_eq!(received_tools.len(), 3);
    assert_eq!(received_tools[2], vec!["create_file".to_string()]);
    assert_eq!(prompts.lock().unwrap().len(), 3);
    for relative in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
        "src/main.tsx",
        "src/App.tsx",
    ] {
        assert!(
            project_root.join(relative).is_file(),
            "missing expected project file {relative}"
        );
    }
    assert!(project_root.join("src").is_dir());
    assert!(project_root.join("src/styles.css").is_file());
    assert!(project_root.join("README.md").is_file());
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    assert!(session.actions().iter().all(|record| matches!(
        &record.action.request,
        ActionRequest::CreateDirectory(_) | ActionRequest::CreateFile(_)
    )));
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("No files were changed")
        )
    }));
    let visible = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    assert!(visible.contains("i finished the react typescript project from the verified plan"));
    assert!(!visible.contains("model-first"), "{visible}");
    assert!(
        !visible.contains("tool calls stayed incomplete"),
        "{visible}"
    );
    assert!(!visible.contains("controller-owned"), "{visible}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_incomplete_verified_plan_tool_batch_continues_once_and_applies_files() {
    let (mut session, root) = rooted_session("model-first-incomplete-live-continuation");
    let project_root = seed_verified_react_ts_file_plan(&mut session, &root, "ElgarLiveE2E");

    let first_output = ProviderOutput::new(
        "Create files per plan: package.json, tsconfig.json, src/main.tsx, App.tsx.",
    )
    .with_tool_calls(vec![raw_model_tool_call(
        "call-src",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "src" }),
    )]);
    let second_output =
        ProviderOutput::new("").with_tool_calls(react_ts_missing_create_file_tool_calls());
    let (provider, received_tools, prompts) =
        ToolEnabledSequenceProvider::new(vec![first_output, second_output]);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Implement the plan in that same folder.",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 2);
    assert_eq!(prompts.lock().unwrap().len(), 2);
    for relative in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
        "src/main.tsx",
        "src/App.tsx",
    ] {
        assert!(
            project_root.join(relative).is_file(),
            "missing expected project file {relative}"
        );
    }
    assert!(project_root.join("src").is_dir());
    assert_eq!(session.actions().len(), 7);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    assert!(session
        .actions()
        .iter()
        .all(|record| record.verified_result.is_some()));
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("No files were changed")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_first_continuation_prose_final_retry_applies_files() {
    let (mut session, root) = rooted_session("model-first-final-continuation-success");
    let project_root = seed_verified_react_ts_file_plan(&mut session, &root, "ElgarLiveE2E");

    let first_output = ProviderOutput::new("Create directories and files per plan. Create files.")
        .with_tool_calls(vec![raw_model_tool_call(
            "call-src",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "src" }),
        )]);
    let second_output = ProviderOutput::new(
        "I only received partial implementation tool calls; the plan still needs files.",
    );
    let third_output =
        ProviderOutput::new("").with_tool_calls(react_ts_missing_create_file_tool_calls());
    let (provider, received_tools, prompts) =
        ToolEnabledSequenceProvider::new(vec![first_output, second_output, third_output]);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "Implement the plan in that same folder.",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let received_tools = received_tools.lock().unwrap();
    assert_eq!(received_tools.len(), 3);
    assert_eq!(received_tools[2], vec!["create_file".to_string()]);
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[1].contains("Return tool calls now."));
    assert!(prompts[2].contains("FINAL TOOL-CALL RETRY."));
    assert!(prompts[2].contains("Required target_path values:"));
    assert!(prompts[2].contains("src/App.tsx"));
    for relative in [
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "index.html",
        "src/main.tsx",
        "src/App.tsx",
    ] {
        assert!(
            project_root.join(relative).is_file(),
            "missing expected project file {relative}"
        );
    }
    assert!(project_root.join("src").is_dir());
    assert_eq!(session.actions().len(), 7);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("No files were changed")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_implement_plan_applies_provider_tool_calls_in_verified_plan_root() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-package",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "package.json", "contents": "{\"scripts\":{}}\n" }),
            ),
            raw_model_tool_call(
                "call-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
    let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-implement-verified-plan");
    let (project_root, _plan_path) =
        seed_verified_react_ts_plan(&mut session, &root, "verified-react");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "implement the plan",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert_eq!(session.actions().len(), 3);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    assert!(session
        .actions()
        .iter()
        .all(|record| record.verified_result.is_some()));
    assert!(project_root.join("package.json").is_file());
    assert!(project_root.join("src").is_dir());
    assert!(project_root.join("src/App.tsx").is_file());
    assert!(!root.join("package.json").exists());
    assert!(session.project_memory().latest_structured_plan().is_none());
    assert!(matches!(
        session.pending_action_selection(),
        crate::session::PendingActionSelection::None
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_create_rest_of_project_uses_provider_tools_and_verified_plan_path() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-src",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "src" }),
        ),
        raw_model_tool_call(
            "call-main",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "src/main.tsx", "contents": "void 0;\n" }),
        ),
    ]);
    let (provider, received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-rest-of-project");
    let (project_root, _plan_path) =
        seed_verified_react_ts_plan(&mut session, &root, "verified-rest");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create the rest of the project",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(session.actions().len(), 2);
    assert!(project_root.join("src/main.tsx").is_file());
    assert!(!root.join("src/main.tsx").exists());
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_prose_only_verified_plan_followup_does_not_scaffold_or_overwrite() {
    let (provider, received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("Done, I implemented the plan."));
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-scaffold-conflict");
    let (project_root, _plan_path) =
        seed_verified_react_ts_plan(&mut session, &root, "verified-conflict");
    std::fs::write(project_root.join("package.json"), "original package").unwrap();

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "implement the plan",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(project_root.join("package.json")).unwrap(),
        "original package"
    );
    assert!(!project_root.join("src").exists());
    assert!(session.actions().is_empty());
    assert!(session.project_memory().latest_structured_plan().is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_followup_targets_latest_verified_folder_instead_of_repo_root() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-package",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "package.json", "contents": "{\"scripts\":{}}\n" }),
            ),
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-latest-folder-followup");
    let project_root = root.join("verified-app");
    std::fs::create_dir_all(&project_root).unwrap();
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: project_root.clone(),
        source_action_id: "action-folder".to_string(),
    });

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "go ahead and make the files inside the folder you created",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(project_root.join("package.json").is_file());
    assert!(project_root.join("src").is_dir());
    assert!(project_root.join("src/App.tsx").is_file());
    assert!(!root.join("package.json").exists());
    assert!(!root.join("src/App.tsx").exists());
    assert!(session.actions().iter().all(|record| {
        record
            .action
            .request
            .approval_target()
            .starts_with("verified-app")
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_pending_action_guard_blocks_second_proposed_action() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "second-action" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let mut session = session();
    let pending = Action::proposed(
        "action-1",
        ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
            target_path: PathBuf::from("first-action"),
        }),
        "create first-action",
    );
    session.push_action(ActionRecord::new(pending));

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create another directory",
        PermissionPolicyMode::ReviewAll,
    );

    assert_eq!(session.actions().len(), 1);
    assert!(session.events().iter().any(|event| match event {
        Event::Error(error) => error.message.contains("already waiting"),
        _ => false,
    }));
}
