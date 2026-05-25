use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use elgar_core::{
    action::{ActionLifecycleState, ActionRequest, FileActionVerification},
    controller::Controller,
    event::{AssistantMessageSource, Event, ProviderOutput, VerifiedActionResult},
    provider::{ControllerProvider, ProviderError, ProviderRequestMetadata, ProviderStreamChunk},
    renderer::render_session,
    router::{route_input, Route},
    session::{Session, StructuredProjectPlanStatus, PROJECT_MEMORY_LIMIT},
};

fn regression_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "elgar-core-regression-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("regression-session", root, root)
}

static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
    _home_lock: Option<MutexGuard<'static, ()>>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let home_lock = (name == "HOME").then(|| {
            HOME_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        });
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self {
            name,
            previous,
            _home_lock: home_lock,
        }
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

fn isolated_home(root: &Path) -> (PathBuf, EnvGuard) {
    let home = root.join("home");
    fs::create_dir_all(home.join("Desktop")).unwrap();
    let home_guard = EnvGuard::set("HOME", &home);
    (home, home_guard)
}

fn event_count(session: &Session, matches: impl Fn(&Event) -> bool) -> usize {
    session
        .events()
        .iter()
        .filter(|event| matches(event))
        .count()
}

fn provider_event_count(session: &Session) -> usize {
    event_count(session, |event| {
        matches!(
            event,
            Event::ProviderStarted(_) | Event::ProviderFinished(_)
        )
    })
}

fn controller_messages(session: &Session) -> Vec<&str> {
    session
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Controller =>
            {
                Some(message.content.as_str())
            }
            _ => None,
        })
        .collect()
}

fn atomic_temp_files(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".elgar-atomic-"))
        })
        .collect()
}

fn assert_no_atomic_temp_files(root: &Path) {
    assert_eq!(atomic_temp_files(root), Vec::<PathBuf>::new());
}

fn provider_assistant_message_count(session: &Session) -> usize {
    event_count(session, |event| {
        matches!(
            event,
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider
        )
    })
}

fn action_truth_event_count(session: &Session) -> usize {
    event_count(session, |event| {
        matches!(
            event,
            Event::ActionProposed(_)
                | Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )
    })
}

fn assert_no_action_or_verified_truth(session: &Session) {
    assert!(session.actions().is_empty());
    assert_eq!(action_truth_event_count(session), 0);
}

fn assert_no_package_install_command(command: &str) {
    assert!(
        command.lines().all(|line| {
            let line = line.trim_start();
            !line.starts_with("npm install")
                && !line.starts_with("npm create")
                && !line.starts_with("pnpm install")
                && !line.starts_with("yarn install")
        }),
        "package install command should be deferred, got:\n{command}"
    );
}

fn write_broad_capability_fixture(root: &Path) {
    fs::write(root.join("edit-me.txt"), "original edit").unwrap();
    fs::write(root.join("overwrite-me.txt"), "original overwrite").unwrap();
    fs::write(root.join("delete-me.txt"), "original delete").unwrap();
    fs::write(root.join("move-me.txt"), "original move").unwrap();
    fs::write(root.join("rename-me.txt"), "original rename").unwrap();
}

fn assert_broad_capability_fixture_unchanged(root: &Path) {
    assert_eq!(
        fs::read_to_string(root.join("edit-me.txt")).unwrap(),
        "original edit"
    );
    assert_eq!(
        fs::read_to_string(root.join("overwrite-me.txt")).unwrap(),
        "original overwrite"
    );
    assert_eq!(
        fs::read_to_string(root.join("delete-me.txt")).unwrap(),
        "original delete"
    );
    assert_eq!(
        fs::read_to_string(root.join("move-me.txt")).unwrap(),
        "original move"
    );
    assert_eq!(
        fs::read_to_string(root.join("rename-me.txt")).unwrap(),
        "original rename"
    );
    assert!(!root.join("moved.txt").exists());
    assert!(!root.join("renamed.txt").exists());
    assert!(!root.join("created-dir").exists());
    assert!(!root.join("shell-ran.txt").exists());
}

#[derive(Debug, Clone)]
struct FilePlanProvider;

impl ControllerProvider for FilePlanProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "file-plan-provider",
            Some("model-a".to_string()),
            "file-plan-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new(
            "Plan: create file planned.py, approve it, and report that planned.py was written.",
        ))
    }
}

#[derive(Debug, Clone)]
struct FailingProvider {
    error: ProviderError,
}

impl FailingProvider {
    fn network_timeout() -> Self {
        Self {
            error: ProviderError::network("provider request timed out"),
        }
    }

    fn malformed_response() -> Self {
        Self {
            error: ProviderError::response_parse("malformed provider response"),
        }
    }
}

impl ControllerProvider for FailingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("failing-provider", Some("model-a".to_string()), "failure-1")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Err(self.error.clone())
    }
}

#[derive(Debug, Clone)]
struct IncompleteStreamProvider;

impl ControllerProvider for IncompleteStreamProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "incomplete-stream-provider",
            Some("model-a".to_string()),
            "stream-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("Approved and wrote streamed.py."))
    }

    fn chat_stream(
        &self,
        _prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        on_chunk(ProviderStreamChunk::Text(
            "Approved and wrote streamed.py.".to_string(),
        ));
        Err(ProviderError::response_parse(
            "chunked body ended before terminal chunk",
        ))
    }
}

#[derive(Debug, Clone)]
struct StreamingSuggestionProvider;

impl ControllerProvider for StreamingSuggestionProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "streaming-suggestion-provider",
            Some("model-a".to_string()),
            "suggestion-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(
            ProviderOutput::new("Approved and wrote hello.py. create file hidden.py")
                .with_thinking("I will approve the pending action."),
        )
    }

    fn chat_stream(
        &self,
        _prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        on_chunk(ProviderStreamChunk::Reasoning(
            "I will approve the pending action.".to_string(),
        ));
        on_chunk(ProviderStreamChunk::Text(
            "Approved and wrote hello.py. create file hidden.py".to_string(),
        ));
        Ok(
            ProviderOutput::new("Approved and wrote hello.py. create file hidden.py")
                .with_thinking("I will approve the pending action."),
        )
    }
}

#[derive(Debug, Clone)]
struct BroadCapabilityClaimProvider;

impl ControllerProvider for BroadCapabilityClaimProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "broad-capability-claim-provider",
            Some("model-a".to_string()),
            "broad-claim-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new(
            "I edited edit-me.txt, overwrote overwrite-me.txt, deleted delete-me.txt, \
             moved move-me.txt to moved.txt, renamed rename-me.txt to renamed.txt, \
             created created-dir, and ran bash to touch shell-ran.txt.",
        ))
    }
}

#[derive(Debug, Clone)]
struct FolderPlanProvider;

impl ControllerProvider for FolderPlanProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "folder-plan-provider",
            Some("model-a".to_string()),
            "folder-plan-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new(
            "Sure! Let me suggest a small folder structure.\n\
             code:\n\
                 project/\n\
                 ├─ src/          # source files\n\
                 ├─ tests/        # tests\n\
                 ├─ docs/         # docs\n\
                 └─ data/         # data\n\
             Once you approve, I can generate the shell commands.",
        ))
    }
}

#[derive(Debug, Clone)]
struct MarkdownPlanProvider;

impl ControllerProvider for MarkdownPlanProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "markdown-plan-provider",
            Some("model-a".to_string()),
            "markdown-plan-1",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        assert!(prompt.contains("Return only Markdown content"));
        assert!(prompt.contains("calculator"));
        Ok(ProviderOutput::new(
            "# Calculator UI Plan\n\n- Define requirements.\n- Build a small Tkinter UI.\n",
        ))
    }
}

#[derive(Debug, Clone)]
struct SimpleMarkdownPlanProvider;

impl ControllerProvider for SimpleMarkdownPlanProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "simple-markdown-plan-provider",
            Some("model-a".to_string()),
            "simple-markdown-plan-1",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        assert!(prompt.contains("Return only Markdown content"));
        Ok(ProviderOutput::new(
            "# Simple Project Plan\n\n- Create the project folder.\n- Add README.md.\n",
        ))
    }
}

#[derive(Debug, Clone)]
struct UnsafeMarkdownPlanProvider;

impl ControllerProvider for UnsafeMarkdownPlanProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "unsafe-markdown-plan-provider",
            Some("model-a".to_string()),
            "unsafe-markdown-plan-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new(
            "Approved and wrote hidden.md.\n\n# Hidden Plan\n",
        ))
    }
}

#[test]
fn ask_model_for_file_plan_is_provider_suggestion_only_and_no_network() {
    let controller = Controller::new(FilePlanProvider);
    let root = regression_root("file-plan-suggestion-only");
    let mut session = session_at(&root);
    let target = root.join("planned.py");

    let result = controller.turn(
        &mut session,
        "what file plan would create planned.py, then approve it?",
    );

    assert_eq!(result.route, Route::AskModel);
    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert_eq!(action_truth_event_count(&session), 0);
    assert_eq!(provider_event_count(&session), 2);
    assert_eq!(provider_assistant_message_count(&session), 1);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderFinished(finished)
            if finished.provider == "file-plan-provider"
                && finished.request_id == "file-plan-1"
                && finished.output.text.contains("planned.py was written")
    )));
    assert_eq!(
        session
            .provider_metadata()
            .map(|metadata| metadata.provider.as_str()),
        Some("file-plan-provider")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn markdown_plan_request_proposes_create_file_with_provider_content_before_approval() {
    let controller = Controller::new(MarkdownPlanProvider);
    let root = regression_root("markdown-plan-proposal");
    let mut session = session_at(&root);
    let target = root.join("calculator-ui-python-plan.md");

    let result = controller.turn(
        &mut session,
        "create an md file with a plan to create a calculator UI using python",
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert!(!target.exists());
    assert_eq!(provider_event_count(&session), 2);
    assert_eq!(provider_assistant_message_count(&session), 1);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );

    let create_file = match &session.actions()[0].action.request {
        ActionRequest::CreateFile(create_file) => create_file,
        other => panic!("expected CreateFile action, got {other:?}"),
    };
    assert_eq!(
        create_file.target_path,
        PathBuf::from("calculator-ui-python-plan.md")
    );
    assert!(create_file.contents.contains("# Calculator UI Plan"));

    let preview = session.actions()[0].action.approval_summary().preview;
    assert!(format!("{preview:?}").contains("# Calculator UI Plan"));

    controller.turn(&mut session, "approve");

    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "# Calculator UI Plan\n\n- Define requirements.\n- Build a small Tkinter UI.\n"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_desktop_markdown_plan_request_creates_pending_shell_write_action() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("desktop-markdown-plan-proposal");
    let (home, _home) = isolated_home(&root);
    let mut session = session_at(&root);

    let result = controller.turn(
        &mut session,
        "please create a plan for a simple project on my desktop",
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(provider_event_count(&session), 2);
    assert_eq!(provider_assistant_message_count(&session), 1);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );

    let expected_file = home.join("Desktop").join("simple-project-plan.md");
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.contains("cat > "));
            assert!(action.command.contains("simple-project-plan.md"));
            assert_eq!(action.cwd, root);
            assert_eq!(action.expected_file.as_ref(), Some(&expected_file));
            assert!(action.expected_effect.contains("Write Markdown plan"));
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_desktop_react_ts_project_request_proposes_controller_owned_plan_not_provider_chat() {
    let controller = Controller::default();
    let root = regression_root("desktop-react-ts-project-plan-proposal");
    let (home, _home) = isolated_home(&root);
    let mut session = session_at(&root);

    let result = controller.turn(
        &mut session,
        "can you create a project on the desktop inside a folder you need to create called Demo? the project should be a simple react TS project.",
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(provider_event_count(&session), 0);
    assert_eq!(provider_assistant_message_count(&session), 0);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );

    let expected_project_root = home.join("Desktop").join("Demo");
    let expected_plan = expected_project_root.join("react-ts-project-plan.md");
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert_eq!(
                action.expected_directory.as_ref(),
                Some(&expected_project_root)
            );
            assert_eq!(action.expected_file.as_ref(), Some(&expected_plan));
            assert!(action.command.contains("cat > "));
            assert!(action.command.contains("React TS Project Plan"));
            assert_no_package_install_command(action.command.as_str());
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approved_react_ts_project_plan_then_execute_scaffolds_files_without_installing_packages() {
    let controller = Controller::default();
    let root = regression_root("react-ts-project-plan-execute");
    let project_root = root.join("Desktop").join("Demo");
    let plan_path = project_root.join("react-ts-project-plan.md");
    let mut session = session_at(&root);

    let plan_result = controller.turn(
        &mut session,
        &format!(
            "can you create a simple React TS project at {}?",
            project_root.display()
        ),
    );

    assert_eq!(plan_result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(provider_event_count(&session), 0);
    assert_eq!(session.actions().len(), 1);
    controller.turn(&mut session, "approve");

    assert!(project_root.is_dir());
    assert!(plan_path.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(project_root.as_path())
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );

    let provider_events_before_execute = provider_event_count(&session);
    let execute_result = controller.turn(&mut session, "execute the plan");

    assert_eq!(execute_result.route, Route::ExecutePlan);
    assert_eq!(
        provider_event_count(&session),
        provider_events_before_execute
    );
    assert_eq!(session.actions().len(), 2);
    let structured_plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("execute plan should record a structured project plan");
    assert_eq!(structured_plan.project_root, project_root);
    assert_eq!(structured_plan.stage, "scaffold");
    assert_eq!(
        structured_plan.status,
        StructuredProjectPlanStatus::Proposed
    );
    assert_eq!(
        structured_plan.expected_directories,
        vec![project_root.join("src")]
    );
    assert!(structured_plan
        .expected_files
        .contains(&project_root.join("package.json")));
    assert!(structured_plan
        .expected_files
        .contains(&project_root.join("src").join("App.tsx")));

    match &session.actions()[1].action.request {
        ActionRequest::ShellCommand(action) => {
            assert_eq!(action.expected_directories, vec![project_root.join("src")]);
            assert!(action
                .expected_files
                .contains(&project_root.join("package.json")));
            assert!(action
                .expected_files
                .contains(&project_root.join("src").join("main.tsx")));
            assert_no_package_install_command(action.command.as_str());
        }
        other => panic!("expected executable ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(project_root.join("package.json").is_file());
    assert!(project_root.join("src").join("App.tsx").is_file());
    assert!(project_root.join("src").join("main.tsx").is_file());
    assert!(fs::read_to_string(project_root.join("README.md"))
        .unwrap()
        .contains("Package installation is deferred"));
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn absolute_markdown_plan_request_writes_only_after_approval_and_verifies_file() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("absolute-markdown-plan-shell");
    let target = root.join("Desktop").join("PROJECT_PLAN.md");
    let mut session = session_at(&root);

    let result = controller.turn(
        &mut session,
        &format!("please create a plan at {}", target.display()),
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.contains("mkdir -p "));
            assert!(action.command.contains("cat > "));
            assert_eq!(action.expected_file.as_ref(), Some(&target));
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# Simple Project Plan\n\n- Create the project folder.\n- Add README.md.\n"
    );
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.exit_code, Some(0));
            let expected_effect = format!("verified file exists: {}", target.display());
            assert_eq!(
                shell.verified_effect.as_deref(),
                Some(expected_effect.as_str())
            );
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_plan_followup_without_pending_action_does_not_go_to_provider() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("execute-plan-no-pending");
    let mut session = session_at(&root);

    let result = controller.turn(&mut session, "okay execute the plan");

    assert_eq!(result.route, Route::ExecutePlan);
    assert_eq!(provider_event_count(&session), 0);
    assert!(session.actions().is_empty());
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No controller-owned executable plan is waiting")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn markdown_plan_inside_verified_folder_then_execute_proposes_batch_files() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("markdown-plan-inside-verified-folder");
    let target_folder = root.join("Desktop").join("NoteKeeper");
    let mut session = session_at(&root);

    controller.turn(
        &mut session,
        &format!("create a folder at {}", target_folder.display()),
    );
    controller.turn(&mut session, "approve");

    assert!(target_folder.is_dir());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(target_folder.as_path())
    );

    let plan_result = controller.turn(
        &mut session,
        "create a plan for a small python project inside that folder",
    );

    assert_eq!(plan_result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(session.actions().len(), 2);
    let plan_path = target_folder.join("small-python-project-plan.md");
    match &session.actions()[1].action.request {
        ActionRequest::ShellCommand(action) => {
            assert_eq!(action.expected_file.as_ref(), Some(&plan_path));
            assert!(action.command.contains("small-python-project-plan.md"));
        }
        other => panic!("expected Markdown ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(plan_path.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.project_root.as_path()),
        Some(target_folder.as_path())
    );
    assert_eq!(
        fs::read_to_string(&plan_path).unwrap(),
        "# Simple Project Plan\n\n- Create the project folder.\n- Add README.md.\n"
    );

    let provider_events_before_execute = provider_event_count(&session);
    let execute_result = controller.turn(&mut session, "okay execute the plan inside that folder");

    assert_eq!(execute_result.route, Route::ExecutePlan);
    assert_eq!(
        provider_event_count(&session),
        provider_events_before_execute
    );
    let structured_plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("execute plan should record a structured project plan");
    assert_eq!(
        structured_plan.source_action_id.as_deref(),
        Some("action-3")
    );
    assert_eq!(structured_plan.source_plan_path, plan_path);
    assert_eq!(structured_plan.project_root, target_folder);
    assert_eq!(structured_plan.stage, "scaffold");
    assert_eq!(
        structured_plan.status,
        StructuredProjectPlanStatus::Proposed
    );
    assert_eq!(
        structured_plan.expected_directories,
        vec![
            structured_plan.project_root.join("src"),
            structured_plan.project_root.join("tests")
        ]
    );
    assert!(structured_plan
        .expected_files
        .contains(&structured_plan.project_root.join("README.md")));
    assert_eq!(session.actions().len(), 3);
    match &session.actions()[2].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.contains("src/csv_filter.py"));
            assert!(action.command.contains("tests/test_csv_filter.py"));
            assert_eq!(
                action.expected_directories,
                vec![target_folder.join("src"), target_folder.join("tests")]
            );
            assert_eq!(
                action.expected_files,
                vec![
                    target_folder.join("src").join("__init__.py"),
                    target_folder.join("src").join("csv_filter.py"),
                    target_folder.join("tests").join("test_csv_filter.py"),
                    target_folder.join("README.md"),
                    target_folder.join("pyproject.toml"),
                ]
            );
        }
        other => panic!("expected executable ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(target_folder.join("src").join("csv_filter.py").is_file());
    assert!(target_folder
        .join("tests")
        .join("test_csv_filter.py")
        .is_file());
    assert!(target_folder.join("README.md").is_file());
    assert_eq!(
        session.actions()[2].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session
            .project_memory()
            .latest_executed_structured_plan()
            .map(|plan| plan.source_action_id.as_deref()),
        Some(Some("action-3"))
    );
    match session.actions()[2].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.exit_code, Some(0));
            assert!(shell
                .verified_effect
                .as_deref()
                .is_some_and(|effect| effect.contains("verified files exist")));
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn create_plan_inside_folder_you_created_uses_latest_verified_folder_without_provider() {
    let controller = Controller::default();
    let root = regression_root("plan-inside-folder-you-created");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    fs::create_dir_all(&desktop).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("HOME", &home);
    let mut session = session_at(&project);
    let verified_folder = desktop.join("helloworld");

    controller.turn(
        &mut session,
        "create a folder in desktop and call it helloworld",
    );
    controller.turn(&mut session, "approve");

    assert!(verified_folder.is_dir());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(verified_folder.as_path())
    );

    let provider_events_before = provider_event_count(&session);
    let result = controller.turn(
        &mut session,
        "create a plan for a basic react project inside the folder you created.",
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(
        provider_event_count(&session),
        provider_events_before,
        "controller-owned plan creation must not fall through to provider prose"
    );
    assert_eq!(session.actions().len(), 2);
    match &session.actions()[1].action.request {
        ActionRequest::ShellCommand(action) => {
            let expected_plan = verified_folder.join("react-project-plan.md");
            assert_eq!(action.expected_directory.as_ref(), Some(&verified_folder));
            assert_eq!(action.expected_file.as_ref(), Some(&expected_plan));
            assert!(action.command.contains("React Project Plan"));
            assert!(
                !action
                    .command
                    .contains(&project.join("project-plan.md").display().to_string()),
                "plan target must not be repo-local project/project-plan.md: {}",
                action.command
            );
        }
        other => panic!("expected Markdown ShellCommand action, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_project_according_to_verified_plan_uses_verified_plan_folder() {
    let controller = Controller::default();
    let root = regression_root("execute-according-to-verified-plan-folder");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    fs::create_dir_all(&desktop).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("HOME", &home);
    let mut session = session_at(&project);
    let verified_folder = desktop.join("helloworld");
    let expected_plan = verified_folder.join("react-project-plan.md");

    controller.turn(
        &mut session,
        "create a folder in desktop and call it helloworld",
    );
    controller.turn(&mut session, "approve");
    controller.turn(
        &mut session,
        "create a plan for a basic react project inside the folder you created.",
    );
    controller.turn(&mut session, "approve");

    assert!(expected_plan.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(expected_plan.as_path())
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.project_root.as_path()),
        Some(verified_folder.as_path())
    );

    let provider_events_before = provider_event_count(&session);
    let result = controller.turn(&mut session, "create the project according to the plan!");

    assert_eq!(result.route, Route::ExecutePlan);
    assert_eq!(provider_event_count(&session), provider_events_before);
    assert_eq!(session.actions().len(), 3);

    let structured_plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("execute plan should record a structured project plan");
    assert_eq!(structured_plan.source_plan_path, expected_plan);
    assert_eq!(structured_plan.project_root, verified_folder);
    assert_eq!(
        structured_plan.status,
        StructuredProjectPlanStatus::Proposed
    );
    assert!(structured_plan
        .expected_files
        .iter()
        .all(|path| path.starts_with(&verified_folder)));
    assert!(structured_plan
        .expected_directories
        .iter()
        .all(|path| path.starts_with(&verified_folder)));

    match &session.actions()[2].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(
                action
                    .expected_files
                    .iter()
                    .all(|path| path.starts_with(&verified_folder)),
                "expected files must stay under verified folder: {:?}",
                action.expected_files
            );
            assert!(
                !action
                    .expected_files
                    .iter()
                    .any(|path| path.starts_with(project.join("project"))),
                "execute plan must not scaffold repo-local project/: {:?}",
                action.expected_files
            );
        }
        other => panic!("expected executable ShellCommand action, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prompt_marker_folder_plan_then_create_project_stays_in_latest_verified_desktop_folder() {
    let controller = Controller::default();
    let root = regression_root("prompt-marker-desktop-project-flow");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    fs::create_dir_all(&desktop).unwrap();
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).unwrap();
    let _home = EnvGuard::set("HOME", &home);
    let mut session = session_at(&repo_root);
    let verified_folder = desktop.join("promptmarker");
    let expected_plan = verified_folder.join("react-project-plan.md");
    let repo_local_project = repo_root.join("project");

    let folder_result = controller.turn(
        &mut session,
        "> create a folder in desktop and call it promptmarker",
    );
    assert_eq!(folder_result.route, Route::ProposeCreateDirectory);
    assert_eq!(provider_event_count(&session), 0);
    controller.turn(&mut session, "approve");

    assert!(verified_folder.is_dir());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(verified_folder.as_path())
    );

    let provider_events_before_plan = provider_event_count(&session);
    let plan_result = controller.turn(
        &mut session,
        "> > create a plan for a basic react project inside the folder you created.",
    );
    assert_eq!(plan_result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(provider_event_count(&session), provider_events_before_plan);
    controller.turn(&mut session, "approve");

    assert!(expected_plan.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(expected_plan.as_path())
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.project_root.as_path()),
        Some(verified_folder.as_path())
    );

    let provider_events_before_create = provider_event_count(&session);
    let create_result = controller.turn(&mut session, "create the project");

    assert_eq!(create_result.route, Route::ExecutePlan);
    assert_eq!(
        provider_event_count(&session),
        provider_events_before_create
    );
    assert_eq!(session.actions().len(), 3);
    match &session.actions()[2].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(
                action
                    .expected_files
                    .iter()
                    .all(|path| path.starts_with(&verified_folder)),
                "expected files must stay under verified folder: {:?}",
                action.expected_files
            );
            assert_no_package_install_command(action.command.as_str());
        }
        other => panic!("expected executable ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(verified_folder.join("package.json").is_file());
    assert!(verified_folder.join("src").join("App.tsx").is_file());
    assert!(verified_folder.join("src").join("main.tsx").is_file());
    assert!(!repo_local_project.exists());
    assert!(!repo_root.join("project-plan.md").exists());
    assert_eq!(
        session.actions()[2].action.state,
        ActionLifecycleState::Applied
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn create_the_project_without_verified_plan_proposes_plan_inside_latest_verified_folder() {
    let controller = Controller::default();
    let root = regression_root("create-project-missing-plan-latest-folder");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    fs::create_dir_all(&desktop).unwrap();
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).unwrap();
    let _home = EnvGuard::set("HOME", &home);
    let mut session = session_at(&repo_root);
    let verified_folder = desktop.join("missingplan");
    let expected_plan = verified_folder.join("project-plan.md");

    controller.turn(
        &mut session,
        "> create a folder in desktop and call it missingplan",
    );
    controller.turn(&mut session, "approve");

    assert!(verified_folder.is_dir());
    assert!(session.project_memory().latest_verified_plan().is_none());

    let provider_events_before = provider_event_count(&session);
    let result = controller.turn(&mut session, "create the project");

    assert_eq!(result.route, Route::ExecutePlan);
    assert_eq!(provider_event_count(&session), provider_events_before);
    assert_eq!(session.actions().len(), 2);
    match &session.actions()[1].action.request {
        ActionRequest::ShellCommand(action) => {
            assert_eq!(action.expected_directory.as_ref(), Some(&verified_folder));
            assert_eq!(action.expected_file.as_ref(), Some(&expected_plan));
            assert!(
                !action
                    .command
                    .contains(&repo_root.join("project").display().to_string()),
                "missing plan proposal must not target repo-local project/: {}",
                action.command
            );
        }
        other => panic!("expected Markdown ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(expected_plan.is_file());
    assert!(!repo_root.join("project").exists());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(expected_plan.as_path())
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_memory_is_bounded_and_dedupes_verified_folder_references() {
    let controller = Controller::default();
    let root = regression_root("project-memory-bounded-folders");
    let mut session = session_at(&root);

    for index in 0..(PROJECT_MEMORY_LIMIT + 3) {
        let folder = root.join("Desktop").join(format!("memory-{index}"));
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
    }

    assert_eq!(
        session.project_memory().verified_folders.len(),
        PROJECT_MEMORY_LIMIT
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(root.join("Desktop").join("memory-10").as_path())
    );

    let duplicate = root.join("Desktop").join("memory-10");
    controller.turn(
        &mut session,
        &format!("create a folder at {}", duplicate.display()),
    );
    controller.turn(&mut session, "approve");

    assert_eq!(
        session.project_memory().verified_folders.len(),
        PROJECT_MEMORY_LIMIT
    );
    assert_eq!(
        session
            .project_memory()
            .verified_folders
            .iter()
            .filter(|reference| reference.path == duplicate)
            .count(),
        1
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_execute_plan_proposal_removes_proposed_structured_memory() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("project-memory-rejected-structured-plan");
    let target_folder = root.join("Desktop").join("RejectPlan");
    let mut session = session_at(&root);

    controller.turn(
        &mut session,
        &format!("create a folder at {}", target_folder.display()),
    );
    controller.turn(&mut session, "approve");
    controller.turn(
        &mut session,
        "create a plan for a small python project inside that folder",
    );
    controller.turn(&mut session, "approve");

    controller.turn(&mut session, "execute the plan inside that folder");
    assert_eq!(session.project_memory().structured_plans.len(), 1);
    assert_eq!(
        session.project_memory().structured_plans[0].status,
        StructuredProjectPlanStatus::Proposed
    );

    controller.turn(&mut session, "reject");

    assert!(session.project_memory().structured_plans.is_empty());
    assert!(!target_folder.join("src").exists());
    assert_eq!(
        session.actions()[2].action.state,
        ActionLifecycleState::Rejected
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_plan_reports_missing_latest_verified_plan_without_shell_proposal() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("project-memory-stale-plan-no-fallback");
    let folder_a = root.join("Desktop").join("PlanA");
    let folder_b = root.join("Desktop").join("PlanB");
    let mut session = session_at(&root);

    for folder in [&folder_a, &folder_b] {
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
        controller.turn(
            &mut session,
            "create a plan for a small python project inside that folder",
        );
        controller.turn(&mut session, "approve");
    }

    let plan_a = folder_a.join("small-python-project-plan.md");
    let plan_b = folder_b.join("small-python-project-plan.md");
    assert!(plan_a.is_file());
    assert!(plan_b.is_file());
    fs::remove_file(&plan_b).unwrap();

    let actions_before = session.actions().len();
    let truth_events_before = action_truth_event_count(&session);
    let result = controller.turn(&mut session, "execute the plan");

    assert_eq!(result.route, Route::ExecutePlan);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(action_truth_event_count(&session), truth_events_before);
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_b.as_path())
    );
    assert!(!folder_a.join("src").exists());
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified Markdown plan could not be read")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn execute_plan_that_folder_reports_stale_latest_folder_without_plan_parent_fallback() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("execute-plan-stale-that-folder");
    let folder_a = root.join("Desktop").join("PlanFolder");
    let folder_b = root.join("Desktop").join("StaleFolder");
    let mut session = session_at(&root);

    controller.turn(
        &mut session,
        &format!("create a folder at {}", folder_a.display()),
    );
    controller.turn(&mut session, "approve");
    controller.turn(
        &mut session,
        "create a plan for a small python project inside that folder",
    );
    controller.turn(&mut session, "approve");
    let plan_path = folder_a.join("small-python-project-plan.md");
    assert!(plan_path.is_file());

    controller.turn(
        &mut session,
        &format!("create a folder at {}", folder_b.display()),
    );
    controller.turn(&mut session, "approve");
    assert!(folder_b.is_dir());
    fs::remove_dir_all(&folder_b).unwrap();

    let actions_before = session.actions().len();
    let truth_events_before = action_truth_event_count(&session);
    let result = controller.turn(&mut session, "execute the plan inside that folder");

    assert_eq!(result.route, Route::ExecutePlan);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(action_truth_event_count(&session), truth_events_before);
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );
    assert_eq!(
        session
            .project_memory()
            .latest_verified_folder()
            .map(|reference| reference.path.as_path()),
        Some(folder_b.as_path())
    );
    assert!(!folder_a.join("src").exists());
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified folder is missing")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn old_serialized_session_without_project_memory_deserializes_with_empty_memory() {
    let root = regression_root("old-session-without-project-memory");
    let root_string = root.display().to_string();

    let session: Session = serde_json::from_value(serde_json::json!({
        "id": "old-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [],
        "provider_metadata": null
    }))
    .unwrap();

    assert!(session.project_memory().verified_folders.is_empty());
    assert!(session.project_memory().verified_plans.is_empty());
    assert!(session.project_memory().structured_plans.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_markdown_file_write_cannot_bypass_approval_or_rejected_terminal_state() {
    let controller = Controller::new(UnsafeMarkdownPlanProvider);
    let root = regression_root("markdown-plan-approval-boundary");
    let mut session = session_at(&root);
    let target = root.join("hidden-plan.md");

    let result = controller.turn(
        &mut session,
        "please write hidden-plan.md with a markdown plan for hidden work",
    );

    assert_eq!(result.route, Route::ProposeWriteFile);
    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    match &session.actions()[0].action.request {
        ActionRequest::CreateFile(action) => {
            assert_eq!(action.target_path, PathBuf::from("hidden-plan.md"));
        }
        other => panic!("expected explicit CreateFile action, got {other:?}"),
    }
    assert_eq!(action_truth_event_count(&session), 1);
    assert_eq!(provider_event_count(&session), 0);

    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");

    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_response_is_suggestion_only_and_cannot_mutate_controller_truth() {
    let controller = Controller::default();
    let root = regression_root("provider-suggestion");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionProposed(_)
                | Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )),
        0
    );

    controller.turn(&mut session, "create hello.py");
    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApproved(_))),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        0
    );
    assert!(session.project_memory().verified_folders.is_empty());
    assert!(session.project_memory().verified_plans.is_empty());
    assert!(session.project_memory().structured_plans.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unsupported_broad_capability_requests_are_unknown_and_do_not_mutate_truth_or_files() {
    let controller = Controller::default();
    let root = regression_root("unsupported-broad-capabilities");
    let mut session = session_at(&root);
    write_broad_capability_fixture(&root);
    let shell_target = root.join("shell-ran.txt");

    for input in [
        "edit edit-me.txt",
        "overwrite overwrite-me.txt",
        "delete",
        "move move-me.txt moved.txt",
        "rename rename-me.txt renamed.txt",
        "mkdir",
        "create directory",
        &format!("shell touch {}", shell_target.display()),
        &format!("bash -lc 'touch {}'", shell_target.display()),
    ] {
        assert_eq!(controller.turn(&mut session, input).route, Route::Unknown);
    }

    assert_broad_capability_fixture_unchanged(&root);
    assert_no_action_or_verified_truth(&session);
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_text_cannot_self_approve_broad_capabilities_or_execute_shell() {
    let controller = Controller::new(BroadCapabilityClaimProvider);
    let root = regression_root("provider-broad-capability-claims");
    let mut session = session_at(&root);
    write_broad_capability_fixture(&root);
    let shell_target = root.join("shell-ran.txt");

    let result = controller.turn(
        &mut session,
        &format!(
            "can you edit edit-me.txt, overwrite overwrite-me.txt, delete delete-me.txt, \
             move move-me.txt to moved.txt, rename rename-me.txt to renamed.txt, \
             mkdir created-dir, and run bash -lc 'touch {}'?",
            shell_target.display()
        ),
    );

    assert_eq!(result.route, Route::AskModel);
    assert_broad_capability_fixture_unchanged(&root);
    assert_no_action_or_verified_truth(&session);
    assert_eq!(provider_event_count(&session), 2);
    assert_eq!(provider_assistant_message_count(&session), 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_timeout_records_error_without_assistant_success_or_mutation() {
    let controller = Controller::new(FailingProvider::network_timeout());
    let root = regression_root("provider-timeout");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    controller.turn(&mut session, "what does the harness do?");

    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(_))));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::Error(error) if error.message.contains("provider request timed out")
    )));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ProviderFinished(_)
        )),
        0
    );
    assert_eq!(provider_assistant_message_count(&session), 0);
    assert_eq!(action_truth_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_provider_response_records_error_without_mutating_pending_action_or_file() {
    let controller = Controller::new(FailingProvider::malformed_response());
    let root = regression_root("malformed-provider-response");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    Controller::default().turn(&mut session, "create file hello.py");
    controller.turn(&mut session, "what if you approve and write hello.py?");

    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::Error(error) if error.message.contains("malformed provider response")
    )));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ProviderFinished(_)
        )),
        0
    );
    assert_eq!(provider_assistant_message_count(&session), 0);
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn incomplete_stream_exposes_live_chunk_but_commits_no_partial_assistant_output() {
    let controller = Controller::new(IncompleteStreamProvider);
    let root = regression_root("incomplete-stream");
    let mut session = session_at(&root);
    let target = root.join("streamed.py");
    let mut chunks = Vec::new();

    Controller::default().turn(&mut session, "create file streamed.py");
    let result =
        controller.model_turn_streaming(&mut session, "what does the harness do?", &mut |chunk| {
            chunks.push(chunk)
        });

    assert_eq!(
        chunks,
        vec![ProviderStreamChunk::Text(
            "Approved and wrote streamed.py.".to_string()
        )]
    );
    assert!(!target.exists());
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::Error(_))));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ProviderFinished(_)
        )),
        0
    );
    assert_eq!(provider_assistant_message_count(&session), 0);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );
    assert_eq!(
        session
            .provider_metadata()
            .map(|metadata| metadata.provider.as_str()),
        Some("incomplete-stream-provider")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_streaming_reasoning_and_text_cannot_create_approve_or_write_actions() {
    let controller = Controller::new(StreamingSuggestionProvider);
    let root = regression_root("streaming-suggestion-only");
    let mut session = session_at(&root);
    let target = root.join("hello.py");
    let hidden = root.join("hidden.py");
    let mut chunks = Vec::new();

    Controller::default().turn(&mut session, "create file hello.py");
    controller.model_turn_streaming(
        &mut session,
        "what if you approve and write hello.py?",
        &mut |chunk| chunks.push(chunk),
    );

    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("I will approve the pending action.".to_string()),
            ProviderStreamChunk::Text(
                "Approved and wrote hello.py. create file hidden.py".to_string()
            )
        ]
    );
    assert!(!target.exists());
    assert!(!hidden.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_file_dogfood_reject_then_propose_again_and_approve() {
    let controller = Controller::default();
    let root = regression_root("dogfood-reject-then-approve");
    let mut session = session_at(&root);
    let rejected_target = root.join("dogfood-rejected.py");
    let approved_target = root.join("dogfood-approved.py");

    controller.turn(&mut session, "create file dogfood-rejected.py");
    controller.turn(&mut session, "reject");

    assert!(!rejected_target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionRejected(_))),
        1
    );

    controller.turn(&mut session, "create file dogfood-approved.py");
    controller.turn(&mut session, "approve");

    assert!(!rejected_target.exists());
    assert!(approved_target.exists());
    assert_eq!(fs::read_to_string(&approved_target).unwrap(), "");
    assert_eq!(session.actions().len(), 2);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[1].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: approved_target.display().to_string()
        })
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        2
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        1
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn router_classifies_create_file_and_unknown_input_is_safe() {
    let controller = Controller::default();
    let root = regression_root("router-unknown");
    let mut session = session_at(&root);
    let target = root.join("hello.py");

    assert_eq!(route_input("create file hello.py"), Route::ProposeWriteFile);
    assert_eq!(route_input("delete file old.py"), Route::ProposeDeleteFile);
    assert_eq!(
        route_input("rename file old.py to new.py"),
        Route::ProposeMoveFile
    );
    assert_eq!(
        route_input("create directory src/generated"),
        Route::ProposeCreateDirectory
    );
    assert_eq!(route_input("run ls"), Route::ProposeShellCommand);
    assert_eq!(controller.turn(&mut session, "   ").route, Route::Unknown);
    assert_eq!(
        controller.turn(&mut session, "bash -lc ls").route,
        Route::Unknown
    );

    assert!(!target.exists());
    assert!(session.actions().is_empty());
    assert_eq!(session.provider_metadata(), None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ProviderStarted(_))),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ProviderFinished(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_like_file_names_route_to_create_file_actions_not_project_plans() {
    for file_name in ["approved.py", "applied.py", "app.py"] {
        let controller = Controller::default();
        let root = regression_root(&format!("app-like-file-name-{file_name}"));
        let mut session = session_at(&root);

        let result = controller.turn(&mut session, &format!("create file {file_name}"));

        assert_eq!(result.route, Route::ProposeWriteFile);
        assert_eq!(provider_event_count(&session), 0);
        assert_eq!(session.actions().len(), 1);
        match &session.actions()[0].action.request {
            ActionRequest::CreateFile(action) => {
                assert_eq!(action.target_path, PathBuf::from(file_name));
            }
            other => panic!("expected CreateFile action for {file_name}, got {other:?}"),
        }

        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn shell_command_actions_can_be_proposed_without_execution() {
    let controller = Controller::default();
    let root = regression_root("shell-command-proposed");
    let mut session = session_at(&root);
    let marker = root.join("shell-ran.txt");

    let result = controller.turn(&mut session, &format!("run touch {}", marker.display()));

    assert_eq!(result.route, Route::ProposeShellCommand);
    assert!(!marker.exists());
    assert_eq!(provider_event_count(&session), 0);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert_eq!(action.command, format!("touch {}", marker.display()));
            assert_eq!(action.cwd, root);
            assert_eq!(action.timeout_seconds, 30);
            assert_eq!(action.output_caps.stdout_bytes, 16 * 1024);
            assert_eq!(action.output_caps.stderr_bytes, 16 * 1024);
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_shell_command_action_is_terminal_and_does_not_execute() {
    let controller = Controller::default();
    let root = regression_root("shell-command-rejected");
    let mut session = session_at(&root);
    let marker = root.join("shell-ran.txt");

    controller.turn(
        &mut session,
        &format!("shell command touch {}", marker.display()),
    );
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");

    assert!(!marker.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approved_shell_command_runs_once_and_records_shell_result() {
    let controller = Controller::default();
    let root = regression_root("shell-command-approved");
    let mut session = session_at(&root);
    let marker = root.join("shell-ran.txt");

    controller.turn(
        &mut session,
        &format!(
            "run command printf out; printf err >&2; printf x >> {}",
            marker.display()
        ),
    );
    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "approve");

    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert!(shell.command.contains("printf out"));
            assert_eq!(shell.cwd, root.display().to_string());
            assert_eq!(shell.stdout, "out");
            assert_eq!(shell.stderr, "err");
            assert_eq!(shell.exit_code, Some(0));
            assert!(!shell.timed_out);
            assert!(!shell.stdout_truncated);
            assert!(!shell.stderr_truncated);
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }
    assert_eq!(session.actions()[0].failure_reason, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApproved(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionFailed(_))),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approved_shell_command_timeout_records_shell_owned_timeout_result() {
    let controller = Controller::default();
    let root = regression_root("shell-command-timeout-controller");
    let root_string = root.display().to_string();
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "timeout-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "ShellCommand": {
                            "command": "sleep 2",
                            "cwd": root_string,
                            "timeout_seconds": 0
                        }
                    },
                    "state": "Proposed",
                    "summary": "run shell command sleep 2"
                },
                "verified_result": null,
                "failure_reason": null
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");

    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.command, "sleep 2");
            assert_eq!(shell.exit_code, None);
            assert!(shell.timed_out);
        }
        other => panic!("expected timeout shell result, got {other:?}"),
    }
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionFailed(_))),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approved_shell_command_output_caps_record_truncated_shell_result() {
    let controller = Controller::default();
    let root = regression_root("shell-command-output-caps-controller");
    let root_string = root.display().to_string();
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "cap-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "ShellCommand": {
                            "command": "printf abcdef; printf uvwxyz >&2",
                            "cwd": root_string,
                            "timeout_seconds": 30,
                            "output_caps": {
                                "stdout_bytes": 3,
                                "stderr_bytes": 4
                            }
                        }
                    },
                    "state": "Proposed",
                    "summary": "run capped shell command"
                },
                "verified_result": null,
                "failure_reason": null
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");

    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.stdout, "abc");
            assert_eq!(shell.stderr, "uvwx");
            assert!(shell.stdout_truncated);
            assert!(shell.stderr_truncated);
            assert_eq!(shell.exit_code, Some(0));
        }
        other => panic!("expected capped shell result, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approved_delete_move_and_directory_actions_record_verified_results() {
    let controller = Controller::default();
    let root = regression_root("approved-expanded-file-actions");

    let mut delete_session = session_at(&root);
    let delete_target = root.join("delete-me.txt");
    fs::write(&delete_target, "delete me").unwrap();
    controller.turn(&mut delete_session, "delete file delete-me.txt");
    controller.turn(&mut delete_session, "approve");

    assert!(!delete_target.exists());
    assert_eq!(
        delete_session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FileDeleted {
                path: delete_target.display().to_string()
            }
        ))
    );

    let mut move_session = session_at(&root);
    let source = root.join("old-name.txt");
    let target = root.join("new-name.txt");
    fs::write(&source, "move me").unwrap();
    let _ = fs::remove_file(&target);
    controller.turn(&mut move_session, "move file old-name.txt to new-name.txt");
    controller.turn(&mut move_session, "approve");

    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "move me");
    assert_eq!(
        move_session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FileMoved {
                source_path: source.display().to_string(),
                target_path: target.display().to_string()
            }
        ))
    );

    let mut directory_session = session_at(&root);
    let directory_target = root.join("generated");
    let _ = fs::remove_dir_all(&directory_target);
    controller.turn(&mut directory_session, "create directory generated");
    controller.turn(&mut directory_session, "approve");

    assert!(directory_target.is_dir());
    assert_eq!(
        directory_session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::DirectoryCreated {
                path: directory_target.display().to_string()
            }
        ))
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_text_cannot_approve_execute_or_verify_pending_shell_command() {
    let controller = Controller::new(BroadCapabilityClaimProvider);
    let root = regression_root("provider-cannot-apply-pending-shell");
    let mut session = session_at(&root);
    let marker = root.join("shell-ran.txt");

    Controller::default().turn(
        &mut session,
        &format!("run command printf x > {}", marker.display()),
    );
    controller.turn(&mut session, "what should happen next?");

    assert!(!marker.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );
    assert_eq!(provider_event_count(&session), 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_text_cannot_approve_or_verify_pending_delete_move_directory_actions() {
    let controller = Controller::new(BroadCapabilityClaimProvider);
    let root = regression_root("provider-cannot-apply-pending-expanded-file");
    let mut session = session_at(&root);
    let delete_target = root.join("delete-me.txt");
    fs::write(&delete_target, "keep").unwrap();

    Controller::default().turn(&mut session, "delete file delete-me.txt");
    controller.turn(&mut session, "what should happen next?");

    assert_eq!(fs::read_to_string(&delete_target).unwrap(), "keep");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn ambiguous_mixed_pending_file_and_shell_actions_remain_blocked() {
    let controller = Controller::default();
    let root = regression_root("ambiguous-mixed-file-shell-actions");
    let root_string = root.display().to_string();
    let marker = root.join("shell-ran.txt");
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "mixed-ambiguous-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "CreateFile": {
                            "target_path": "first.py",
                            "contents": ""
                        }
                    },
                    "state": "Proposed",
                    "summary": "write first.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-2",
                    "request": {
                        "ShellCommand": {
                            "command": format!("printf x > {}", marker.display()),
                            "cwd": root_string,
                            "timeout_seconds": 30
                        }
                    },
                    "state": "Proposed",
                    "summary": "run shell command"
                },
                "verified_result": null,
                "failure_reason": null
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "create file third.py");

    assert!(!root.join("first.py").exists());
    assert!(!root.join("third.py").exists());
    assert!(!marker.exists());
    assert_eq!(session.actions().len(), 2);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Proposed));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
                | Event::ActionProposed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_move_and_directory_actions_can_be_proposed_without_mutating_files() {
    let controller = Controller::default();
    let root = regression_root("new-file-actions-proposed");

    let mut delete_session = session_at(&root);
    let delete_target = root.join("delete-me.txt");
    fs::write(&delete_target, "keep").unwrap();
    let delete_result = controller.turn(&mut delete_session, "delete file delete-me.txt");

    assert_eq!(delete_result.route, Route::ProposeDeleteFile);
    assert_eq!(fs::read_to_string(&delete_target).unwrap(), "keep");
    assert_eq!(
        delete_session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    match &delete_session.actions()[0].action.request {
        ActionRequest::DeleteFile(action) => {
            assert_eq!(action.target_path, PathBuf::from("delete-me.txt"))
        }
        other => panic!("expected DeleteFile action, got {other:?}"),
    }

    let mut move_session = session_at(&root);
    let source = root.join("old-name.txt");
    let target = root.join("new-name.txt");
    fs::write(&source, "move me").unwrap();
    let move_result = controller.turn(
        &mut move_session,
        "rename file old-name.txt to new-name.txt",
    );

    assert_eq!(move_result.route, Route::ProposeMoveFile);
    assert_eq!(fs::read_to_string(&source).unwrap(), "move me");
    assert!(!target.exists());
    match &move_session.actions()[0].action.request {
        ActionRequest::MoveFile(action) => {
            assert_eq!(action.source_path, PathBuf::from("old-name.txt"));
            assert_eq!(action.target_path, PathBuf::from("new-name.txt"));
        }
        other => panic!("expected MoveFile action, got {other:?}"),
    }

    let mut directory_session = session_at(&root);
    let directory_target = root.join("generated");
    let directory_result = controller.turn(&mut directory_session, "create directory generated");

    assert_eq!(directory_result.route, Route::ProposeCreateDirectory);
    assert!(!directory_target.exists());
    match &directory_session.actions()[0].action.request {
        ActionRequest::CreateDirectory(action) => {
            assert_eq!(action.target_path, PathBuf::from("generated"));
        }
        other => panic!("expected CreateDirectory action, got {other:?}"),
    }

    assert_eq!(provider_event_count(&delete_session), 0);
    assert_eq!(provider_event_count(&move_session), 0);
    assert_eq!(provider_event_count(&directory_session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_folder_requests_route_to_directory_or_shell_action() {
    let controller = Controller::default();
    let root = regression_root("natural-folder-routing");

    let mut relative_session = session_at(&root);
    let relative_target = root.join("hello-world-local");
    let relative_result = controller.turn(
        &mut relative_session,
        "create a folder called hello-world-local",
    );

    assert_eq!(relative_result.route, Route::ProposeCreateDirectory);
    assert!(!relative_target.exists());
    assert_eq!(relative_session.actions().len(), 1);
    match &relative_session.actions()[0].action.request {
        ActionRequest::CreateDirectory(action) => {
            assert_eq!(action.target_path, PathBuf::from("hello-world-local"));
        }
        other => panic!("expected CreateDirectory action, got {other:?}"),
    }
    controller.turn(&mut relative_session, "approve");
    assert!(relative_target.is_dir());
    assert_eq!(provider_event_count(&relative_session), 0);

    let (home, _home) = isolated_home(&root);
    let mut desktop_session = session_at(&root);
    let desktop_result = controller.turn(
        &mut desktop_session,
        "can you create a folder called hello-world in the desktop?",
    );
    let expected_desktop_target = home.join("Desktop").join("hello-world");

    assert_eq!(desktop_result.route, Route::ProposeCreateDirectory);
    assert_eq!(desktop_session.actions().len(), 1);
    assert_eq!(provider_event_count(&desktop_session), 0);
    match &desktop_session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.starts_with("mkdir -p "));
            assert!(action.command.contains("hello-world"));
            assert_eq!(action.cwd, root);
            assert_eq!(
                action.expected_directory.as_ref(),
                Some(&expected_desktop_target)
            );
            assert!(action
                .expected_effect
                .contains(&expected_desktop_target.display().to_string()));
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }
    controller.turn(&mut desktop_session, "reject");
    assert!(!root.join("hello-world").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_desktop_folder_names_target_named_child_not_desktop_or_repo_the() {
    let controller = Controller::default();
    let root = regression_root("natural-desktop-folder-names");
    let home = root.join("home");
    let desktop = home.join("Desktop");
    fs::create_dir_all(&desktop).unwrap();
    let _home = EnvGuard::set("HOME", &home);

    for (index, (input, folder_name)) in [
        (
            "create a folder in desktop and call it helloworld",
            "helloworld",
        ),
        (
            "create a folder on the desktop called helloworld",
            "helloworld",
        ),
        ("create a folder called the demo on the desktop", "the demo"),
    ]
    .into_iter()
    .enumerate()
    {
        let project = root.join(format!("project-{index}"));
        fs::create_dir_all(&project).unwrap();
        let mut session = session_at(&project);
        let expected_target = desktop.join(folder_name);

        let result = controller.turn(&mut session, input);

        assert_eq!(result.route, Route::ProposeCreateDirectory);
        assert_eq!(provider_event_count(&session), 0);
        assert_eq!(session.actions().len(), 1);
        assert!(!expected_target.exists());
        assert!(!project.join("the").exists());
        match &session.actions()[0].action.request {
            ActionRequest::ShellCommand(action) => {
                assert!(action.command.starts_with("mkdir -p "));
                assert_eq!(action.cwd, project);
                assert_eq!(action.expected_directory.as_ref(), Some(&expected_target));
                assert_ne!(action.expected_directory.as_ref(), Some(&desktop));
                assert_ne!(
                    action.expected_directory.as_ref(),
                    Some(&project.join("the"))
                );
            }
            other => panic!("expected ShellCommand action, got {other:?}"),
        }
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_absolute_folder_request_runs_as_verified_shell_action_after_approval() {
    let controller = Controller::default();
    let root = regression_root("natural-folder-shell-absolute");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let parent = root.join("outside");
    let target = parent.join("hello-world");
    let mut session = session_at(&project);

    let result = controller.turn(
        &mut session,
        &format!("create a folder called hello-world at {}", parent.display()),
    );

    assert_eq!(result.route, Route::ProposeCreateDirectory);
    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.starts_with("mkdir -p "));
            assert!(action.command.contains(&target.display().to_string()));
            assert_eq!(action.cwd, project);
            assert_eq!(action.expected_directory.as_ref(), Some(&target));
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(target.is_dir());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.exit_code, Some(0));
            assert!(!shell.timed_out);
            let expected_effect = format!("verified directory exists: {}", target.display());
            assert_eq!(
                shell.verified_effect.as_deref(),
                Some(expected_effect.as_str())
            );
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn natural_quoted_absolute_folder_request_handles_shell_path_quoting() {
    let controller = Controller::default();
    let root = regression_root("natural-folder-shell-quoted");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let target = root.join("outside dir").join("kid's folder");
    let mut session = session_at(&project);

    let result = controller.turn(
        &mut session,
        &format!("create a folder at \"{}\"", target.display()),
    );

    assert_eq!(result.route, Route::ProposeCreateDirectory);
    assert_eq!(session.actions().len(), 1);
    match &session.actions()[0].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.starts_with("mkdir -p "));
            assert!(action.command.contains("'\\''"));
            assert_eq!(action.expected_directory.as_ref(), Some(&target));
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(target.is_dir());
    match session.actions()[0].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.exit_code, Some(0));
            assert!(shell.verified_effect.is_some());
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_folder_plan_prose_alone_does_not_create_shell_proposal() {
    let controller = Controller::new(FolderPlanProvider);
    let root = regression_root("provider-folder-plan-prose-no-shell");
    let base = root.join("Desktop");
    fs::create_dir_all(&base).unwrap();
    let mut session = session_at(&root);

    let plan_result = controller.model_turn(&mut session, "suggest folders for a project");
    assert_eq!(plan_result.route, Route::AskModel);
    assert_eq!(provider_assistant_message_count(&session), 1);

    let propose_result = controller.turn(
        &mut session,
        &format!("okay create this plan under {}", base.display()),
    );

    assert_eq!(propose_result.route, Route::ProposeCreateDirectory);
    assert!(session.actions().is_empty());
    assert_eq!(action_truth_event_count(&session), 0);
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("no target path could be parsed")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn followup_create_this_plan_proposes_and_verifies_all_planned_directories() {
    let controller = Controller::new(FolderPlanProvider);
    let root = regression_root("followup-folder-plan");
    let base = root.join("Desktop");
    fs::create_dir_all(&base).unwrap();
    let plan_path = base.join("folder-plan.md");
    let expected_dirs = ["src", "tests", "docs", "data"]
        .map(|name| base.join("project").join(name))
        .to_vec();
    let mut session = session_at(&root);

    let plan_result = controller.turn(
        &mut session,
        &format!("please create a plan at {}", plan_path.display()),
    );
    assert_eq!(plan_result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(provider_assistant_message_count(&session), 1);
    controller.turn(&mut session, "approve");
    assert!(plan_path.is_file());
    assert_eq!(
        session
            .project_memory()
            .latest_verified_plan()
            .map(|reference| reference.path.as_path()),
        Some(plan_path.as_path())
    );

    let propose_result = controller.turn(
        &mut session,
        &format!("okay create this plan under {}", base.display()),
    );

    assert_eq!(propose_result.route, Route::ProposeCreateDirectory);
    assert_eq!(session.actions().len(), 2);
    assert!(expected_dirs.iter().all(|path| !path.exists()));
    match &session.actions()[1].action.request {
        ActionRequest::ShellCommand(action) => {
            assert!(action.command.starts_with("mkdir -p "));
            for expected_dir in &expected_dirs {
                assert!(
                    action.command.contains(&expected_dir.display().to_string()),
                    "command did not contain {}: {}",
                    expected_dir.display(),
                    action.command
                );
            }
            assert_eq!(action.expected_directories, expected_dirs);
            assert_eq!(action.expected_directory, None);
        }
        other => panic!("expected ShellCommand action, got {other:?}"),
    }

    controller.turn(&mut session, "approve");

    assert!(expected_dirs.iter().all(|path| path.is_dir()));
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );
    match session.actions()[1].verified_result.as_ref() {
        Some(VerifiedActionResult::Shell(shell)) => {
            assert_eq!(shell.exit_code, Some(0));
            assert!(!shell.timed_out);
            assert!(shell
                .verified_effect
                .as_deref()
                .is_some_and(|effect| effect.contains("verified directories exist")));
        }
        other => panic!("expected verified shell result, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_verified_folder_plan_file_does_not_create_shell_proposal() {
    let controller = Controller::new(FolderPlanProvider);
    let root = regression_root("missing-folder-plan-file-no-shell");
    let base = root.join("Desktop");
    fs::create_dir_all(&base).unwrap();
    let plan_path = base.join("folder-plan.md");
    let expected_dir = base.join("project").join("src");
    let mut session = session_at(&root);

    controller.turn(
        &mut session,
        &format!("please create a plan at {}", plan_path.display()),
    );
    controller.turn(&mut session, "approve");
    assert!(plan_path.is_file());
    fs::remove_file(&plan_path).unwrap();

    let actions_before = session.actions().len();
    let truth_events_before = action_truth_event_count(&session);
    let result = controller.turn(
        &mut session,
        &format!("okay create this plan under {}", base.display()),
    );

    assert_eq!(result.route, Route::ProposeCreateDirectory);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(action_truth_event_count(&session), truth_events_before);
    assert!(!expected_dir.exists());
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified Markdown plan is missing")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn markdown_plan_that_folder_followup_reports_stale_latest_folder_without_fallback() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("markdown-plan-stale-that-folder");
    let folder_a = root.join("Desktop").join("FolderA");
    let folder_b = root.join("Desktop").join("FolderB");
    let mut session = session_at(&root);

    for folder in [&folder_a, &folder_b] {
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
    }
    assert!(folder_a.is_dir());
    assert!(folder_b.is_dir());
    fs::remove_dir_all(&folder_b).unwrap();

    let actions_before = session.actions().len();
    let provider_events_before = provider_event_count(&session);
    let result = controller.turn(
        &mut session,
        "create a plan for a small python project inside that folder",
    );

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(provider_event_count(&session), provider_events_before);
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified folder is missing")));
    assert!(!folder_a.join("small-python-project-plan.md").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_markdown_plan_that_folder_reports_stale_latest_folder_before_accepting_md_token() {
    let controller = Controller::new(SimpleMarkdownPlanProvider);
    let root = regression_root("markdown-plan-explicit-md-stale-that-folder");
    let folder_a = root.join("Desktop").join("FolderA");
    let folder_b = root.join("Desktop").join("FolderB");
    let mut session = session_at(&root);

    for folder in [&folder_a, &folder_b] {
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
    }
    assert!(folder_a.is_dir());
    assert!(folder_b.is_dir());
    fs::remove_dir_all(&folder_b).unwrap();

    let actions_before = session.actions().len();
    let provider_events_before = provider_event_count(&session);
    let result = controller.turn(&mut session, "create a plan foo.md inside that folder");

    assert_eq!(result.route, Route::ProposeMarkdownPlanFile);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(provider_event_count(&session), provider_events_before);
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified folder is missing")));
    assert!(!root.join("foo.md").exists());
    assert!(!folder_a.join("foo.md").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn create_plan_that_folder_followup_reports_stale_folder_without_default_fallback() {
    let controller = Controller::new(FolderPlanProvider);
    let root = regression_root("folder-plan-stale-that-folder");
    let folder_a = root.join("Desktop").join("FolderA");
    let folder_b = root.join("Desktop").join("FolderB");
    let plan_path = folder_a.join("folder-plan.md");
    let mut session = session_at(&root);

    for folder in [&folder_a, &folder_b] {
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
    }
    controller.turn(
        &mut session,
        &format!("please create a plan at {}", plan_path.display()),
    );
    controller.turn(&mut session, "approve");
    assert!(plan_path.is_file());
    fs::remove_dir_all(&folder_b).unwrap();

    let actions_before = session.actions().len();
    let truth_events_before = action_truth_event_count(&session);
    let result = controller.turn(&mut session, "okay create this plan inside that folder");

    assert_eq!(result.route, Route::ProposeCreateDirectory);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(action_truth_event_count(&session), truth_events_before);
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified folder is missing")));
    assert!(!folder_a.join("project").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn create_plan_that_folder_with_trailing_text_reports_stale_folder_without_plan_root_fallback() {
    let controller = Controller::new(FolderPlanProvider);
    let root = regression_root("folder-plan-stale-that-folder-please");
    let folder_a = root.join("Desktop").join("FolderA");
    let folder_b = root.join("Desktop").join("FolderB");
    let plan_path = folder_a.join("folder-plan.md");
    let mut session = session_at(&root);

    for folder in [&folder_a, &folder_b] {
        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
    }
    controller.turn(
        &mut session,
        &format!("please create a plan at {}", plan_path.display()),
    );
    controller.turn(&mut session, "approve");
    assert!(plan_path.is_file());
    fs::remove_dir_all(&folder_b).unwrap();

    let actions_before = session.actions().len();
    let truth_events_before = action_truth_event_count(&session);
    let result = controller.turn(
        &mut session,
        "okay create this plan inside that folder please",
    );

    assert_eq!(result.route, Route::ProposeCreateDirectory);
    assert_eq!(session.actions().len(), actions_before);
    assert_eq!(action_truth_event_count(&session), truth_events_before);
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("latest verified folder is missing")));
    assert!(!folder_a.join("project").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_delete_move_and_directory_actions_do_not_mutate_files() {
    let controller = Controller::default();
    let root = regression_root("new-file-actions-rejected");

    let mut delete_session = session_at(&root);
    let delete_target = root.join("delete-me.txt");
    fs::write(&delete_target, "keep").unwrap();
    controller.turn(&mut delete_session, "delete file delete-me.txt");
    controller.turn(&mut delete_session, "reject");
    controller.turn(&mut delete_session, "approve");

    assert_eq!(fs::read_to_string(&delete_target).unwrap(), "keep");
    assert_eq!(
        delete_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(delete_session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&delete_session, |event| matches!(
            event,
            Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let mut move_session = session_at(&root);
    let source = root.join("old-name.txt");
    let target = root.join("new-name.txt");
    fs::write(&source, "move me").unwrap();
    let _ = fs::remove_file(&target);
    controller.turn(&mut move_session, "move file old-name.txt to new-name.txt");
    controller.turn(&mut move_session, "reject");
    controller.turn(&mut move_session, "approve");

    assert_eq!(fs::read_to_string(&source).unwrap(), "move me");
    assert!(!target.exists());
    assert_eq!(
        move_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(move_session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&move_session, |event| matches!(
            event,
            Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let mut directory_session = session_at(&root);
    let directory_target = root.join("generated");
    controller.turn(&mut directory_session, "mkdir generated");
    controller.turn(&mut directory_session, "reject");
    controller.turn(&mut directory_session, "approve");

    assert!(!directory_target.exists());
    assert_eq!(
        directory_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(directory_session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&directory_session, |event| matches!(
            event,
            Event::ActionApplied(_) | Event::ActionFailed(_)
        )),
        0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_file_lifecycle_requires_user_approval_and_records_terminal_states() {
    let controller = Controller::default();
    let root = regression_root("write-file-lifecycle");
    let mut rejected_session = session_at(&root);
    let rejected_target = root.join("rejected.py");

    let proposed = controller.turn(&mut rejected_session, "create file rejected.py");

    assert_eq!(proposed.route, Route::ProposeWriteFile);
    assert!(!rejected_target.exists());
    assert_eq!(
        rejected_session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert!(proposed
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));

    controller.turn(&mut rejected_session, "reject");
    controller.turn(&mut rejected_session, "approve");

    assert!(!rejected_target.exists());
    assert_eq!(
        rejected_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(rejected_session.actions()[0].verified_result, None);
    assert!(rejected_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));
    assert!(rejected_session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let mut approved_session = session_at(&root);
    let approved_target = root.join("approved.py");
    let other_target = root.join("other.py");

    controller.turn(&mut approved_session, "create file approved.py");
    controller.turn(&mut approved_session, "approve");

    assert!(approved_target.exists());
    assert_eq!(fs::read_to_string(&approved_target).unwrap(), "");
    assert!(!other_target.exists());
    assert_eq!(
        approved_session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        approved_session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: approved_target.display().to_string()
        })
    );
    assert!(approved_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(approved_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let mut nested_session = session_at(&root);
    let nested_target = root.join("missing").join("nested.py");

    controller.turn(&mut nested_session, "create file missing/nested.py");
    controller.turn(&mut nested_session, "approve");

    assert!(nested_target.exists());
    assert_eq!(fs::read_to_string(&nested_target).unwrap(), "");
    assert_eq!(
        nested_session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        nested_session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: nested_target.display().to_string()
        })
    );
    assert_eq!(nested_session.actions()[0].failure_reason, None);
    assert!(nested_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn edit_and_overwrite_file_actions_require_approval_and_verified_results() {
    let controller = Controller::default();
    let root = regression_root("edit-overwrite-lifecycle");

    let mut rejected_edit_session = session_at(&root);
    let edit_target = root.join("edit.txt");
    fs::write(&edit_target, "old contents").unwrap();
    controller.turn(
        &mut rejected_edit_session,
        "edit file edit.txt replace old with new",
    );
    controller.turn(&mut rejected_edit_session, "reject");
    controller.turn(&mut rejected_edit_session, "approve");

    assert_eq!(fs::read_to_string(&edit_target).unwrap(), "old contents");
    assert_eq!(
        rejected_edit_session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(rejected_edit_session.actions()[0].verified_result, None);

    let mut approved_edit_session = session_at(&root);
    fs::write(&edit_target, "old contents").unwrap();
    let edit = controller.turn(
        &mut approved_edit_session,
        "edit file edit.txt replace old with new",
    );
    controller.turn(&mut approved_edit_session, "approve");

    assert_eq!(edit.route, Route::ProposePatchFile);
    assert_eq!(fs::read_to_string(&edit_target).unwrap(), "new contents");
    assert_eq!(
        approved_edit_session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FilePatched {
                path: edit_target.display().to_string()
            }
        ))
    );

    let mut approved_overwrite_session = session_at(&root);
    let overwrite_target = root.join("overwrite.txt");
    fs::write(&overwrite_target, "original").unwrap();
    let overwrite = controller.turn(
        &mut approved_overwrite_session,
        "overwrite file overwrite.txt with replacement",
    );
    controller.turn(&mut approved_overwrite_session, "approve");

    assert_eq!(overwrite.route, Route::ProposeOverwriteFile);
    assert_eq!(
        fs::read_to_string(&overwrite_target).unwrap(),
        "replacement"
    );
    assert_eq!(
        approved_overwrite_session.actions()[0].verified_result,
        Some(VerifiedActionResult::File(
            FileActionVerification::FileOverwritten {
                path: overwrite_target.display().to_string()
            }
        ))
    );
    assert_eq!(
        event_count(&approved_overwrite_session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_)
        )),
        2
    );
    assert_eq!(provider_event_count(&approved_overwrite_session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn second_write_file_proposal_is_blocked_while_one_action_is_pending() {
    let controller = Controller::default();
    let root = regression_root("single-pending-proposal");
    let mut session = session_at(&root);

    let first = controller.turn(&mut session, "create file first.py");
    let second = controller.turn(&mut session, "create file second.py");

    assert_eq!(first.route, Route::ProposeWriteFile);
    assert_eq!(second.route, Route::ProposeWriteFile);
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        1
    );
    assert!(second
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionProposed(_))));
    assert!(controller_messages(&session).iter().any(|message| message
        .contains("Approve or reject it before requesting another CreateFile action")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approving_after_blocked_second_proposal_applies_only_original_action() {
    let controller = Controller::default();
    let root = regression_root("approve-original-after-blocked");
    let mut session = session_at(&root);
    let original = root.join("original.py");
    let blocked = root.join("blocked.py");

    controller.turn(&mut session, "create file original.py");
    controller.turn(&mut session, "create file blocked.py");
    controller.turn(&mut session, "approve");

    assert!(original.exists());
    assert!(!blocked.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten {
            path: original.display().to_string()
        })
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        1
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_and_reject_with_no_pending_action_are_safe() {
    let controller = Controller::default();
    let root = regression_root("no-pending-actions");
    let mut session = session_at(&root);

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");

    assert!(session.actions().is_empty());
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionRejected(_)
                | Event::ActionApplied(_)
                | Event::ActionFailed(_)
        )),
        0
    );
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for approval.")));
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for rejection.")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn restored_session_with_multiple_proposed_actions_cannot_apply_hidden_actions() {
    let controller = Controller::default();
    let root = regression_root("ambiguous-restored-actions");
    let first = root.join("first.py");
    let second = root.join("second.py");
    let third = root.join("third.py");
    let root_string = root.display().to_string();
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "restored-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "CreateFile": {
                            "target_path": "first.py",
                            "contents": ""
                        }
                    },
                    "state": "Proposed",
                    "summary": "write first.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-2",
                    "request": {
                        "CreateFile": {
                            "target_path": "second.py",
                            "contents": ""
                        }
                    },
                    "state": "Proposed",
                    "summary": "write second.py"
                },
                "verified_result": null,
                "failure_reason": null
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "create file third.py");

    assert!(!first.exists());
    assert!(!second.exists());
    assert!(!third.exists());
    assert_eq!(session.actions().len(), 2);
    assert!(session
        .actions()
        .iter()
        .all(|record| record.action.state == ActionLifecycleState::Proposed));
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        0
    );
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("Multiple proposed actions are waiting")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn restored_terminal_actions_are_not_selected_or_allowed_to_block_new_proposals() {
    let controller = Controller::default();
    let root = regression_root("terminal-restored-actions");
    let approved = root.join("approved.py");
    let applied = root.join("applied.py");
    let rejected = root.join("rejected.py");
    let failed = root.join("failed.py");
    let fresh = root.join("fresh.py");
    let root_string = root.display().to_string();
    let mut session: Session = serde_json::from_value(serde_json::json!({
        "id": "restored-session",
        "project_root": root_string,
        "cwd": root_string,
        "events": [],
        "actions": [
            {
                "action": {
                    "id": "action-1",
                    "request": {
                        "CreateFile": {
                            "target_path": "approved.py",
                            "contents": ""
                        }
                    },
                    "state": "Approved",
                    "summary": "write approved.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-2",
                    "request": {
                        "CreateFile": {
                            "target_path": "applied.py",
                            "contents": ""
                        }
                    },
                    "state": "Applied",
                    "summary": "write applied.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-3",
                    "request": {
                        "CreateFile": {
                            "target_path": "rejected.py",
                            "contents": ""
                        }
                    },
                    "state": "Rejected",
                    "summary": "write rejected.py"
                },
                "verified_result": null,
                "failure_reason": null
            },
            {
                "action": {
                    "id": "action-4",
                    "request": {
                        "CreateFile": {
                            "target_path": "failed.py",
                            "contents": ""
                        }
                    },
                    "state": "Failed",
                    "summary": "write failed.py"
                },
                "verified_result": null,
                "failure_reason": "restored failure"
            }
        ],
        "provider_metadata": null
    }))
    .unwrap();

    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "create file fresh.py");

    assert!(!approved.exists());
    assert!(!applied.exists());
    assert!(!rejected.exists());
    assert!(!failed.exists());
    assert!(!fresh.exists());
    assert_eq!(session.actions().len(), 5);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Approved
    );
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );
    assert_eq!(
        session.actions()[2].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(
        session.actions()[3].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(
        session.actions()[4].action.state,
        ActionLifecycleState::Proposed
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_)
                | Event::ActionApplied(_)
                | Event::ActionRejected(_)
                | Event::ActionFailed(_)
        )),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionProposed(_))),
        1
    );
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for approval.")));
    assert!(controller_messages(&session)
        .iter()
        .any(|message| message.contains("No proposed action is waiting for rejection.")));
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_action_remains_terminal_after_followup_approval_or_rejection() {
    let controller = Controller::default();
    let root = regression_root("rejected-terminal");
    let mut session = session_at(&root);
    let target = root.join("terminal.py");

    controller.turn(&mut session, "create file terminal.py");
    controller.turn(&mut session, "reject");
    controller.turn(&mut session, "approve");
    controller.turn(&mut session, "reject");

    assert!(!target.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionRejected(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(
            event,
            Event::ActionApproved(_) | Event::ActionApplied(_)
        )),
        0
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approving_write_file_existing_target_fails_without_overwriting() {
    let controller = Controller::default();
    let root = regression_root("existing-target");
    let mut session = session_at(&root);
    let target = root.join("existing.py");
    fs::write(&target, "original").unwrap();

    controller.turn(&mut session, "create file existing.py");
    controller.turn(&mut session, "approve");

    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("write target already exists")));
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionFailed(_))),
        1
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        0
    );
    assert_eq!(provider_event_count(&session), 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn failed_atomic_overwrite_records_no_verified_success_and_cleans_temp_file() {
    let controller = Controller::default();
    let root = regression_root("atomic-overwrite-failure");
    let mut session = session_at(&root);
    let target = root.join("directory-target");
    fs::create_dir(&target).unwrap();

    controller.turn(
        &mut session,
        "overwrite file directory-target with replacement",
    );
    controller.turn(&mut session, "approve");

    assert!(target.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0].failure_reason.is_some());
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionApplied(_))),
        0
    );
    assert_eq!(
        event_count(&session, |event| matches!(event, Event::ActionFailed(_))),
        1
    );
    assert_no_atomic_temp_files(&root);

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn approved_write_file_symlink_escape_fails_without_verified_result() {
    use std::os::unix::fs::symlink;

    let controller = Controller::default();
    let root = regression_root("symlink-escape");
    let outside = root.parent().unwrap().join(format!(
        "elgar-core-regression-{}-symlink-outside",
        std::process::id()
    ));
    let outside_target = outside.join("escaped.py");
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    let mut session = session_at(&root);

    controller.turn(&mut session, "create file link/escaped.py");
    controller.turn(&mut session, "approve");

    assert!(!outside_target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Failed
    );
    assert_eq!(session.actions()[0].verified_result, None);
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("target parent resolves outside the allowed root")));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn controller_events_and_renderer_report_inspectable_action_states() {
    let controller = Controller::default();
    let root = regression_root("renderer");
    let mut rejected_session = session_at(&root);

    controller.turn(&mut rejected_session, "create file rejected.py");
    controller.turn(&mut rejected_session, "reject");

    assert!(rejected_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(rejected_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));

    let rejected_rendered = render_session(&rejected_session);
    assert!(rejected_rendered.contains("action proposed: action-1 CreateFile write rejected.py"));
    assert!(rejected_rendered.contains("action rejected: action-1 CreateFile write rejected.py"));

    let mut applied_session = session_at(&root);
    let applied_target = root.join("applied.py");

    controller.turn(&mut applied_session, "create file applied.py");
    controller.turn(&mut applied_session, "approve");

    assert!(applied_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApproved(_))));
    assert!(applied_session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let applied_rendered = render_session(&applied_session);
    assert!(applied_rendered.contains("action proposed: action-1 CreateFile write applied.py"));
    assert!(applied_rendered.contains("action approved: action-1 CreateFile write applied.py"));
    assert!(applied_rendered.contains(&format!(
        "action applied: action-1 CreateFile file written: {}",
        applied_target.display()
    )));

    let _ = fs::remove_dir_all(root);
}
