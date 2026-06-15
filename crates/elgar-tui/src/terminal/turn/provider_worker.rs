//! Runs harness turns outside the terminal input loop.
//!
//! The TUI stays responsive while this file owns the background worker thread
//! that calls the core harness and sends completion updates back to `provider.rs`.

use std::{path::PathBuf, sync::mpsc, thread};

use elgar_core::{
    event::Event,
    harness::run_harness_turn_with_cancel,
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

    pub(super) fn try_complete(&self) -> Result<Option<ProviderTurnUpdate>, String> {
        if self.cancel.is_canceled() {
            return Ok(Some(ProviderTurnUpdate::Canceled));
        }

        match self.receiver.try_recv() {
            Ok(ProviderTurnWorkerMessage::Complete(result)) => {
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
        let worker_started = std::time::Instant::now();
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
        let result = run_harness_turn_with_cancel(&provider, &mut session, &input, &worker_cancel);
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
                "event_count": result.events.len()
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
}
