use super::*;

#[test]
fn provider_turn_task_reports_canceled_without_applying_stale_result() {
    #[derive(Clone)]
    struct DelayedProvider;

    impl elgar_core::provider::ControllerProvider for DelayedProvider {
        fn request_metadata(&self) -> elgar_core::provider::ProviderRequestMetadata {
            elgar_core::provider::ProviderRequestMetadata::new(
                "delayed-provider",
                None,
                "delayed-request-1",
            )
        }

        fn chat(
            &self,
            _prompt: &str,
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
            Ok(elgar_core::event::ProviderOutput::new("late response"))
        }
    }

    let controller = Controller::new(DelayedProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::super::super::start_provider_turn(controller, session, "hello".to_string());

    task.cancel();
    std::thread::sleep(std::time::Duration::from_millis(30));

    assert!(matches!(
        task.try_complete().unwrap(),
        Some(ProviderTurnUpdate::Canceled)
    ));
}

#[test]
fn provider_turn_task_reports_streaming_chunks_before_completion() {
    #[derive(Clone)]
    struct StreamingProvider;

    impl elgar_core::provider::ControllerProvider for StreamingProvider {
        fn request_metadata(&self) -> elgar_core::provider::ProviderRequestMetadata {
            elgar_core::provider::ProviderRequestMetadata::new(
                "stream-provider",
                Some("model-a".to_string()),
                "stream-request-1",
            )
        }

        fn chat(
            &self,
            _prompt: &str,
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            Ok(elgar_core::event::ProviderOutput::new("Hello"))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            on_chunk(ProviderStreamChunk::Reasoning("Need greet.".to_string()));
            on_chunk(ProviderStreamChunk::Text("Hello".to_string()));
            Ok(elgar_core::event::ProviderOutput::new("Hello").with_thinking("Need greet."))
        }
    }

    let controller = Controller::new(StreamingProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::super::super::start_provider_turn(controller, session, "hello".to_string());
    let mut chunks = Vec::new();
    let completed = (0..20)
        .find_map(|_| {
            let result = task.try_complete().unwrap();
            match result {
                Some(ProviderTurnUpdate::Chunk(chunk)) => {
                    chunks.push(chunk);
                    None
                }
                Some(ProviderTurnUpdate::Completed(completed)) => Some(completed),
                Some(ProviderTurnUpdate::Canceled) => panic!("provider turn should complete"),
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    None
                }
            }
        })
        .expect("stream provider turn should complete");

    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("Need greet.".to_string()),
            ProviderStreamChunk::Text("Hello".to_string())
        ]
    );
    assert_eq!(completed.events.len(), 4);
}

#[test]
fn completed_provider_turn_uses_final_output_not_capped_live_preview() {
    #[derive(Clone)]
    struct LargeStreamingProvider;

    impl ControllerProvider for LargeStreamingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "large-stream-provider",
                Some("model-a".to_string()),
                "large-stream-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused"))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<ProviderOutput, ProviderError> {
            let final_text = format!(
                "UNCAPPED_PREFIX_{}UNCAPPED_SUFFIX",
                "x".repeat(LIVE_RESPONSE_PREVIEW_BYTES + 512)
            );
            on_chunk(ProviderStreamChunk::Text(final_text.clone()));
            Ok(ProviderOutput::new(final_text))
        }
    }

    let controller = Controller::new(LargeStreamingProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::super::super::start_provider_turn(controller, session, "hello".to_string());
    let completed = wait_for_completed_provider_turn(&task);
    let mut shell = TuiShell::new();

    shell.consume_events(&completed.events);

    let rendered = shell.render();
    assert!(rendered.contains("UNCAPPED_PREFIX_"));
    assert!(rendered.contains("UNCAPPED_SUFFIX"));
}
