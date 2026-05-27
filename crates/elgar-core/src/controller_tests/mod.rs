use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

use crate::{
    action::{
        Action, ActionLifecycleState, ActionRequest, CreateFileAction, FileActionVerification,
        OverwriteFileAction, PatchFileAction,
    },
    action_gate::ActionGate,
    event::{
        AssistantMessageSource, Event, ProviderMetrics, ProviderOutput, ProviderTokenUsage,
        VerifiedActionResult,
    },
    provider::{ControllerProvider, ProviderConfig, ProviderError, ProviderStub},
    router::Route,
    session::{ActionRecord, Session},
};

use super::Controller;

// These tests cover the explicit controller/review runtime. Normal live TUI
// turns are guarded separately through AgentRuntime.

fn session() -> Session {
    Session::new("session-1", ".", ".")
}

fn rooted_session(name: &str) -> (Session, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("elgar-controller-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    (Session::new("session-1", root.clone(), root.clone()), root)
}

static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    previous: Option<OsString>,
    _home_lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set_home(value: &std::path::Path) -> Self {
        let home_lock = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", value);
        Self {
            previous,
            _home_lock: home_lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("HOME", previous);
        } else {
            std::env::remove_var("HOME");
        }
    }
}

#[derive(Debug, Clone)]
struct FakeProvider {
    output: Result<ProviderOutput, ProviderError>,
}

impl FakeProvider {
    fn output(output: ProviderOutput) -> Self {
        Self { output: Ok(output) }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            output: Err(ProviderError::provider(message, Some(404), None)),
        }
    }
}

impl ControllerProvider for FakeProvider {
    fn request_metadata(&self) -> crate::provider::ProviderRequestMetadata {
        crate::provider::ProviderRequestMetadata::new(
            "fake-provider",
            Some("fake-model".to_string()),
            "fake-request-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        self.output.clone()
    }
}

#[derive(Debug, Clone)]
struct StreamingFakeProvider;

impl ControllerProvider for StreamingFakeProvider {
    fn request_metadata(&self) -> crate::provider::ProviderRequestMetadata {
        crate::provider::ProviderRequestMetadata::new(
            "stream-provider",
            Some("stream-model".to_string()),
            "stream-request-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("I approved and wrote hello.py."))
    }

    fn chat_stream(
        &self,
        _prompt: &str,
        on_chunk: &mut dyn FnMut(crate::provider::ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        on_chunk(crate::provider::ProviderStreamChunk::Reasoning(
            "Need to describe only.".to_string(),
        ));
        on_chunk(crate::provider::ProviderStreamChunk::Text(
            "I approved and wrote hello.py.".to_string(),
        ));
        Ok(ProviderOutput::new("I approved and wrote hello.py.")
            .with_thinking("Need to describe only."))
    }
}

mod action_lifecycle;
mod basic_turns;
mod provider_streaming_errors;
