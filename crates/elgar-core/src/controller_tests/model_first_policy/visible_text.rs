use super::*;

#[test]
fn model_first_text_only_provider_output_records_text_and_no_action() {
    let (provider, received_tools, chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("I can help with that."));
    let controller = Controller::new(provider);
    let mut session = session();

    let result = controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "explain the project",
        PermissionPolicyMode::ReviewAll,
    );

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert_eq!(received_tools.lock().unwrap()[0].len(), 8);
    assert!(received_tools.lock().unwrap()[0].contains(&"ask_guidance".to_string()));
    assert!(received_tools.lock().unwrap()[0].contains(&"create_file".to_string()));
    assert!(session.actions().is_empty());
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert!(result.events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "I can help with that."
        )
    }));
}

#[test]
fn model_first_unrelated_text_still_uses_provider_tool_chat() {
    let (provider, received_tools, chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("provider answered"));
    let controller = Controller::new(provider);
    let mut session = session();

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "tell me about Rust ownership",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(*chat_calls.lock().unwrap(), 0);
    assert_eq!(received_tools.lock().unwrap().len(), 1);
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content == "provider answered"
        )
    }));
}

#[test]
fn model_first_tool_contract_prose_with_tool_call_is_not_rendered_as_provider_chat() {
    let output = ProviderOutput::new("Create directory. Use create_directory tool.")
        .with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "tool-contract-dir" }),
        )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-tool-contract-prose-hidden");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called tool-contract-dir",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(root.join("tool-contract-dir").is_dir());
    assert!(provider_assistant_messages(&session)
        .iter()
        .all(|message| !message.contains("create_directory tool")));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_provider_tool_contract_prose_is_not_rendered_as_provider_chat() {
    let controller = Controller::new(FakeProvider::success(
        "Create directory. Use create_directory tool.",
    ));
    let mut session = session();

    controller.model_turn(&mut session, "hello");

    assert!(provider_assistant_messages(&session)
        .iter()
        .all(|message| !message.contains("create_directory tool")));
}

#[test]
fn markdown_plan_provider_contract_line_is_removed_from_visible_provider_chat() {
    let controller = Controller::new(FakeProvider::success(
        "Output markdown content only.\n# Calculator Plan\n\n- Build a small UI.\n",
    ));
    let (mut session, root) = rooted_session("markdown-plan-provider-contract-hidden");

    controller.turn(
        &mut session,
        "create an md file with a plan to create a calculator UI using python",
    );

    let messages = provider_assistant_messages(&session);
    assert!(messages
        .iter()
        .all(|message| !message.contains("Output markdown content only")));
    assert!(messages
        .iter()
        .any(|message| message.contains("# Calculator Plan")));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn text_only_provider_answer_drops_instruction_lines() {
    let controller = Controller::new(FakeProvider::success(
        "Output markdown content only.\n# Plan\n\nUse create_file tool only after approval.",
    ));
    let mut session = session();

    controller.model_turn(&mut session, "write a plan");

    assert_eq!(provider_assistant_messages(&session), vec!["# Plan"]);
}

#[test]
fn normal_text_only_provider_answer_remains_visible_unchanged() {
    let answer = "Tool-call mode may use create_file tool, but this is a plain explanation.";
    let controller = Controller::new(FakeProvider::success(answer));
    let mut session = session();

    controller.model_turn(&mut session, "explain tool-call mode");

    assert_eq!(provider_assistant_messages(&session), vec![answer]);
}

#[test]
fn model_first_provider_prose_claiming_success_without_tool_call_creates_no_truth() {
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
        ProviderOutput::new("Done, I created success.txt and verified it."),
    );
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-prose-only");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create success.txt",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!root.join("success.txt").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
    )));
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("did not receive a tool call")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}
