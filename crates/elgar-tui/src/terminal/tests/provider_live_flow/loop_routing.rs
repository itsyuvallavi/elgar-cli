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
fn terminal_loop_normal_action_like_text_stays_plain_without_tools() {
    #[derive(Clone)]
    struct PlainOnlyProvider;

    impl ControllerProvider for PlainOnlyProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "plain-only-provider",
                Some("model-a".to_string()),
                "plain-only-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused chat response"))
        }

        fn chat_messages_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("plain runtime response"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            panic!("normal TUI text must not use the tool-enabled provider path");
        }
    }

    let controller = Controller::new(PlainOnlyProvider);
    let root = temp_root("terminal-normal-text-plain");
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

    assert!(!root.join("live-guard").exists());
    assert!(session.actions().is_empty());
    assert!(shell.render().contains("plain runtime response"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_explicit_tool_turn_creates_directory() {
    let controller = scripted_tool_controller(vec![scripted_create_directory_output(
        "create-helloworld",
        "helloworld",
    )]);
    let root = temp_root("terminal-explicit-tool-create-directory");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called helloworld",
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
fn terminal_loop_explicit_tool_turn_can_create_project_files() {
    let controller = scripted_tool_controller(vec![ProviderOutput::new("Creating demo project.")
        .with_tool_calls(vec![
            RawModelToolCall {
                id: "create-demo-dir".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: serde_json::json!({
                    "target_path": "demo"
                }),
                assistant_summary: Some("create demo".to_string()),
            },
            RawModelToolCall {
                id: "create-demo-package".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": "demo/package.json",
                    "contents": "{\"scripts\":{\"dev\":\"vite\"}}\n"
                }),
                assistant_summary: Some("write demo/package.json".to_string()),
            },
            RawModelToolCall {
                id: "create-demo-app".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": "demo/src/App.tsx",
                    "contents": "export function App() { return <main>Demo</main>; }\n"
                }),
                assistant_summary: Some("write demo/src/App.tsx".to_string()),
            },
        ])]);
    let root = temp_root("terminal-explicit-tool-react-project");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create a react project called demo",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
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
fn terminal_loop_normal_action_like_text_goes_to_plain_provider() {
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
    assert!(session.actions().is_empty());
}

#[test]
fn terminal_loop_normal_file_request_does_not_create_without_tool_command() {
    let controller = scripted_tool_controller(vec![scripted_create_directory_output(
        "unexpected-tool-call",
        "review-guard",
    )]);
    let root = temp_root("terminal-normal-file-request-no-tool");
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
    assert!(!root.join("review-guard").exists());
    assert!(session.actions().is_empty());
    assert!(shell.render().contains("scripted provider response"));
    assert!(!shell.render().contains("Input was not recognized"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_explicit_tool_turn_can_use_verified_project_memory() {
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
                        "target_path": "demo/pages/index.tsx",
                        "contents": "export default function Home() { return <main>Hello</main>; }\n"
                    }),
                    assistant_summary: Some("create missing page".to_string()),
                },
                RawModelToolCall {
                    id: "repair-css".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": "demo/styles/globals.css",
                        "contents": "@tailwind base;\n@tailwind components;\n@tailwind utilities;\n"
                    }),
                    assistant_summary: Some("create missing styles".to_string()),
                },
            ]))
        }
    }

    let setup_controller = scripted_tool_controller(vec![
        scripted_create_directory_output("create-demo", "demo"),
        scripted_create_file_output("create-plan", "demo/project-plan.md", "# Plan\n"),
    ]);
    let controller = Controller::new(RepairProvider);
    let root = temp_root("terminal-repair-followup-project-folder");
    let project = root.join("demo");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called demo",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(project.is_dir());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create a plan for a project in the same folder",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(project.join("project-plan.md").is_file());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool i think you forgot some files",
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
    assert!(!rendered.contains("Writing demo/pages/index.tsx."));
    assert!(rendered.contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_verified_plan_preflight_blocks_wrong_project_root() {
    #[derive(Clone)]
    struct WrongProjectProvider;

    impl ControllerProvider for WrongProjectProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "wrong-project-provider",
                Some("model-a".to_string()),
                "wrong",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            _messages: Vec<elgar_core::provider::ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("Creating missing file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "wrong-project-file".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": "other/pages/index.tsx",
                        "contents": "export default function Home() { return <main>Wrong</main>; }\n"
                    }),
                    assistant_summary: Some("create missing page".to_string()),
                },
            ]))
        }
    }

    let setup_controller = scripted_tool_controller(vec![
        scripted_create_directory_output("create-demo", "demo"),
        scripted_create_file_output("create-plan", "demo/project-plan.md", "# Plan\n"),
    ]);
    let controller = Controller::new(WrongProjectProvider);
    let root = temp_root("terminal-plan-preflight-wrong-root");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create folder called demo",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create a plan in demo",
        &setup_controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    let action_count_after_plan = session.actions().len();

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool continue from the verified plan",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    let rendered = shell.render();
    assert_eq!(session.actions().len(), action_count_after_plan);
    assert!(!root.join("other/pages/index.tsx").exists());
    assert!(rendered.contains("verified plan is rooted at demo"));
    assert!(rendered.contains("outside that project"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_normal_project_request_does_not_create_files() {
    let controller = Controller::default();
    let root = temp_root("terminal-normal-project-request");
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
    assert!(shell.render().contains("stub provider response"));
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
