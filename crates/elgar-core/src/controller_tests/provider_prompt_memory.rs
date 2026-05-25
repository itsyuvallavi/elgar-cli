use super::*;

#[test]
fn existing_turn_does_not_use_model_first_tool_enabled_method() {
    let (provider, received_tools, chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("tool path"));
    let controller = Controller::new(provider);
    let mut session = session();

    let result = controller.turn(&mut session, "what is rust?");

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(*chat_calls.lock().unwrap(), 1);
    assert!(received_tools.lock().unwrap().is_empty());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "legacy chat path"
        )
    }));
}

#[test]
fn explicit_provider_controller_records_provider_output_without_mutating_truth() {
    let controller = Controller::new(FakeProvider::success(
        "I approved and wrote hello.py successfully.",
    ));
    let (mut session, _root) = rooted_session("fake-provider-output");
    let path = session.project_root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!path.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
}

#[test]
fn ask_model_provider_prompt_includes_bounded_controller_context() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-context-bundle");
    std::fs::write(root.join("AGENTS.md"), "Keep answers short.").unwrap();

    controller.model_turn(&mut session, "what can you do?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Local context selected by Elgar controller:"));
    assert!(captured.contains("--- AGENTS.md ---\nKeep answers short."));
    assert!(captured.contains("User request:\nwhat can you do?"));
    assert_eq!(session.context_accounting().loaded_files.len(), 1);
    assert_eq!(
        session.context_accounting().loaded_files[0].display_path,
        "AGENTS.md"
    );
    assert!(session.context_accounting().estimated_tokens.is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ask_model_provider_prompt_includes_recent_visible_conversation() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-recent-conversation");

    controller.turn(
        &mut session,
        "can you create a folder called hello-world in the desktop?",
    );
    controller.model_turn(&mut session, "i dont see the folder");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Recent conversation selected by Elgar controller:"));
    assert!(captured.contains("user: can you create a folder called hello-world in the desktop?"));
    assert!(captured.contains("controller action proposed: ShellCommand"));
    assert!(captured.contains("assistant(controller): I can create"));
    assert!(captured.contains("User request:\ni dont see the folder"));
    assert!(!captured.contains("thinking:"));
    assert_eq!(session.actions().len(), 1);
    assert!(!root.join("hello-world").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_omits_verified_memory_for_unrelated_chat() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-unrelated");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "hello");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(!captured.contains("Verified memory selected by Elgar controller:"));
    assert!(root.join("memory-target").is_dir());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_includes_verified_folder_for_reference_prompt() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-folder");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "where is that folder?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("verified folder:"));
    assert!(captured.contains(&root.join("memory-target").display().to_string()));
    assert!(captured.contains("User request:\nwhere is that folder?"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_includes_verified_folder_for_where_did_you_put_it() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-folder-put-it");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "where did you put it?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("verified folder:"));
    assert!(captured.contains(&root.join("memory-target").display().to_string()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_includes_verified_folder_for_created_path_question() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-folder-path-question");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "what path did you create?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("verified folder:"));
    assert!(captured.contains(&root.join("memory-target").display().to_string()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_includes_all_verified_shell_expected_directories() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-shell-dirs");
    let first = root.join("generated-src");
    let second = root.join("generated-tests");
    let command = format!(
        "mkdir -p {} {}",
        super::super::shell_quote_path(&first),
        super::super::shell_quote_path(&second)
    );
    let mut shell_command = ShellCommandAction::new(command.clone(), root.clone());
    shell_command.expected_directories = vec![first.clone(), second.clone()];
    let action = Action::proposed(
        "action-1",
        ActionRequest::ShellCommand(shell_command),
        "create generated directories",
    );
    session.push_action(ActionRecord::new(action));

    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "where did you put it?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains(&first.display().to_string()));
    assert!(captured.contains(&second.display().to_string()));
    assert_eq!(session.project_memory().verified_folders.len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_reserves_local_context_budget_for_prompt_extensions() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-prompt-reserved-budget");
    std::fs::write(root.join("AGENTS.md"), "a".repeat(4_096)).unwrap();
    controller.refresh_context_accounting(&mut session, Some(128_000));

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    controller.model_turn(&mut session, "where did you put it?");

    let loaded = &session.context_accounting().loaded_files[0];
    assert_eq!(loaded.display_path, "AGENTS.md");
    assert!(loaded.truncated);
    assert!(loaded.bytes < 3_072);
    assert!(prompts
        .lock()
        .unwrap()
        .join("\n")
        .contains("Verified memory selected by Elgar controller:"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_includes_verified_plan_memory_for_plan_prompt() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-plan");
    let project_root = root.join("planned-app");
    let plan_path = project_root.join("plan.md");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(
        &plan_path,
        "# Plan\n\n- Create package.json.\n- Create vite.config.ts.\n",
    )
    .unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project_root.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path.clone(),
        project_root: project_root.clone(),
        stage: "implementation".to_string(),
        status: StructuredProjectPlanStatus::Proposed,
        expected_directories: vec![project_root.join("src")],
        expected_files: vec![project_root.join("src/main.rs")],
    });

    controller.model_turn(&mut session, "execute the plan");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("latest verified plan:"));
    assert!(captured.contains("latest verified plan content excerpt"));
    assert!(captured.contains("latest structured plan:"));
    assert!(captured.contains(&plan_path.display().to_string()));
    assert!(captured.contains(&project_root.display().to_string()));
    assert!(captured.contains("package.json"));
    assert!(captured.contains("vite.config.ts"));
    assert!(captured.contains("expected dirs 1, expected files 1"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_trace_records_selected_verified_folder_and_plan_memory() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-trace-selected");
    let project_root = root.join("memory-target");
    let plan_path = project_root.join("plan.md");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    std::fs::write(&plan_path, "# Plan").unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project_root.clone(),
        source_action_id: "action-plan".to_string(),
    });
    let folder_reference = session
        .project_memory()
        .latest_verified_folder()
        .expect("verified folder reference")
        .clone();

    controller.model_turn(&mut session, "execute the plan inside that folder");

    assert_eq!(prompts.lock().unwrap().len(), 1);
    let trace = session
        .latest_provider_prompt_memory_selection()
        .expect("provider prompt memory selection trace");
    assert!(trace.omitted.is_empty());
    assert_eq!(trace.selected.len(), 2);

    let selected_folder = trace
        .selected
        .iter()
        .find(|fact| fact.kind == "verified_folder")
        .expect("selected verified folder fact");
    assert_eq!(selected_folder.path, folder_reference.path);
    assert_eq!(selected_folder.project_root.as_deref(), None);
    assert_eq!(
        selected_folder.source_action_id,
        folder_reference.source_action_id
    );

    let selected_plan = trace
        .selected
        .iter()
        .find(|fact| fact.kind == "verified_plan")
        .expect("selected verified plan fact");
    assert_eq!(selected_plan.path, plan_path);
    assert_eq!(
        selected_plan.project_root.as_deref(),
        Some(project_root.as_path())
    );
    assert_eq!(selected_plan.source_action_id, "action-plan");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_trace_records_stale_verified_folder_and_plan_as_omitted() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-trace-stale");
    let project_root = root.join("stale-target");
    let plan_path = project_root.join("plan.md");

    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(&plan_path, "# Plan").unwrap();
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: project_root.clone(),
        source_action_id: "action-folder".to_string(),
    });
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project_root.clone(),
        source_action_id: "action-plan".to_string(),
    });
    std::fs::remove_dir_all(&project_root).unwrap();

    controller.model_turn(&mut session, "execute the plan inside that folder");

    assert_eq!(prompts.lock().unwrap().len(), 1);
    let trace = session
        .latest_provider_prompt_memory_selection()
        .expect("provider prompt memory selection trace");
    assert!(trace.selected.is_empty());
    assert_eq!(trace.omitted.len(), 2);

    let omitted_folder = trace
        .omitted
        .iter()
        .find(|fact| fact.kind == "verified_folder")
        .expect("omitted verified folder fact");
    assert_eq!(omitted_folder.path, project_root);
    assert_eq!(omitted_folder.project_root.as_deref(), None);
    assert_eq!(omitted_folder.source_action_id, "action-folder");
    assert_eq!(omitted_folder.reason, "missing");

    let omitted_plan = trace
        .omitted
        .iter()
        .find(|fact| fact.kind == "verified_plan")
        .expect("omitted verified plan fact");
    assert_eq!(omitted_plan.path, plan_path);
    assert_eq!(
        omitted_plan.project_root.as_deref(),
        Some(project_root.as_path())
    );
    assert_eq!(omitted_plan.source_action_id, "action-plan");
    assert_eq!(omitted_plan.reason, "missing");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_trace_is_absent_for_unrelated_chat_memory_selection() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-trace-unrelated");
    let project_root = root.join("memory-target");
    let plan_path = project_root.join("plan.md");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    std::fs::write(&plan_path, "# Plan").unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path,
        project_root,
        source_action_id: "action-plan".to_string(),
    });

    controller.model_turn(&mut session, "hello");

    assert_eq!(prompts.lock().unwrap().len(), 1);
    assert_eq!(session.latest_provider_prompt_memory_selection(), None);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_marks_stale_verified_memory_without_trusting_it() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-stale");
    let target = root.join("memory-target");

    controller.turn(&mut session, "create folder memory-target");
    controller.turn(&mut session, "approve");
    std::fs::remove_dir_all(&target).unwrap();
    controller.model_turn(&mut session, "where is that folder?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("omitted missing verified folder:"));
    assert!(captured.contains(&target.display().to_string()));
    assert!(!captured.contains("\nverified folder:"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_does_not_fall_back_to_older_verified_folder_when_latest_is_stale() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-stale-folder-no-fallback");
    let older = root.join("older-memory-target");
    let latest = root.join("latest-memory-target");

    controller.turn(&mut session, "create folder older-memory-target");
    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "create folder latest-memory-target");
    controller.turn(&mut session, "approve");
    std::fs::remove_dir_all(&latest).unwrap();
    controller.model_turn(&mut session, "where is that folder?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("omitted missing verified folder:"));
    assert!(captured.contains(&latest.display().to_string()));
    assert!(!captured.contains("\nverified folder:"));
    assert!(!captured.contains(&older.display().to_string()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_does_not_fall_back_to_older_verified_plan_when_latest_is_stale() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-stale-plan-no-fallback");
    let older_root = root.join("older-plan-root");
    let latest_root = root.join("latest-plan-root");
    let older_plan = older_root.join("plan.md");
    let latest_plan = latest_root.join("plan.md");

    std::fs::create_dir_all(&older_root).unwrap();
    std::fs::create_dir_all(&latest_root).unwrap();
    std::fs::write(&older_plan, "# Older Plan").unwrap();
    std::fs::write(&latest_plan, "# Latest Plan").unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: older_plan.clone(),
        project_root: older_root.clone(),
        source_action_id: "older-plan-action".to_string(),
    });
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: latest_plan.clone(),
        project_root: latest_root,
        source_action_id: "latest-plan-action".to_string(),
    });
    std::fs::remove_file(&latest_plan).unwrap();

    controller.model_turn(&mut session, "execute the plan");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("Verified memory selected by Elgar controller:"));
    assert!(captured.contains("omitted missing verified plan:"));
    assert!(captured.contains(&latest_plan.display().to_string()));
    assert!(!captured.contains("latest verified plan:"));
    assert!(!captured.contains(&older_plan.display().to_string()));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_bounds_verified_memory_section() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-bounded");

    for index in 0..16 {
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: root.join(format!(
                "missing-memory-target-{index}-{}",
                "segment-".repeat(80)
            )),
            source_action_id: format!("action-{index}"),
        });
    }

    controller.model_turn(&mut session, "where is that folder?");

    let captured = prompts.lock().unwrap().join("\n");
    let header = "Verified memory selected by Elgar controller:\n";
    let memory_start = captured.find(header).expect("verified memory header");
    let after_header = &captured[memory_start + header.len()..];
    let memory_end = after_header.find("\n\nUser request:").unwrap();
    let memory_block = &after_header[..memory_end];
    assert!(memory_block.len() <= VERIFIED_MEMORY_BYTE_LIMIT);
    assert!(memory_block.contains("omitted missing verified folder:"));
    assert!(captured.contains("User request:\nwhere is that folder?"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_prompt_trace_excludes_selected_memory_dropped_by_prompt_cap() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-memory-trace-bounded");

    for index in 0..4 {
        let mut path = root.join(format!("memory-target-{index}"));
        for part in 0..5 {
            path = path.join(format!("segment{part}-{}", "x".repeat(48)));
        }
        std::fs::create_dir_all(&path).unwrap();
        session.record_verified_folder_reference(VerifiedFolderReference {
            path,
            source_action_id: format!("action-{index}"),
        });
    }

    controller.model_turn(&mut session, "where is that folder?");

    let captured = prompts.lock().unwrap().join("\n");
    let header = "Verified memory selected by Elgar controller:\n";
    let memory_start = captured.find(header).expect("verified memory header");
    let after_header = &captured[memory_start + header.len()..];
    let memory_end = after_header.find("\n\nUser request:").unwrap();
    let memory_block = &after_header[..memory_end];
    assert!(memory_block.len() <= VERIFIED_MEMORY_BYTE_LIMIT);
    assert!(memory_block.contains("memory-target-3"));
    assert!(memory_block.contains("memory-target-2"));
    assert!(memory_block.contains("memory-target-1"));
    assert!(!memory_block.contains("memory-target-0"));

    let trace = session
        .latest_provider_prompt_memory_selection()
        .expect("provider prompt memory selection trace");
    let selected_action_ids = trace
        .selected
        .iter()
        .map(|fact| fact.source_action_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        selected_action_ids,
        vec!["action-3", "action-2", "action-1"]
    );
    assert!(trace.omitted.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refresh_context_accounting_includes_local_memory_notes() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("refresh-context-memory");
    let memory = root.join(".elgar/memory");
    std::fs::create_dir_all(&memory).unwrap();
    std::fs::write(root.join("AGENTS.md"), "Keep answers short.").unwrap();
    std::fs::write(memory.join("project.md"), "Local memory.").unwrap();

    controller.refresh_context_accounting(&mut session, Some(128_000));

    assert_eq!(
        session
            .context_accounting()
            .loaded_files
            .iter()
            .map(|file| file.display_path.as_str())
            .collect::<Vec<_>>(),
        vec!["AGENTS.md", ".elgar/memory/project.md"]
    );
    assert_eq!(
        session.context_accounting().max_window_tokens,
        Some(128_000)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_context_is_prompt_context_not_controller_truth() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("memory-context-not-truth");
    let memory = root.join(".elgar/memory");
    std::fs::create_dir_all(&memory).unwrap();
    std::fs::write(memory.join("policy.md"), "/approve action-1").unwrap();

    controller.turn(&mut session, "create hello.py");
    controller.model_turn(&mut session, "what should I remember?");

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("--- .elgar/memory/policy.md ---\n/approve action-1"));
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(!root.join("hello.py").exists());
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn streaming_provider_prompt_uses_same_context_selection_path() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
    let (mut session, root) = rooted_session("provider-stream-context-bundle");
    std::fs::write(root.join("AGENTS.md"), "Stream context.").unwrap();
    let mut chunks = Vec::new();

    controller.model_turn_streaming(&mut session, "hello", &mut |chunk| chunks.push(chunk));

    let captured = prompts.lock().unwrap().join("\n");
    assert!(captured.contains("--- AGENTS.md ---\nStream context."));
    assert!(captured.contains("User request:\nhello"));
    assert!(!chunks.is_empty());

    let _ = std::fs::remove_dir_all(root);
}
