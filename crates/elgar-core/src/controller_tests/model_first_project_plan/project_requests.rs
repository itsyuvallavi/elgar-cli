use super::*;

#[test]
fn model_first_compound_folder_project_request_applies_model_tool_calls() {
    let (mut session, root) = rooted_session("model-first-compound-folder-project-tools");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let project_root = desktop.join("Demo123");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-create-root",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": project_root.display().to_string() }),
        ),
        raw_model_tool_call(
            "call-create-app",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": project_root.join("calculator.py").display().to_string(),
                "contents": "print('calculator UI placeholder')\n"
            }),
        ),
        raw_model_tool_call(
            "call-create-readme",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": project_root.join("README.md").display().to_string(),
                "contents": "# Demo123 Calculator\n"
            }),
        ),
    ]);
    let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);

    controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI.",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

    let greedy_clause = "inside the folder create a python project of a calculator with UI";
    let greedy_target = desktop.join(format!("Demo123, {greedy_clause}"));
    assert!(project_root.is_dir());
    assert!(project_root.join("calculator.py").is_file());
    assert!(project_root.join("README.md").is_file());
    assert!(!greedy_target.exists());
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert_eq!(session.actions().len(), 3);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));
    for record in session.actions() {
        assert!(!record.action.summary.contains(greedy_clause));
        assert!(!record
            .action
            .request
            .approval_target()
            .contains(greedy_clause));
        assert!(record.verified_result.is_some());
    }
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(project_root.as_path())
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_compound_folder_project_prose_only_creates_no_malformed_folder() {
    let (provider, received_tools, chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("I need tool calls to create files."));
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-compound-folder-project-prose");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);

    controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI.",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

    let greedy_target =
        desktop.join("Demo123, inside the folder create a python project of a calculator with UI");
    assert!(!desktop.join("Demo123").exists());
    assert!(!greedy_target.exists());
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )
    }));
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("Created")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_provider_stub_compound_folder_project_emits_project_tool_calls() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("model-first-stub-compound-project-tools");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let project_root = desktop.join("Demo123");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set_home(&home);

    controller.legacy_controller_model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

    assert!(project_root.is_dir());
    assert!(project_root.join("calculator.py").is_file());
    assert!(project_root.join("README.md").is_file());
    assert!(!desktop
        .join("Demo123, inside the folder create a python project of a calculator with UI")
        .exists());
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Applied));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_create_project_you_planned_uses_provider_tools_only() {
    let (mut session, root) = rooted_session("model-first-create-project-you-planned");

    let create_folder_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-folder",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "planned-hybrid" }),
    )]);
    let (create_folder_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(create_folder_output);
    let create_folder_controller = Controller::new(create_folder_provider);
    create_folder_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called planned-hybrid",
        PermissionPolicyMode::AutoCreateReviewModify,
    );
    let project_root = root.join("planned-hybrid");
    assert!(project_root.is_dir());

    let plan_contents = format!(
            "# TS and Python Project Plan\n\nProject root: {}\n\n- Add TypeScript files: `package.json`, `tsconfig.json`, and `src/main.ts`.\n- Add Python files: `python/main.py` and `requirements.txt`.\n- Add a README with run instructions.\n",
            project_root.display()
        );
    let write_plan_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-plan",
        RawModelToolName::Known(ModelToolName::CreateFile),
        json!({ "target_path": "project-plan.md", "contents": plan_contents }),
    )]);
    let (write_plan_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(write_plan_output);
    let write_plan_controller = Controller::new(write_plan_provider);
    write_plan_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "write a TypeScript and Python project plan inside that folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );
    assert!(project_root.join("project-plan.md").is_file());

    let (read_plan_provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
        ProviderOutput::new("The verified plan describes TypeScript and Python project files."),
    );
    let read_plan_controller = Controller::new(read_plan_provider);
    read_plan_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "read the plan you wrote",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let actions_before_execute = session.actions().len();
    let (prose_provider, prose_received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("Done, I created the project."));
    let prose_controller = Controller::new(prose_provider);
    prose_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create the project you planned",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(prose_received_tools.lock().unwrap().len(), 1);
    assert_eq!(session.actions().len(), actions_before_execute);
    assert!(!project_root.join("package.json").exists());

    let execute_output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-src",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "src" }),
        ),
        raw_model_tool_call(
            "call-python",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "python" }),
        ),
        raw_model_tool_call(
            "call-package",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "package.json", "contents": "{}\n" }),
        ),
        raw_model_tool_call(
            "call-tsconfig",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "tsconfig.json", "contents": "{}\n" }),
        ),
        raw_model_tool_call(
            "call-main-ts",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "src/main.ts", "contents": "console.log('ok');\n" }),
        ),
        raw_model_tool_call(
            "call-main-py",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "python/main.py", "contents": "print('ok')\n" }),
        ),
        raw_model_tool_call(
            "call-requirements",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "requirements.txt", "contents": "" }),
        ),
        raw_model_tool_call(
            "call-readme",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "README.md", "contents": "# planned-hybrid\n" }),
        ),
    ]);
    let (execute_provider, received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(execute_output);
    let execute_controller = Controller::new(execute_provider);
    execute_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create the project you planned",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(received_tools.lock().unwrap().len(), 1);
    for relative in [
        "package.json",
        "tsconfig.json",
        "src/main.ts",
        "python/main.py",
        "requirements.txt",
        "README.md",
    ] {
        assert!(
            project_root.join(relative).is_file(),
            "missing expected project file {relative}"
        );
    }
    assert!(project_root.join("src").is_dir());
    assert!(project_root.join("python").is_dir());
    let applied_after_plan = session
        .actions()
        .iter()
        .filter(|record| record.action.state == ActionLifecycleState::Applied)
        .count();
    assert_eq!(applied_after_plan, actions_before_execute + 8);
    assert!(session.project_memory().latest_structured_plan().is_none());

    let _ = std::fs::remove_dir_all(root);
}
