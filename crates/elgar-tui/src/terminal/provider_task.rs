use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

use elgar_core::{
    controller::Controller, event::Event, provider::ControllerProvider, session::Session,
};

pub(super) struct ProviderTurnTask {
    receiver: mpsc::Receiver<Result<CompletedProviderTurn, String>>,
    canceled: Arc<AtomicBool>,
}

impl ProviderTurnTask {
    pub(super) fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    pub(super) fn try_complete(&self) -> Result<Option<ProviderTurnUpdate>, String> {
        if self.canceled.load(Ordering::SeqCst) {
            return Ok(Some(ProviderTurnUpdate::Canceled));
        }

        match self.receiver.try_recv() {
            Ok(result) => {
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
    Completed(CompletedProviderTurn),
    Canceled,
}

pub(super) struct CompletedProviderTurn {
    pub(super) session: Session,
    pub(super) events: Vec<Event>,
}

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
        let result = controller.model_turn(&mut session, &input);
        if worker_canceled.load(Ordering::SeqCst) {
            return;
        }
        let _ = sender.send(Ok(CompletedProviderTurn {
            session,
            events: result.events,
        }));
    });

    ProviderTurnTask { receiver, canceled }
}
