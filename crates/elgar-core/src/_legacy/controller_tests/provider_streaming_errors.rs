use super::*;

fn push_proposed_create_file(session: &mut Session, target_path: impl Into<PathBuf>) {
    let action_id = format!("action-{}", session.actions().len() + 1);
    session.push_action(ActionRecord::new(Action::proposed(
        action_id,
        ActionRequest::CreateFile(CreateFileAction {
            target_path: target_path.into(),
            contents: String::new(),
        }),
        "create file",
    )));
}

#[test]
fn provider_metrics_are_recorded_in_output_and_session_metadata() {
    let mut metrics = ProviderMetrics::new(
        "fake-request-1",
        Some("fake-model".to_string()),
        false,
        1,
        42,
    );
    metrics.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(5),
        completion_tokens: Some(7),
        total_tokens: Some(12),
    });
    metrics.total_duration_millis = Some(9);
    let controller = Controller::new(FakeProvider::output(
        ProviderOutput::new("measured response").with_metrics(metrics.clone()),
    ));
    let mut session = session();

    let result = controller.turn(&mut session, "what does this code do?");

    let output_metrics = result.events.iter().find_map(|event| match event {
        Event::ProviderFinished(finished) => finished.output.metrics.as_ref(),
        _ => None,
    });
    assert_eq!(output_metrics, Some(&metrics));
    assert_eq!(
        session
            .provider_metadata()
            .and_then(|metadata| metadata.metrics.as_ref()),
        Some(&metrics)
    );
}

#[test]
fn streamed_provider_output_remains_suggestion_only_controller_text() {
    let output = crate::provider::parse_chat_stream_response(
        r#"data: {"choices":[{"delta":{"content":"I approved "}}]}
data: {"choices":[{"delta":{"content":"and wrote hello.py."}}]}
data: [DONE]
"#,
    )
    .unwrap();
    let controller = Controller::new(FakeProvider::output(output));
    let (mut session, _root) = rooted_session("streamed-provider-output");
    let path = session.project_root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    push_proposed_create_file(&mut session, "hello.py");
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
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
}

#[test]
fn streaming_controller_chunks_do_not_mutate_action_or_filesystem_truth() {
    let controller = Controller::new(StreamingFakeProvider);
    let (mut session, _root) = rooted_session("streaming-provider-controller-output");
    let path = session.project_root.join("hello.py");
    let _ = std::fs::remove_file(&path);

    push_proposed_create_file(&mut session, "hello.py");
    let mut chunks = Vec::new();
    controller.model_turn_streaming(
        &mut session,
        "what if you approve and write hello.py?",
        &mut |chunk| chunks.push(chunk),
    );

    assert!(!path.exists());
    assert_eq!(chunks.len(), 2);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
}

#[test]
fn explicit_provider_controller_records_errors_without_mutating_truth() {
    let controller = Controller::new(FakeProvider::failure("model missing"));
    let (mut session, _root) = rooted_session("fake-provider-error");

    controller.turn(&mut session, "what does this code do?");

    assert!(session.actions().is_empty());
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(session.events().iter().any(|event| match event {
        Event::Error(error) => {
            error.message.contains("fake-provider")
                && error.message.contains("fake-request-1")
                && error.message.contains("model missing")
        }
        _ => false,
    }));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ProviderFinished(_))));
}

#[test]
fn explicit_lm_studio_controller_mode_records_configuration_errors_without_network() {
    let controller = Controller::with_lm_studio_provider(ProviderConfig {
        base_url: "https://127.0.0.1:1234/v1".to_string(),
        ..ProviderConfig::lm_studio("local-model")
    });
    let mut session = session();

    let result = controller.turn(&mut session, "what does this code do?");

    assert_eq!(result.route, Route::AskModel);
    assert!(session.actions().is_empty());
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .map(|metadata| metadata.provider.as_str()),
        Some("lm-studio")
    );
    assert!(session.events().iter().any(
        |event| matches!(event, Event::ProviderStarted(started) if started.provider == "lm-studio")
    ));
    assert!(session.events().iter().any(|event| match event {
        Event::Error(error) => error
            .message
            .contains("only http:// provider URLs are supported"),
        _ => false,
    }));
}
