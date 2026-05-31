use super::*;

#[test]
fn terminal_live_provider_dogfood_flow_keeps_provider_suggestions_and_actions_safe() {
    #[derive(Clone)]
    struct DogfoodProvider;

    impl ControllerProvider for DogfoodProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "dogfood-provider",
                Some("model-a".to_string()),
                "dogfood-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new(
                serde_json::json!({
                    "route": "chat",
                    "content": "Provider suggests creating hidden.py"
                })
                .to_string(),
            ))
        }

        fn chat_messages_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<ProviderOutput, ProviderError> {
            self.chat("")
        }

        fn chat_with_tools_with_metadata(
            &self,
            prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if prompt.contains("create file approved.py") {
                return Ok(
                    ProviderOutput::new("Creating approved.py.").with_tool_calls(vec![
                        RawModelToolCall {
                            id: "dogfood-tool-call-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: serde_json::json!({
                                "target_path": "approved.py",
                                "contents": ""
                            }),
                            assistant_summary: Some("create approved.py".to_string()),
                        },
                    ]),
                );
            }

            if prompt.contains("create file rejected.py") {
                return Ok(
                    ProviderOutput::new("Creating rejected.py.").with_tool_calls(vec![
                        RawModelToolCall {
                            id: "dogfood-tool-call-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: serde_json::json!({
                                "target_path": "rejected.py",
                                "contents": ""
                            }),
                            assistant_summary: Some("create rejected.py".to_string()),
                        },
                    ]),
                );
            }

            Ok(ProviderOutput::new("Provider suggests creating hidden.py")
                .with_thinking("Need answer without mutating files."))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<ProviderOutput, ProviderError> {
            on_chunk(ProviderStreamChunk::Reasoning(
                "Need answer without mutating files.".to_string(),
            ));
            on_chunk(ProviderStreamChunk::Text(
                "Provider suggests creating hidden.py".to_string(),
            ));
            Ok(ProviderOutput::new("Provider suggests creating hidden.py")
                .with_thinking("Need answer without mutating files."))
        }
    }

    let controller = Controller::new(DogfoodProvider);
    let root = temp_root("terminal-live-dogfood-flow");
    let hidden_target = root.join("hidden.py");
    let rejected_target = root.join("rejected.py");
    let approved_target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what should we create?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert!(shell.render().contains("◐ working"));

    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(chunks.is_empty());
    assert!(shell
        .render()
        .contains("Provider suggests creating hidden.py"));
    assert!(session.actions().is_empty());
    assert!(!hidden_target.exists());

    let mut input = TerminalInput::default();
    let mut output = Vec::new();
    for character in "/copy".chars() {
        let exited = handle_terminal_key_with_copy_writer(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
            &mut output,
        );
        assert!(!exited);
    }
    let exited = handle_terminal_key_with_copy_writer(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
        &mut output,
    );
    assert!(!exited);
    assert!(shell.copy.render_hint().starts_with("copied conversation"));
    assert!(!output.is_empty());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/clear",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(shell.conversation.lines.is_empty());
    assert!(session.actions().is_empty());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create file rejected.py",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(rejected_target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create file approved.py",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(approved_target.exists());
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );

    assert!(handle_submitted_terminal_input_for_loop(
        "/q",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_provider_visible_transcript_keeps_plain_tool_like_prose_without_execution() {
    #[derive(Clone)]
    struct VisibleContractProvider {
        tool_turns: Arc<AtomicUsize>,
    }

    impl ControllerProvider for VisibleContractProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "visible-contract-provider",
                Some("model-a".to_string()),
                "visible-contract-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("Here is the useful provider answer."))
        }

        fn chat_messages_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<ProviderOutput, ProviderError> {
            let user_request = latest_user_message(&messages);
            if user_request.contains("show tool chatter") {
                return Ok(ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": "Use create_file tool. Provide tool calls. Create files: package.json?"
                    })
                    .to_string(),
                ));
            }

            Ok(ProviderOutput::new(
                serde_json::json!({
                    "route": "chat",
                    "content": "Here is the useful provider answer."
                })
                .to_string(),
            ))
        }

        fn chat_with_tools_with_metadata(
            &self,
            prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            let user_request = prompt
                .rsplit_once("User request:\n")
                .map(|(_context, request)| request.trim())
                .unwrap_or(prompt);

            if user_request.contains("show tool chatter") {
                return Ok(ProviderOutput::new(
                    "Use create_file tool. Provide tool calls. Create files: package.json?",
                ));
            }

            if self.tool_turns.fetch_add(1, Ordering::SeqCst) == 0
                && user_request.contains("create file protocol.py")
            {
                return Ok(ProviderOutput::new(
                    "Use create_file tool. Output markdown content only. Creating protocol.py.",
                )
                .with_tool_calls(vec![RawModelToolCall {
                    id: "visible-contract-tool-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": "protocol.py",
                        "contents": "print('ok')\n"
                    }),
                    assistant_summary: Some("create protocol.py".to_string()),
                }]));
            }

            Ok(ProviderOutput::new("Here is the useful provider answer."))
        }
    }

    let controller = Controller::new(VisibleContractProvider {
        tool_turns: Arc::new(AtomicUsize::new(0)),
    });
    let root = temp_root("terminal-provider-visible-contract");
    let target = root.join("protocol.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "/tool create file protocol.py",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    let rendered = shell.conversation.render_body();
    assert!(target.exists());
    assert!(rendered.contains("Wrote "));

    assert!(!handle_submitted_terminal_input_for_loop(
        "show tool chatter",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("Use create_file tool"));
    assert!(rendered.contains("Provide tool calls"));
    assert!(rendered.contains("Create files: package.json?"));
    assert_eq!(session.actions().len(), 1);

    assert!(!handle_submitted_terminal_input_for_loop(
        "what happened?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    let rendered = shell.conversation.render_body();
    assert!(rendered.contains("Here is the useful provider answer."));
    assert_eq!(session.actions().len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_live_provider_dogfood_error_does_not_mutate_actions_or_files() {
    #[derive(Clone)]
    struct TimeoutProvider;

    impl ControllerProvider for TimeoutProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "timeout-provider",
                Some("model-a".to_string()),
                "timeout-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Err(ProviderError::network("provider request timed out"))
        }

        fn chat_with_tools_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            Err(ProviderError::network("provider request timed out"))
        }
    }

    let controller = Controller::new(TimeoutProvider);
    let root = temp_root("terminal-live-dogfood-error");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "hello",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    let rendered = shell.render();

    assert!(chunks.is_empty());
    assert!(rendered.contains("Provider error · timeout-provider"));
    assert!(rendered.contains("provider request timed out"));
    assert!(session.actions().is_empty());
    assert!(root_has_no_user_files(&root));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_cancel_drops_pending_provider_turn_without_session_mutation() {
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
    assert!(shell.status.provider_active());

    let exited = handle_submitted_terminal_input_for_loop(
        "/cancel",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_none());
    assert!(session.events().is_empty());
    assert_eq!(shell.status.render_body(), "canceled");
    assert!(!shell.status.provider_active());
    assert!(shell.render().contains("Provider request canceled."));
    assert!(!shell.render().contains("stub provider response"));
}

#[test]
fn terminal_loop_cancel_drops_late_provider_completion_from_visible_and_session_path() {
    #[derive(Clone)]
    struct SlowProvider;

    impl ControllerProvider for SlowProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("slow-provider", None, "slow-request-1")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            std::thread::sleep(std::time::Duration::from_millis(30));
            Ok(ProviderOutput::new("late stale response"))
        }
    }

    let controller = Controller::new(SlowProvider);
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

    let exited = handle_submitted_terminal_input_for_loop(
        "/cancel",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );
    std::thread::sleep(std::time::Duration::from_millis(60));

    assert!(!exited);
    assert!(pending_turn.is_none());
    assert!(session.events().is_empty());
    assert!(session.actions().is_empty());
    assert_eq!(shell.status.render_body(), "canceled");
    assert!(shell.render().contains("Provider request canceled."));
    assert!(!shell.render().contains("late stale response"));
    assert!(!shell.render().contains("slow-provider"));
}
