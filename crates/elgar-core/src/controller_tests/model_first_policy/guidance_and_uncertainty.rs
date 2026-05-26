use super::*;

#[test]
fn model_first_capability_question_create_directory_tool_call_does_not_mutate() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "new_folder" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-capability-question-create");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "can you create a folder for me?",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!root.join("new_folder").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )
    }));
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("imperative request")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_guidance_tool_only_asks_question_and_creates_no_action() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-guidance",
        RawModelToolName::Known(ModelToolName::AskGuidance),
        json!({
            "question": "Which folder should I use?",
            "reason": "No verified folder is available."
        }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-guidance-tool-only");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a project in that folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )
    }));
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content == "Which folder should I use?"
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_guidance_plus_action_blocks_mutation_and_asks_question() {
    let output = ProviderOutput::new("").with_tool_calls(vec![
        raw_model_tool_call(
            "call-guidance",
            RawModelToolName::Known(ModelToolName::AskGuidance),
            json!({ "question": "Which folder should I use?" }),
        ),
        raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "guessed-folder" }),
        ),
    ]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-guidance-plus-action");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a project in that folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!root.join("guessed-folder").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content == "Which folder should I use?"
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_uncertainty_text_with_action_blocks_mutation() {
    let output = ProviderOutput::new("I'm not sure which folder you mean, but I will create this.")
        .with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "uncertain-folder" }),
        )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-uncertain-action");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a project in that folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(!root.join("uncertain-folder").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )
    }));
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller
                    && message.content.contains("clarification")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_stub_ambiguous_that_folder_request_does_not_fake_tool_guidance() {
    let controller = Controller::default();
    let (mut session, root) = rooted_session("model-first-that-folder-guidance");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a project in that folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(session.actions().is_empty());
    assert!(!root.join("project").exists());
    assert!(session.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
                    && message.content.contains("stub provider response")
        )
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_imperative_create_directory_still_auto_creates() {
    let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "imperative_folder" }),
    )]);
    let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
    let controller = Controller::new(provider);
    let (mut session, root) = rooted_session("model-first-imperative-create-dir");

    controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called imperative_folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert!(root.join("imperative_folder").is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_first_truth_guard_replaces_false_folder_denial_after_verified_create() {
    let create_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
        "call-create-dir",
        RawModelToolName::Known(ModelToolName::CreateDirectory),
        json!({ "target_path": "truth_folder" }),
    )]);
    let (create_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(create_output);
    let create_controller = Controller::new(create_provider);
    let (mut session, root) = rooted_session("model-first-truth-guard-folder");

    create_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "create a folder called truth_folder",
        PermissionPolicyMode::AutoCreateReviewModify,
    );
    assert!(root.join("truth_folder").is_dir());

    let (deny_provider, _received_tools, _chat_calls) =
        ToolEnabledFakeProvider::new(ProviderOutput::new("No folder was created."));
    let deny_controller = Controller::new(deny_provider);
    deny_controller.legacy_controller_model_first_turn_with_policy(
        &mut session,
        "i was just asking! i didnt tell you to do it!",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let visible_messages = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message) => Some(message.content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible_messages
        .to_ascii_lowercase()
        .contains("no folder was created"));
    assert!(visible_messages.contains("Filesystem truth:"));
    assert!(!render_session(&session)
        .to_ascii_lowercase()
        .contains("no folder was created"));

    let _ = std::fs::remove_dir_all(root);
}
