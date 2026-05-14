use std::path::Path;

use elgar_core::{controller::Controller, renderer::render_session, session::Session};

pub fn render_cli_turn(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let controller = Controller::default();
    let mut session = Session::new("cli-smoke-session", project_root.as_ref(), cwd.as_ref());

    controller.turn(&mut session, input);
    render_session(&session)
}
