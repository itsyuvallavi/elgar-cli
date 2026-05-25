use std::path::Path;

use elgar_core::{
    controller::{Controller, TurnResult},
    provider::{ControllerProvider, ProviderConfig},
    session::Session,
};

use crate::shell::TuiShell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiControllerSmoke {
    pub session: Session,
    pub turn: TurnResult,
    pub rendered: String,
}

pub fn run_default_controller_smoke(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> TuiControllerSmoke {
    let controller = Controller::default();
    run_controller_smoke(&controller, input, project_root, cwd)
}

pub fn run_lm_studio_controller_smoke(
    config: ProviderConfig,
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> TuiControllerSmoke {
    let controller = Controller::with_lm_studio_provider(config);
    run_controller_smoke(&controller, input, project_root, cwd)
}

pub fn run_controller_smoke<P>(
    controller: &Controller<P>,
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> TuiControllerSmoke
where
    P: ControllerProvider,
{
    let mut session = Session::new(
        "tui-controller-smoke-session",
        project_root.as_ref(),
        cwd.as_ref(),
    );
    let mut shell = TuiShell::new();
    let turn = controller.turn(&mut session, input);
    shell.consume_events(&turn.events);
    let rendered = shell.render();

    TuiControllerSmoke {
        session,
        turn,
        rendered,
    }
}
