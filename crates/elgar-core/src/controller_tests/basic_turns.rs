use super::*;

#[test]
fn records_user_input_and_route_for_unknown_turn() {
    let controller = Controller::default();
    let mut session = session();

    let result = controller.turn(&mut session, "   ");

    assert_eq!(result.route, Route::Unknown);
    assert_eq!(session.events().len(), 2);
    assert!(matches!(session.events()[0], Event::UserMessage(_)));
    assert!(matches!(session.events()[1], Event::AssistantMessage(_)));
    assert!(session.actions().is_empty());
    assert_eq!(session.provider_metadata(), None);
}

#[test]
fn ask_model_calls_provider_stub_and_records_provider_events() {
    let controller = Controller::new(ProviderStub::new("test-provider").with_model("stub-model"));
    let mut session = session();

    let result = controller.turn(&mut session, "what does this code do?");

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(result.events.len(), 4);
    assert!(matches!(result.events[0], Event::UserMessage(_)));
    assert!(matches!(result.events[1], Event::ProviderStarted(_)));
    assert!(matches!(result.events[2], Event::ProviderFinished(_)));
    assert!(matches!(result.events[3], Event::AssistantMessage(_)));
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .map(|metadata| metadata.provider.as_str()),
        Some("test-provider")
    );
    assert!(session.actions().is_empty());
}

#[test]
fn explicit_model_turn_sends_unclassified_chat_to_provider() {
    let controller = Controller::new(ProviderStub::new("test-provider"));
    let mut session = session();

    let result = controller.model_turn(&mut session, "sadsadad");

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(result.events.len(), 4);
    assert!(matches!(result.events[0], Event::UserMessage(_)));
    assert!(matches!(result.events[1], Event::ProviderStarted(_)));
    assert!(matches!(result.events[2], Event::ProviderFinished(_)));
    assert!(matches!(result.events[3], Event::AssistantMessage(_)));
    assert!(session.actions().is_empty());
}

#[test]
fn provider_text_is_recorded_as_provider_text_not_verified_truth() {
    let controller = Controller::default();
    let mut session = session();

    controller.turn(&mut session, "explain how to create hello.py");

    let provider_texts: Vec<&str> = session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ProviderFinished(finished) => Some(finished.output.text.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(provider_texts.len(), 1);
    assert!(provider_texts[0].contains("stub provider response"));
    assert!(session.actions().is_empty());
    assert!(session.actions().iter().all(|action| {
        !matches!(
            action.verified_result,
            Some(VerifiedActionResult::FileWritten { .. })
        )
    }));
}

#[test]
fn ask_model_assistant_message_is_provider_sourced() {
    let controller = Controller::default();
    let mut session = session();

    controller.turn(&mut session, "what is rust?");

    let provider_message = session.events().iter().find_map(|event| match event {
        Event::AssistantMessage(message) if message.source == AssistantMessageSource::Provider => {
            Some(message.content.as_str())
        }
        _ => None,
    });

    assert!(provider_message.is_some_and(|message| message.contains("stub provider response")));
}

#[test]
fn greeting_routes_to_stub_chat_with_no_network_guidance() {
    let controller = Controller::default();
    let mut session = session();

    let result = controller.turn(&mut session, "hello!");

    assert_eq!(result.route, Route::AskModel);
    assert!(session.actions().is_empty());
    assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(started) if started.provider == "stub-provider")));

    let provider_message = session.events().iter().find_map(|event| match event {
        Event::AssistantMessage(message) if message.source == AssistantMessageSource::Provider => {
            Some(message.content.as_str())
        }
        _ => None,
    });

    assert!(provider_message.is_some_and(|message| {
        message.contains("stub provider response (no-network) to: hello!")
            && message.contains("No live provider call was made")
            && message.contains("tui-controller-smoke")
    }));
}

#[test]
fn non_provider_routes_do_not_call_provider() {
    let controller = Controller::default();
    let mut session = session();

    for input in ["help", "approve", "reject", "create hello.py"] {
        let result = controller.turn(&mut session, input);
        assert_ne!(result.route, Route::AskModel);
    }

    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ProviderStarted(_) | Event::ProviderFinished(_)
    )));
    assert_eq!(session.provider_metadata(), None);
}

#[test]
fn provider_stub_turn_does_not_create_files() {
    let controller = Controller::default();
    let mut session = session();
    let path = std::env::temp_dir().join(format!(
        "elgar-provider-stub-{}-hello.py",
        std::process::id()
    ));

    assert!(!path.exists());

    controller.turn(&mut session, "explain how to write hello.py");

    assert!(!path.exists());
    assert!(session.actions().is_empty());
}
