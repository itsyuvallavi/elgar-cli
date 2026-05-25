use super::*;

#[test]
fn terminal_loop_starts_provider_text_turn_as_active_pulse() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what does the harness do?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert_eq!(shell.status.render_body(), "◐ working");
    assert!(shell.status.provider_active());
    assert!(shell
        .conversation
        .render_body()
        .contains("> what does the harness do?\n◐ working"));
    shell.status.advance_thinking_pulse();
    shell.conversation.advance_loading_pulse();
    assert_eq!(shell.status.render_body(), "◓ working");
    assert!(shell.conversation.render_body().contains("◓ working"));

    let task = pending_turn.take().unwrap();
    let completed = wait_for_completed_provider_turn(&task);

    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert_eq!(session.events().len(), completed.events.len());
    assert_eq!(shell.status.render_body(), "reply ready");
    assert!(!shell.status.provider_active());
    assert!(!shell.render().contains("User\n"));
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Model:"));
}

#[test]
fn terminal_loop_sends_unclassified_non_slash_text_to_provider() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "sadsadad",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert!(shell
        .conversation
        .render_body()
        .contains("> sadsadad\n◐ working"));
    assert!(!shell
        .conversation
        .render_body()
        .contains("Input was not recognized"));

    let task = pending_turn.take().unwrap();
    let completed = wait_for_completed_provider_turn(&task);

    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert_eq!(session.events().len(), completed.events.len());
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Input was not recognized"));
}

#[test]
fn terminal_loop_normal_turn_uses_agent_loop_not_legacy_controller_model_first() {
    #[derive(Clone)]
    struct MessageOnlyToolProvider;

    impl ControllerProvider for MessageOnlyToolProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "message-only-tool-provider",
                Some("model-a".to_string()),
                "message-only-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused chat response"))
        }

        fn chat_with_tools_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            panic!("live TUI normal turns must not use legacy controller model-first");
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<elgar_core::provider::ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, elgar_core::provider::ChatRole::Tool))
            {
                return Ok(ProviderOutput::new("Done."));
            }

            Ok(
                ProviderOutput::new("Creating live-guard.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "message-only-tool-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: serde_json::json!({
                            "target_path": "live-guard"
                        }),
                        assistant_summary: Some("create live-guard".to_string()),
                    },
                ]),
            )
        }
    }

    let controller = Controller::new(MessageOnlyToolProvider);
    let root = temp_root("terminal-agent-loop-not-legacy-controller");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called live-guard",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(root.join("live-guard").is_dir());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(!shell.render().contains("Approve to"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_keeps_prompt_marker_folder_plan_create_project_controller_owned() {
    let controller = Controller::default();
    let root = temp_root("terminal-folder-plan-execute-model-first");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "> create folder called helloworld",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(verified_folder.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(shell.render().contains("Created"));
    assert!(!shell.render().contains("Model-first tool call validated"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_model_first_same_folder_plan_uses_provider_tools_and_verified_folder() {
    let controller = Controller::default();
    let root = temp_root("terminal-same-folder-plan-model-first");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called helloworld",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(verified_folder.is_dir());

    let provider_events_before_plan = provider_event_count(&session);
    assert!(!handle_submitted_terminal_input_for_loop(
        "create a plan for a simple React TS project in the same folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > provider_events_before_plan);

    let plan_path = verified_folder.join("react-ts-project-plan.md");
    let applied_plan = session
        .actions()
        .iter()
        .find(|record| {
            record.action.state == ActionLifecycleState::Applied
                && matches!(record.action.request, ActionRequest::CreateFile(_))
        })
        .expect("same-folder plan should be applied");
    let ActionRequest::CreateFile(action) = &applied_plan.action.request else {
        panic!("same-folder plan should create a Markdown file");
    };
    assert_eq!(
        action.target_path,
        std::path::PathBuf::from("helloworld/react-ts-project-plan.md")
    );
    assert!(applied_plan.verified_result.is_some());
    assert!(plan_path.is_file());
    assert!(!root.join("react-ts-project-plan.md").exists());
    assert!(applied_plan
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.is_policy_approved()));
    assert!(!shell.render().contains("Approved."));
    assert!(!shell.render().contains("Creating the plan."));
    assert!(shell.render().contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_implements_verified_plan_inside_same_folder_without_approval() {
    let controller = Controller::default();
    let root = temp_root("terminal-implement-plan-model-first");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called helloworld",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(verified_folder.is_dir());

    assert!(!handle_submitted_terminal_input_for_loop(
        "create a plan for a simple React TS project in the same folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(verified_folder.join("react-ts-project-plan.md").is_file());

    assert!(!handle_submitted_terminal_input_for_loop(
        "implement the plan",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(verified_folder.join("package.json").is_file());
    assert!(verified_folder.join("src/App.tsx").is_file());
    assert!(!root.join("package.json").exists());
    assert!(!shell.render().contains("Approve to"));
    assert!(session.actions().iter().any(|record| {
        record.action.state == ActionLifecycleState::Applied
            && matches!(&record.action.request, ActionRequest::CreateFile(action) if action.target_path == std::path::PathBuf::from("helloworld/package.json"))
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_routes_unclassified_action_like_text_to_controller_not_provider() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create the local widget after setup",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Input was not recognized"));
}

#[test]
fn terminal_loop_polite_folder_request_uses_model_tool_path() {
    let controller = Controller::default();
    let root = temp_root("terminal-polished-folder-request");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create folder called review-guard",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(root.join("review-guard").is_dir());
    assert_eq!(session.actions().len(), 1);
    assert!(matches!(
        &session.actions()[0].action.request,
        ActionRequest::CreateDirectory(_)
    ));
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(session.actions()[0]
        .policy_decision
        .as_ref()
        .is_some_and(|decision| decision.is_policy_approved()));
    assert!(!shell.render().contains("Approved."));
    assert!(!shell.render().contains("Creating review-guard."));
    assert!(shell.render().contains("Created"));
    assert!(!shell.render().contains("Input was not recognized"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_broad_react_project_request_creates_with_provider_tools() {
    let controller = Controller::default();
    let root = temp_root("terminal-polished-react-project-request");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "can you please create a react project called demo",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(!session.actions().is_empty());
    assert!(root.join("demo").is_dir());
    assert!(root.join("demo/package.json").is_file());
    assert!(root.join("demo/src/App.tsx").is_file());
    assert!(!shell.render().contains("Approve to"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_repair_followup_uses_latest_project_folder_without_tool_chatter() {
    #[derive(Clone)]
    struct RepairProvider;

    impl ControllerProvider for RepairProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("repair-provider", Some("model-a".to_string()), "repair")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<elgar_core::provider::ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, elgar_core::provider::ChatRole::Tool))
            {
                return Ok(ProviderOutput::new("Done."));
            }

            let context = messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(context.contains("latest verified plan: demo/project-plan.md"));

            Ok(ProviderOutput::new(
                "We need create pages/index.tsx, styles/globals.css, tailwind config.",
            )
            .with_tool_calls(vec![
                RawModelToolCall {
                    id: "repair-page".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": "./pages/index.tsx",
                        "contents": "export default function Home() { return <main>Hello</main>; }\n"
                    }),
                    assistant_summary: Some("create missing page".to_string()),
                },
                RawModelToolCall {
                    id: "repair-css".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": "./styles/globals.css",
                        "contents": "@tailwind base;\n@tailwind components;\n@tailwind utilities;\n"
                    }),
                    assistant_summary: Some("create missing styles".to_string()),
                },
            ]))
        }
    }

    let setup_controller = Controller::default();
    let controller = Controller::new(RepairProvider);
    let root = temp_root("terminal-repair-followup-project-folder");
    let project = root.join("demo");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called demo",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(project.is_dir());

    assert!(!handle_submitted_terminal_input_for_loop(
        "create a plan for a project in the same folder",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(project.join("project-plan.md").is_file());

    assert!(!handle_submitted_terminal_input_for_loop(
        "i think you forgot some files",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    let rendered = shell.render();
    assert!(project.join("pages/index.tsx").is_file());
    assert!(project.join("styles/globals.css").is_file());
    assert!(!root.join("pages/index.tsx").exists());
    assert!(!root.join("styles/globals.css").exists());
    assert!(!rendered.contains("We need create"));
    assert!(!rendered.contains("Writing ./pages/index.tsx."));
    assert!(rendered.contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_model_first_guidance_renders_naturally_without_creating_files() {
    let controller = Controller::default();
    let root = temp_root("terminal-model-first-guidance");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create a project in that folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(session.actions().is_empty());
    assert!(!root.join("project").exists());
    assert!(shell
        .render()
        .contains("Which folder should I use for the project?"));
    assert!(!shell.render().contains("Proposed action"));
    assert!(!shell.render().contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_explicit_desktop_folder_guidance_prose_does_not_mutate_without_tool_call() {
    #[derive(Clone)]
    struct DesktopGuidanceProseProvider;

    impl ControllerProvider for DesktopGuidanceProseProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "desktop-guidance-prose-provider",
                Some("model-a".to_string()),
                "desktop-guidance-prose-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new(
                "Do you want the folder created in your home Desktop directory?",
            ))
        }

        fn chat_with_tools_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new(
                "Do you want the folder created in your home Desktop directory?",
            ))
        }
    }

    let controller = Controller::new(DesktopGuidanceProseProvider);
    let root = temp_root("terminal-explicit-desktop-folder-prose");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    let target = desktop.join("ElgarLiveE2E-20260524T111953Z");
    std::fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set("HOME", &home);
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "Create a folder on my Desktop called ElgarLiveE2E-20260524T111953Z",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());

    let rendered = shell.render();
    assert!(!target.exists());
    assert!(rendered.contains("Do you want the folder created"));
    assert!(session.actions().is_empty());

    let _ = std::fs::remove_dir_all(root);
}
