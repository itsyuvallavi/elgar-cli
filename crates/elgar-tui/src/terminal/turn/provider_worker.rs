//! Runs harness turns outside the terminal input loop.
//!
//! The TUI stays responsive while this file owns the background worker thread
//! that calls the core harness and sends completion updates back to `provider.rs`.

use std::{path::PathBuf, sync::mpsc, thread, time::Instant};

use elgar_core::{
    event::Event,
    harness::run_harness_turn_with_cancel_and_stream,
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

pub(super) struct ProviderTurnTask {
    receiver: mpsc::Receiver<ProviderTurnWorkerMessage>,
    cancel: ProviderCancelToken,
    project_root: PathBuf,
    session_id: String,
    turn_id: u64,
}

impl ProviderTurnTask {
    /// Cancel the active turn and ask the provider transport to abort.
    pub(super) fn cancel(&self) {
        self.cancel.cancel();
        let _ = append_log_event(
            &self.project_root,
            &self.session_id,
            LogInput::new(
                self.turn_id,
                LogPhase::Worker,
                file!(),
                "ProviderTurnTask::cancel",
                "provider_worker_cancel_requested",
            ),
        );
    }

    /// Cancel the active turn after the interactive watchdog expires.
    pub(super) fn cancel_for_watchdog(&self, timeout_millis: u64) {
        self.cancel.cancel();
        let _ = append_log_event(
            &self.project_root,
            &self.session_id,
            LogInput::new(
                self.turn_id,
                LogPhase::Worker,
                file!(),
                "ProviderTurnTask::cancel_for_watchdog",
                "provider_worker_watchdog_timeout",
            )
            .with_metadata(serde_json::json!({
                "timeout_millis": timeout_millis
            })),
        );
    }

    pub(super) fn try_complete(&self) -> Result<Option<ProviderTurnUpdate>, String> {
        if self.cancel.is_canceled() {
            return Ok(Some(ProviderTurnUpdate::Canceled));
        }

        match self.receiver.try_recv() {
            Ok(ProviderTurnWorkerMessage::Stream(event)) => {
                Ok(Some(ProviderTurnUpdate::Stream(event)))
            }
            Ok(ProviderTurnWorkerMessage::Complete(result)) => {
                let metadata = provider_worker_completion_metadata(&result);
                let _ = append_log_event(
                    &self.project_root,
                    &self.session_id,
                    LogInput::new(
                        self.turn_id,
                        LogPhase::Worker,
                        file!(),
                        "ProviderTurnTask::try_complete",
                        "provider_worker_completion_received",
                    )
                    .with_metadata(metadata),
                );
                if self.cancel.is_canceled() {
                    Ok(Some(ProviderTurnUpdate::Canceled))
                } else {
                    result.map(|completed| Some(ProviderTurnUpdate::Completed(completed)))
                }
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("provider request worker disconnected".to_string())
            }
        }
    }
}

pub(super) enum ProviderTurnUpdate {
    Completed(Box<CompletedProviderTurn>),
    Stream(Box<Event>),
    Canceled,
}

pub(super) struct CompletedProviderTurn {
    pub(super) session: Session,
    pub(super) events: Vec<Event>,
}

pub(super) fn start_harness_turn<P>(
    provider: P,
    mut session: Session,
    input: String,
) -> ProviderTurnTask
where
    P: ControllerProvider + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let cancel = ProviderCancelToken::new();
    let worker_cancel = cancel.clone();
    let turn_id = session.next_turn_id();
    let project_root = session.project_root.clone();
    let session_id = session.id.clone();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Worker,
            file!(),
            "start_harness_turn",
            "provider_worker_spawned",
        )
        .with_metadata(serde_json::json!({
            "input_chars": input.chars().count()
        })),
    );

    thread::spawn(move || {
        let worker_started = Instant::now();
        let _ = append_log_event(
            &session.project_root,
            &session.id,
            LogInput::new(
                turn_id,
                LogPhase::Worker,
                file!(),
                "start_harness_turn",
                "provider_worker_started",
            ),
        );
        let stream_sender = sender.clone();
        let mut stream_events = move |event: Event| {
            let _ = stream_sender.send(ProviderTurnWorkerMessage::Stream(Box::new(event)));
        };
        let result = run_harness_turn_with_cancel_and_stream(
            &provider,
            &mut session,
            &input,
            &worker_cancel,
            &mut stream_events,
        );
        if worker_cancel.is_canceled() {
            let _ = append_log_event(
                &session.project_root,
                &session.id,
                LogInput::new(
                    turn_id,
                    LogPhase::Worker,
                    file!(),
                    "start_harness_turn",
                    "provider_worker_canceled",
                )
                .with_duration_ms(worker_started.elapsed().as_millis() as u64),
            );
            return;
        }
        let _ = append_log_event(
            &session.project_root,
            &session.id,
            LogInput::new(
                turn_id,
                LogPhase::Worker,
                file!(),
                "start_harness_turn",
                "provider_worker_finished",
            )
            .with_duration_ms(worker_started.elapsed().as_millis() as u64)
            .with_metadata(serde_json::json!({
                "event_count": result.events.len(),
                "provider_started_count": count_events(&result.events, is_provider_started),
                "provider_finished_count": count_events(&result.events, is_provider_finished),
                "assistant_message_count": count_events(&result.events, is_assistant_message),
                "latest_provider_request_id": latest_provider_request_id(&result.events),
            })),
        );
        let _ = sender.send(ProviderTurnWorkerMessage::Complete(Ok(Box::new(
            CompletedProviderTurn {
                session,
                events: result.events,
            },
        ))));
    });

    ProviderTurnTask {
        receiver,
        cancel,
        project_root,
        session_id,
        turn_id,
    }
}

enum ProviderTurnWorkerMessage {
    Complete(Result<Box<CompletedProviderTurn>, String>),
    Stream(Box<Event>),
}

fn provider_worker_completion_metadata(
    result: &Result<Box<CompletedProviderTurn>, String>,
) -> serde_json::Value {
    match result {
        Ok(completed) => serde_json::json!({
            "status": "ok",
            "event_count": completed.events.len(),
            "provider_started_count": count_events(&completed.events, is_provider_started),
            "provider_finished_count": count_events(&completed.events, is_provider_finished),
            "assistant_message_count": count_events(&completed.events, is_assistant_message),
            "latest_provider_request_id": latest_provider_request_id(&completed.events),
        }),
        Err(message) => serde_json::json!({
            "status": "error",
            "error": message,
        }),
    }
}

fn count_events(events: &[Event], predicate: fn(&Event) -> bool) -> usize {
    events.iter().filter(|event| predicate(event)).count()
}

fn is_provider_started(event: &Event) -> bool {
    matches!(event, Event::ProviderStarted(_))
}

fn is_provider_finished(event: &Event) -> bool {
    matches!(event, Event::ProviderFinished(_))
}

fn is_assistant_message(event: &Event) -> bool {
    matches!(event, Event::AssistantMessage(_))
}

fn latest_provider_request_id(events: &[Event]) -> Option<&str> {
    events.iter().rev().find_map(|event| match event {
        Event::ProviderFinished(finished) => Some(finished.request_id.as_str()),
        Event::ProviderStarted(started) => Some(started.request_id.as_str()),
        _ => None,
    })
}
