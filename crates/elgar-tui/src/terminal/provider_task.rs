use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

#[cfg(test)]
use elgar_core::controller::Controller;
#[cfg(test)]
use elgar_core::provider::ProviderStreamChunk;
use elgar_core::{
    agent_runtime::AgentRuntime, event::Event, policy::PermissionPolicyMode,
    provider::ControllerProvider, session::Session,
};

pub(super) struct ProviderTurnTask {
    receiver: mpsc::Receiver<ProviderTurnWorkerMessage>,
    canceled: Arc<AtomicBool>,
}

impl ProviderTurnTask {
    /// Cancel at the UI/session boundary.
    ///
    /// This suppresses later chunks and final session updates. It does not
    /// currently abort an already-running provider socket in the worker thread.
    pub(super) fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    pub(super) fn try_complete(&self) -> Result<Option<ProviderTurnUpdate>, String> {
        if self.canceled.load(Ordering::SeqCst) {
            return Ok(Some(ProviderTurnUpdate::Canceled));
        }

        match self.receiver.try_recv() {
            #[cfg(test)]
            Ok(ProviderTurnWorkerMessage::Chunk(chunk)) => {
                Ok(Some(ProviderTurnUpdate::Chunk(chunk)))
            }
            Ok(ProviderTurnWorkerMessage::Complete(result)) => {
                if self.canceled.load(Ordering::SeqCst) {
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
    #[cfg(test)]
    Chunk(ProviderStreamChunk),
    Completed(Box<CompletedProviderTurn>),
    Canceled,
}

pub(super) struct CompletedProviderTurn {
    pub(super) session: Session,
    pub(super) events: Vec<Event>,
}

#[cfg(test)]
pub(super) fn start_provider_turn<P>(
    controller: Controller<P>,
    mut session: Session,
    input: String,
) -> ProviderTurnTask
where
    P: ControllerProvider + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let canceled = Arc::new(AtomicBool::new(false));
    let worker_canceled = Arc::clone(&canceled);

    thread::spawn(move || {
        let result = controller.model_turn_streaming(&mut session, &input, &mut |chunk| {
            if !worker_canceled.load(Ordering::SeqCst) {
                let _ = sender.send(ProviderTurnWorkerMessage::Chunk(chunk));
            }
        });
        if worker_canceled.load(Ordering::SeqCst) {
            return;
        }
        let _ = sender.send(ProviderTurnWorkerMessage::Complete(Ok(Box::new(
            CompletedProviderTurn {
                session,
                events: result.events,
            },
        ))));
    });

    ProviderTurnTask { receiver, canceled }
}

pub(super) fn start_model_first_turn<P>(
    runtime: AgentRuntime<P>,
    mut session: Session,
    input: String,
    policy_mode: PermissionPolicyMode,
) -> ProviderTurnTask
where
    P: ControllerProvider + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let canceled = Arc::new(AtomicBool::new(false));
    let worker_canceled = Arc::clone(&canceled);

    // Live TUI natural-language turns run through the agent runtime. The
    // controller-review model-first runtime is legacy smoke coverage only.
    thread::spawn(move || {
        let result = runtime.turn(&mut session, &input, policy_mode);
        if worker_canceled.load(Ordering::SeqCst) {
            return;
        }
        let _ = sender.send(ProviderTurnWorkerMessage::Complete(Ok(Box::new(
            CompletedProviderTurn {
                session,
                events: result.events,
            },
        ))));
    });

    ProviderTurnTask { receiver, canceled }
}

enum ProviderTurnWorkerMessage {
    #[cfg(test)]
    Chunk(ProviderStreamChunk),
    Complete(Result<Box<CompletedProviderTurn>, String>),
}
