use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    action::ActionLifecycleState,
    controller::Controller,
    event::{Event, ProviderOutput, VerifiedActionResult},
    provider::{
        ControllerProvider, ProviderConfig, ProviderError, ProviderRequestMetadata, ProviderStub,
    },
    router::Route,
    session::Session,
};
use elgar_tui::{
    run_controller_smoke, run_default_controller_smoke, run_lm_studio_controller_smoke, TuiShell,
};

fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-tui-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("tui-smoke-session", root, root)
}

#[derive(Debug, Clone)]
struct FailingProvider;

impl ControllerProvider for FailingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "fake-provider",
            Some("fake-model".to_string()),
            "fake-request-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Err(ProviderError::provider("model missing", Some(404), None))
    }
}

#[derive(Debug, Clone)]
struct ClaimingProvider;

impl ControllerProvider for ClaimingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "claiming-provider",
            Some("claiming-model".to_string()),
            "claiming-request-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new(
            "I wrote hello.py and applied the action successfully.",
        ))
    }
}

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[test]
fn renders_initial_state() {
    let shell = TuiShell::new();
    let rendered = shell.render();

    assert!(rendered.contains("[Conversation]\n(empty conversation)"));
    assert!(rendered.contains("[Pending Action]\nnone"));
    assert!(rendered.contains("[Status]\nready"));
    assert!(rendered.contains("[Input]\n> "));
}

#[test]
fn renders_core_events_from_controller_turns() {
    let controller = Controller::default();
    let root = smoke_root("render-core-events");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what does the harness do?");
    shell.consume_events(&result.events);

    let rendered = shell.render();
    assert!(rendered.contains("You: what does the harness do?"));
    assert!(rendered
        .contains("Provider progress: working with stub-provider (request stub-request-1)."));
    assert!(rendered.contains(
        "Provider progress: response ready from stub-provider (request stub-request-1). Provider text is suggestion only."
    ));
    assert!(rendered.contains("Assistant suggestion: stub provider response"));
    assert!(rendered.contains("[Status]\nreply ready"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn default_controller_smoke_uses_stub_even_when_lm_studio_env_is_set() {
    let _model = EnvGuard::set(
        "ELGAR_LM_STUDIO_MODEL",
        "loaded-model-that-must-not-be-used",
    );
    let _base_url = EnvGuard::set("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1");
    let root = smoke_root("default-smoke-env");

    let smoke = run_default_controller_smoke("what does the harness do?", &root, &root);

    assert_eq!(smoke.turn.route, Route::AskModel);
    assert_eq!(
        smoke
            .session
            .provider_metadata()
            .map(|metadata| metadata.provider.as_str()),
        Some("stub-provider")
    );
    assert_eq!(
        smoke
            .session
            .provider_metadata()
            .and_then(|metadata| metadata.request_id.as_deref()),
        Some("stub-request-1")
    );
    assert!(smoke
        .rendered
        .contains("Provider progress: working with stub-provider (request stub-request-1)."));
    assert!(smoke.rendered.contains("Provider text is suggestion only."));
    assert!(smoke
        .rendered
        .contains("Assistant suggestion: stub provider response"));
    assert!(!smoke.rendered.contains("lm-studio"));
    assert!(!smoke
        .rendered
        .contains("only http:// provider URLs are supported"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_controller_smoke_uses_the_passed_controller() {
    let controller = Controller::new(ProviderStub::new("tui-smoke-provider").with_model("model-a"));
    let root = smoke_root("explicit-smoke-controller");

    let smoke = run_controller_smoke(&controller, "what does this do?", &root, &root);

    assert_eq!(smoke.turn.route, Route::AskModel);
    assert_eq!(
        smoke
            .session
            .provider_metadata()
            .map(|metadata| metadata.provider.as_str()),
        Some("tui-smoke-provider")
    );
    assert_eq!(
        smoke
            .session
            .provider_metadata()
            .and_then(|metadata| metadata.model.as_deref()),
        Some("model-a")
    );
    assert!(smoke.rendered.contains("You: what does this do?"));
    assert!(smoke
        .rendered
        .contains("Provider progress: working with tui-smoke-provider (request stub-request-1)."));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_lm_studio_tui_smoke_renders_through_tui_shell_without_network() {
    let root = smoke_root("explicit-lm-studio-no-network");

    let smoke = run_lm_studio_controller_smoke(
        ProviderConfig {
            model: Some("local-model".to_string()),
            base_url: "https://127.0.0.1:1234/v1".to_string(),
            ..ProviderConfig::default()
        },
        "Say hello in one sentence.",
        &root,
        &root,
    );

    assert_eq!(smoke.turn.route, Route::AskModel);
    assert_eq!(
        smoke
            .session
            .provider_metadata()
            .map(|metadata| metadata.provider.as_str()),
        Some("lm-studio")
    );
    assert!(smoke.rendered.contains("You: Say hello in one sentence."));
    assert!(smoke
        .rendered
        .contains("Provider progress: working with lm-studio (request lm-studio-request-1)."));
    assert!(smoke.rendered.contains(
        "Provider error from lm-studio: Configuration provider error: only http:// provider URLs are supported"
    ));
    assert!(smoke.rendered.contains("[Status]\nprovider error"));
    assert!(!smoke.rendered.contains("stub-provider"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn submits_user_input_through_shared_controller() {
    let controller = Controller::new(ProviderStub::new("smoke-provider").with_model("smoke-model"));
    let root = smoke_root("submit-input");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = shell.submit_input(&controller, &mut session, "what does this do?");

    assert_eq!(result.route, Route::AskModel);
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .map(|metadata| metadata.provider.as_str()),
        Some("smoke-provider")
    );
    assert_eq!(
        session
            .provider_metadata()
            .as_ref()
            .and_then(|metadata| metadata.model.as_deref()),
        Some("smoke-model")
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(shell.render().contains("You: what does this do?"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn renders_provider_error_events_without_network() {
    let controller = Controller::new(FailingProvider);
    let root = smoke_root("provider-error");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = shell.submit_input(&controller, &mut session, "what does this do?");

    assert_eq!(result.route, Route::AskModel);
    assert!(session.actions().is_empty());
    assert!(session.events().iter().any(|event| match event {
        Event::Error(error) => error.message.contains("model missing"),
        _ => false,
    }));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ProviderFinished(_))));

    let rendered = shell.render();
    assert!(rendered.contains(
        "Provider error from fake-provider: Provider provider error (404): model missing"
    ));
    assert!(rendered.contains("[Status]\nprovider error"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_progress_and_text_remain_separate_from_verified_action_truth() {
    let controller = Controller::new(ClaimingProvider);
    let root = smoke_root("provider-progress-boundary");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    let result = shell.submit_input(&controller, &mut session, "what happened?");

    assert_eq!(result.route, Route::AskModel);
    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().all(|event| {
        !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )
    }));

    let rendered = shell.render();
    assert!(rendered.contains(
        "Provider progress: working with claiming-provider (request claiming-request-1)."
    ));
    assert!(rendered.contains(
        "Provider progress: response ready from claiming-provider (request claiming-request-1). Provider text is suggestion only."
    ));
    assert!(rendered
        .contains("Assistant suggestion: I wrote hello.py and applied the action successfully."));
    assert!(!rendered.contains("Applied and verified"));
    assert!(!rendered.contains("file written:"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn displays_proposed_write_file_action_without_writing() {
    let controller = Controller::default();
    let root = smoke_root("proposed-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    let result = shell.submit_input(&controller, &mut session, "create file hello.py");

    assert_eq!(result.route, Route::ProposeWriteFile);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );

    let rendered = shell.render();
    assert!(rendered.contains("[Pending Action]\nAction: action-1 WriteFile"));
    assert!(rendered.contains("Target: hello.py"));
    assert!(rendered.contains("Summary: write hello.py"));
    assert!(rendered.contains("State: waiting for approval"));
    assert!(rendered.contains("No file has been changed yet. Press F5 to approve or F6 to reject."));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approves_write_file_through_controller_and_renders_verified_result() {
    let controller = Controller::default();
    let root = smoke_root("approve-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    shell.submit_input(&controller, &mut session, "create file hello.py");
    let result = shell.submit_approval(&controller, &mut session);

    assert_eq!(result.route, Route::ApproveAction);
    assert!(target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: target.display().to_string()
        })
    );

    let rendered = shell.render();
    assert!(rendered.contains("State: applied and verified"));
    assert!(rendered.contains(&format!("Result: file written: {}", target.display())));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_write_file_through_controller_without_writing() {
    let controller = Controller::default();
    let root = smoke_root("reject-write-file");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    shell.submit_input(&controller, &mut session, "create file hello.py");
    let result = shell.submit_rejection(&controller, &mut session);

    assert_eq!(result.route, Route::RejectAction);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let rendered = shell.render();
    assert!(rendered.contains("State: rejected"));
    assert!(rendered.contains("Result: Rejected. No file was changed."));
    assert!(rendered.contains("Rejected actions are final"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rendering_and_rejection_do_not_call_provider_or_mutate_files_directly() {
    let controller =
        Controller::new(ProviderStub::new("should-not-be-called").with_model("unused-model"));
    let root = smoke_root("surface-boundaries");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();
    let target = root.join("hello.py");

    let proposed = controller.turn(&mut session, "create file hello.py");
    shell.consume_events(&proposed.events);
    let before_render = session.clone();
    let rendered = shell.render();

    assert!(rendered.contains("State: waiting for approval"));
    assert_eq!(session, before_render);
    assert!(!target.exists());
    assert_eq!(session.provider_metadata(), None);
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ProviderStarted(_) | Event::ProviderFinished(_)
    )));

    let rejected = shell.submit_rejection(&controller, &mut session);

    assert_eq!(rejected.route, Route::RejectAction);
    assert!(!target.exists());
    assert_eq!(session.provider_metadata(), None);
    assert!(session.events().iter().all(|event| !matches!(
        event,
        Event::ProviderStarted(_) | Event::ProviderFinished(_)
    )));

    let _ = fs::remove_dir_all(root);
}
