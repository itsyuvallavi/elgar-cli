use serde_json::json;
use std::{
    collections::VecDeque,
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::{
    action::{
        Action, ActionLifecycleState, ActionRequest, FileActionVerification, ShellCommandAction,
    },
    controller_prompt::VERIFIED_MEMORY_BYTE_LIMIT,
    event::{
        AssistantMessageSource, Event, ProviderMetrics, ProviderOutput, ProviderTokenUsage,
        VerifiedActionResult,
    },
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    policy::{ApprovalSource, PermissionPolicyMode},
    provider::{
        ChatToolDefinition, ControllerProvider, ProviderConfig, ProviderError,
        ProviderRequestMetadata, ProviderStub,
    },
    renderer::render_session,
    router::Route,
    session::{
        ActionRecord, Session, StructuredProjectPlan, StructuredProjectPlanStatus,
        VerifiedFolderReference, VerifiedPlanReference,
    },
};

use super::Controller;

// These tests cover the explicit controller/review runtime. Normal live TUI
// turns are guarded separately and should use the Pi-style agent loop instead.

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

fn provider_assistant_messages(session: &Session) -> Vec<&str> {
    session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider =>
            {
                Some(message.content.as_str())
            }
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone)]
struct FakeProvider {
    output: Result<ProviderOutput, ProviderError>,
}

impl FakeProvider {
    fn success(text: impl Into<String>) -> Self {
        Self {
            output: Ok(ProviderOutput::new(text)),
        }
    }

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
struct CapturingProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

impl CapturingProvider {
    fn new(prompts: Arc<Mutex<Vec<String>>>) -> Self {
        Self { prompts }
    }
}

impl ControllerProvider for CapturingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "capture-provider",
            Some("capture-model".to_string()),
            "capture-request-1",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        Ok(ProviderOutput::new("captured"))
    }
}

#[derive(Debug, Clone)]
struct ToolEnabledFakeProvider {
    output: Result<ProviderOutput, ProviderError>,
    received_tool_names: Arc<Mutex<Vec<Vec<String>>>>,
    chat_call_count: Arc<Mutex<usize>>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl ToolEnabledFakeProvider {
    fn new(output: ProviderOutput) -> (Self, Arc<Mutex<Vec<Vec<String>>>>, Arc<Mutex<usize>>) {
        let (provider, received_tool_names, chat_call_count, _prompts) =
            Self::new_with_prompt_capture(output);
        (provider, received_tool_names, chat_call_count)
    }

    fn new_with_prompt_capture(
        output: ProviderOutput,
    ) -> (
        Self,
        Arc<Mutex<Vec<Vec<String>>>>,
        Arc<Mutex<usize>>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let received_tool_names = Arc::new(Mutex::new(Vec::new()));
        let chat_call_count = Arc::new(Mutex::new(0));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                output: Ok(output),
                received_tool_names: Arc::clone(&received_tool_names),
                chat_call_count: Arc::clone(&chat_call_count),
                prompts: Arc::clone(&prompts),
            },
            received_tool_names,
            chat_call_count,
            prompts,
        )
    }
}

impl ControllerProvider for ToolEnabledFakeProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "tool-provider",
            Some("tool-model".to_string()),
            "tool-request-1",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        *self.chat_call_count.lock().unwrap() += 1;
        Ok(ProviderOutput::new("legacy chat path"))
    }

    fn chat_with_tools_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.received_tool_names.lock().unwrap().push(
            tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect(),
        );
        self.output.clone()
    }
}

#[derive(Debug, Clone)]
struct ToolEnabledSequenceProvider {
    outputs: Arc<Mutex<VecDeque<ProviderOutput>>>,
    received_tool_names: Arc<Mutex<Vec<Vec<String>>>>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl ToolEnabledSequenceProvider {
    fn new(
        outputs: Vec<ProviderOutput>,
    ) -> (Self, Arc<Mutex<Vec<Vec<String>>>>, Arc<Mutex<Vec<String>>>) {
        let received_tool_names = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                outputs: Arc::new(Mutex::new(VecDeque::from(outputs))),
                received_tool_names: Arc::clone(&received_tool_names),
                prompts: Arc::clone(&prompts),
            },
            received_tool_names,
            prompts,
        )
    }
}

impl ControllerProvider for ToolEnabledSequenceProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "sequence-tool-provider",
            Some("tool-model".to_string()),
            "sequence-tool-request",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        Ok(ProviderOutput::new("legacy chat path"))
    }

    fn chat_with_tools_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.received_tool_names.lock().unwrap().push(
            tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect(),
        );
        self.outputs
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ProviderError::provider("no sequence output left", None, None))
    }
}

fn raw_model_tool_call(
    id: &str,
    name: RawModelToolName,
    arguments: serde_json::Value,
) -> RawModelToolCall {
    RawModelToolCall {
        id: id.to_string(),
        name,
        arguments,
        assistant_summary: None,
    }
}

fn seed_verified_folder(session: &mut Session, root: &std::path::Path, name: &str) -> PathBuf {
    let project_root = root.join(name);
    std::fs::create_dir_all(&project_root).unwrap();
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: project_root.clone(),
        source_action_id: format!("action-folder-{name}"),
    });
    project_root
}

fn seed_verified_react_ts_plan(
    session: &mut Session,
    root: &std::path::Path,
    name: &str,
) -> (PathBuf, PathBuf) {
    let project_root = seed_verified_folder(session, root, name);
    let plan_path = project_root.join("react-ts-project-plan.md");
    std::fs::write(
            &plan_path,
            format!(
                "# React TS Project Plan\n\nProject root: {}\n\n- Add a Vite-style React scaffold.\n- Defer package installation.\n",
                project_root.display()
            ),
        )
        .unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project_root.clone(),
        source_action_id: format!("action-plan-{name}"),
    });
    (project_root, plan_path)
}

fn seed_verified_react_ts_file_plan(
    session: &mut Session,
    root: &std::path::Path,
    name: &str,
) -> PathBuf {
    let project_root = seed_verified_folder(session, root, name);
    let plan_path = project_root.join("plan.md");
    std::fs::write(
            &plan_path,
            "# React TypeScript Project Plan\n\n- Create `package.json`.\n- Create `tsconfig.json`.\n- Create `vite.config.ts`.\n- Create `index.html`.\n- Create `src/main.tsx`.\n- Create `src/App.tsx`.\n- Defer package installation.\n",
        )
        .unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path,
        project_root: project_root.clone(),
        source_action_id: "action-plan-live".to_string(),
    });
    project_root
}

fn seed_verified_live_react_ts_plan(
    session: &mut Session,
    root: &std::path::Path,
    name: &str,
) -> PathBuf {
    let project_root = seed_verified_folder(session, root, name);
    let plan_path = project_root.join("plan.md");
    std::fs::write(
            &plan_path,
            "# React TypeScript Project Plan\n\n- **Directory structure**\n  - `src/` - source code (components, hooks, styles)\n  - `public/` - static assets and index.html\n  - `tsconfig.json` - TypeScript compiler options\n  - `package.json` - dependencies, scripts\n  - `vite.config.ts` - build configuration (optional)\n- **Key dependencies**\n  - `react`, `react-dom`\n  - `typescript`\n  - `vite` (or `create-react-app` with TS template)\n- **Scripts**\n  - `dev`: start dev server\n  - `build`: production build\n  - `lint`: run ESLint\n- **Styling** - use CSS modules or styled-components\n- **Testing** - Jest + React Testing Library (optional)\n- **Version control** - initialise git, add `.gitignore`\n\nThis plan outlines the essential files and structure for a minimal React + TS setup.\n",
        )
        .unwrap();
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path,
        project_root: project_root.clone(),
        source_action_id: "action-plan-live-shape".to_string(),
    });
    project_root
}

fn react_ts_missing_create_file_tool_calls() -> Vec<RawModelToolCall> {
    vec![
        raw_model_tool_call(
            "call-package",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "package.json", "contents": "{\"scripts\":{\"dev\":\"vite\"}}\n" }),
        ),
        raw_model_tool_call(
            "call-tsconfig",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "tsconfig.json", "contents": "{\"compilerOptions\":{}}\n" }),
        ),
        raw_model_tool_call(
            "call-vite",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "vite.config.ts", "contents": "export default {};\n" }),
        ),
        raw_model_tool_call(
            "call-index",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "index.html", "contents": "<div id=\"root\"></div>\n" }),
        ),
        raw_model_tool_call(
            "call-main",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "src/main.tsx", "contents": "import './App';\n" }),
        ),
        raw_model_tool_call(
            "call-app",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "src/App.tsx", "contents": "export function App() { return null; }\n" }),
        ),
    ]
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
mod model_first_policy;
mod model_first_project_plan;
mod provider_prompt_memory;
mod provider_streaming_errors;
