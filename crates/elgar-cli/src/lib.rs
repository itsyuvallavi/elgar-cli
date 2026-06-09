//! Public CLI library surface used by the `elgar` binary and CLI tests.
//!
//! This module re-exports startup and diagnostic helpers and owns the simple
//! single-turn CLI rendering path.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use elgar_core::{
    harness::run_harness_turn,
    provider::{LmStudioProvider, ProviderStub},
    renderer::render_session,
    session::Session,
};

mod diagnostics;
mod startup;

static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub use diagnostics::*;
pub use startup::*;

pub fn init_terminal_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "off"))
        .format_timestamp_millis()
        .try_init();
}

/// Runs one CLI prompt against the no-network stub provider.
///
/// This is used by smoke tests and by fallback paths when no live provider
/// config is available.
pub fn render_cli_turn(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    log::debug!(
        "render_cli_turn stub_provider input_chars={} project_root={} cwd={}",
        input.chars().count(),
        project_root.as_ref().display(),
        cwd.as_ref().display()
    );
    let provider = ProviderStub::default();
    let session_id = runtime_session_id("cli-smoke");
    let mut session = Session::new(&session_id, project_root.as_ref(), cwd.as_ref());

    run_harness_turn(&provider, &mut session, input);
    render_session(&session)
}

/// Runs one CLI prompt using `elgar-provider.json` when live config exists.
///
/// If runtime config is missing or disabled, this falls back to
/// `render_cli_turn` and uses the stub provider.
pub fn render_cli_turn_from_runtime_config(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, RuntimeProviderConfigError> {
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();
    let Some(runtime_provider) = load_runtime_provider(project_root_ref)? else {
        log::debug!(
            "runtime provider config missing; using stub provider project_root={}",
            project_root_ref.display()
        );
        return Ok(render_cli_turn(input, project_root_ref, cwd_ref));
    };

    log::info!(
        "render_cli_turn raw_provider provider={} model={:?} input_chars={} cwd={}",
        runtime_provider.config.provider,
        runtime_provider.config.model,
        input.chars().count(),
        cwd_ref.display()
    );
    let provider = LmStudioProvider::new(runtime_provider.config);
    let session_id = runtime_session_id("cli-runtime");
    let mut session = Session::new(&session_id, project_root_ref, cwd_ref);

    run_harness_turn(&provider, &mut session, input);
    Ok(render_session(&session))
}

/// Builds a unique session id for CLI-created sessions.
pub(crate) fn runtime_session_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let counter = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{millis}-{counter}", std::process::id())
}

#[cfg(test)]
mod tests;
