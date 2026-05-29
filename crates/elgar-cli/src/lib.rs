use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

use elgar_core::{
    action_gate::ActionGate,
    agent_runtime::AgentRuntime,
    policy::PermissionPolicyMode,
    provider::{
        chat_lm_studio, ChatMessage, ControllerProvider, ProviderCompatibility, ProviderConfig,
        ProviderError, LM_STUDIO_DEFAULT_BASE_URL,
    },
    renderer::render_session,
    session::Session,
};
use serde::Deserialize;

pub mod perf;

pub const PROVIDER_SMOKE_COMMAND: &str = "provider-smoke";
pub const TUI_COMMAND: &str = "tui";
pub const TUI_TERMINAL_COMMAND: &str = "tui-terminal";
pub const PERF_BASELINE_COMMAND: &str = "perf-baseline";
pub const PROVIDER_SMOKE_DEFAULT_PROMPT: &str = "Say hello in one sentence.";
pub const LM_STUDIO_MODEL_ENV: &str = "ELGAR_LM_STUDIO_MODEL";
pub const LM_STUDIO_BASE_URL_ENV: &str = "ELGAR_LM_STUDIO_BASE_URL";
pub const PROVIDER_CONFIG_ENV: &str = "ELGAR_PROVIDER_CONFIG";
pub const PERMISSION_POLICY_MODE_ENV: &str = "ELGAR_PERMISSION_POLICY_MODE";
pub const PROJECT_ROOT_ENV: &str = "ELGAR_PROJECT_ROOT";
pub const INSTALL_REPO_ROOT_ENV: &str = "ELGAR_INSTALL_REPO_ROOT";
pub const PROVIDER_CONFIG_FILE: &str = "elgar-provider.json";
const AGENTS_FILE: &str = "AGENTS.md";

pub fn should_launch_terminal_tui_by_default(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProvider {
    pub config: ProviderConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub project_root: PathBuf,
    pub cwd: PathBuf,
}

impl RuntimePaths {
    pub fn from_current_dir() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::from_cwd(cwd)
    }

    pub fn from_cwd(cwd: impl AsRef<Path>) -> Self {
        let cwd = cwd.as_ref().to_path_buf();
        let project_root = resolve_runtime_project_root(&cwd, installed_project_root());
        Self { project_root, cwd }
    }
}

pub fn resolve_runtime_project_root(
    cwd: impl AsRef<Path>,
    installed_root: Option<PathBuf>,
) -> PathBuf {
    let cwd = cwd.as_ref();

    env_project_root()
        .or_else(explicit_provider_config_root)
        .or_else(|| find_elgar_project_root(cwd))
        .or_else(|| installed_root.filter(|root| is_elgar_project_root(root)))
        .unwrap_or_else(|| cwd.to_path_buf())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeProviderConfigError {
    InvalidEnvironment { name: &'static str },
    ReadFailed { path: PathBuf, message: String },
    ParseFailed { path: PathBuf, message: String },
    UnsupportedProvider { provider: String },
    MissingModel { path: PathBuf },
    InvalidPermissionPolicyMode { source: String, value: String },
}

impl std::fmt::Display for RuntimeProviderConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEnvironment { name } => {
                write!(
                    formatter,
                    "provider config failed: environment variable {name} is not valid Unicode"
                )
            }
            Self::ReadFailed { path, message } => {
                write!(
                    formatter,
                    "provider config failed: could not read {}: {message}",
                    path.display()
                )
            }
            Self::ParseFailed { path, message } => {
                write!(
                    formatter,
                    "provider config failed: could not parse {}: {message}",
                    path.display()
                )
            }
            Self::UnsupportedProvider { provider } => {
                write!(
                    formatter,
                    "provider config failed: unsupported provider {provider}"
                )
            }
            Self::MissingModel { path } => {
                write!(
                    formatter,
                    "provider config failed: {} is live but has no default_model",
                    path.display()
                )
            }
            Self::InvalidPermissionPolicyMode { source, value } => {
                write!(
                    formatter,
                    "permission policy config failed: {source} has invalid mode `{value}`; expected review_all, auto_create_review_modify, workspace_write_with_review, or full_access"
                )
            }
        }
    }
}

impl std::error::Error for RuntimeProviderConfigError {}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct RuntimeProviderConfigFile {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    timeout_millis: Option<u64>,
    #[serde(default)]
    connect_timeout_millis: Option<u64>,
    #[serde(default)]
    read_timeout_millis: Option<u64>,
    #[serde(default)]
    write_timeout_millis: Option<u64>,
    #[serde(default)]
    request_timeout_millis: Option<u64>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    context_window_tokens: Option<u64>,
    #[serde(default)]
    compatibility: ProviderCompatibility,
    #[serde(default)]
    permission_policy_mode: Option<String>,
}

pub fn render_cli_turn(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let runtime = AgentRuntime::default();
    let mut session = Session::new("cli-smoke-session", project_root.as_ref(), cwd.as_ref());

    runtime.refresh_context_accounting(&mut session, None);
    runtime.turn(&mut session, input, default_permission_policy_mode());
    render_session(&session)
}

pub fn render_cli_turn_from_runtime_config(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, RuntimeProviderConfigError> {
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();
    let Some(runtime_provider) = load_runtime_provider(project_root_ref)? else {
        return Ok(render_cli_turn(input, project_root_ref, cwd_ref));
    };

    let policy_mode = runtime_permission_policy_mode(project_root_ref)?;
    let context_window_tokens = runtime_provider.config.configured_context_window_tokens();
    let runtime = AgentRuntime::with_lm_studio_provider(runtime_provider.config);
    let mut session = Session::new("cli-runtime-session", project_root_ref, cwd_ref);

    runtime.refresh_context_accounting(&mut session, context_window_tokens);
    runtime.turn(&mut session, input, policy_mode);
    Ok(render_session(&session))
}

pub fn load_runtime_provider(
    start: impl AsRef<Path>,
) -> Result<Option<RuntimeProvider>, RuntimeProviderConfigError> {
    let Some(path) = runtime_provider_config_path(start)? else {
        return Ok(None);
    };

    let contents =
        fs::read_to_string(&path).map_err(|error| RuntimeProviderConfigError::ReadFailed {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let file: RuntimeProviderConfigFile = serde_json::from_str(&contents).map_err(|error| {
        RuntimeProviderConfigError::ParseFailed {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;

    runtime_provider_from_file(path, file)
}

pub fn default_permission_policy_mode() -> PermissionPolicyMode {
    PermissionPolicyMode::AutoCreateReviewModify
}

pub fn runtime_permission_policy_mode(
    start: impl AsRef<Path>,
) -> Result<PermissionPolicyMode, RuntimeProviderConfigError> {
    match std::env::var(PERMISSION_POLICY_MODE_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return PermissionPolicyMode::parse(trimmed).map_err(|_| {
                    RuntimeProviderConfigError::InvalidPermissionPolicyMode {
                        source: PERMISSION_POLICY_MODE_ENV.to_string(),
                        value: value.to_string(),
                    }
                });
            }
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(RuntimeProviderConfigError::InvalidEnvironment {
                name: PERMISSION_POLICY_MODE_ENV,
            });
        }
    }

    let Some(path) = runtime_provider_config_path(start)? else {
        return Ok(default_permission_policy_mode());
    };

    let contents =
        fs::read_to_string(&path).map_err(|error| RuntimeProviderConfigError::ReadFailed {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let file: RuntimeProviderConfigFile = serde_json::from_str(&contents).map_err(|error| {
        RuntimeProviderConfigError::ParseFailed {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;

    let Some(value) = file
        .permission_policy_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(default_permission_policy_mode());
    };

    PermissionPolicyMode::parse(value).map_err(|_| {
        RuntimeProviderConfigError::InvalidPermissionPolicyMode {
            source: path.display().to_string(),
            value: value.to_string(),
        }
    })
}

fn runtime_provider_config_path(
    start: impl AsRef<Path>,
) -> Result<Option<PathBuf>, RuntimeProviderConfigError> {
    match std::env::var(PROVIDER_CONFIG_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if matches!(trimmed, "" | "off" | "none" | "disabled") {
                return Ok(None);
            }
            return Ok(Some(PathBuf::from(trimmed)));
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(RuntimeProviderConfigError::InvalidEnvironment {
                name: PROVIDER_CONFIG_ENV,
            });
        }
    }

    Ok(find_provider_config_file(start))
}

fn find_provider_config_file(start: impl AsRef<Path>) -> Option<PathBuf> {
    let mut current = start.as_ref();
    loop {
        let candidate = current.join(PROVIDER_CONFIG_FILE);
        if candidate.exists() {
            return Some(candidate);
        }

        let parent = current.parent()?;
        current = parent;
    }
}

fn installed_project_root() -> Option<PathBuf> {
    option_env!("ELGAR_INSTALL_REPO_ROOT")
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
}

fn env_project_root() -> Option<PathBuf> {
    std::env::var(PROJECT_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|root| is_elgar_project_root(root))
}

fn explicit_provider_config_root() -> Option<PathBuf> {
    let value = std::env::var(PROVIDER_CONFIG_ENV).ok()?;
    let trimmed = value.trim();
    if matches!(trimmed, "" | "off" | "none" | "disabled") {
        return None;
    }

    Path::new(trimmed)
        .parent()
        .map(Path::to_path_buf)
        .filter(|root| is_elgar_project_root(root))
}

fn find_elgar_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if is_elgar_project_root(current) {
            return Some(current.to_path_buf());
        }

        let parent = current.parent()?;
        current = parent;
    }
}

fn is_elgar_project_root(root: &Path) -> bool {
    root.join(PROVIDER_CONFIG_FILE).exists() || root.join(AGENTS_FILE).exists()
}

fn runtime_provider_from_file(
    path: PathBuf,
    file: RuntimeProviderConfigFile,
) -> Result<Option<RuntimeProvider>, RuntimeProviderConfigError> {
    let mode = file.mode.trim();
    if !mode.eq_ignore_ascii_case("live") {
        return Ok(None);
    }

    let provider = if file.provider.trim().is_empty() {
        "lm-studio"
    } else {
        file.provider.trim()
    };
    if provider != "lm-studio" {
        return Err(RuntimeProviderConfigError::UnsupportedProvider {
            provider: provider.to_string(),
        });
    }

    let model = file
        .default_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| RuntimeProviderConfigError::MissingModel { path: path.clone() })?;

    let mut config = ProviderConfig::lm_studio(model);
    if let Some(base_url) = file
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        config.base_url = base_url.to_string();
    }
    if let Some(timeout_millis) = file.timeout_millis {
        config.timeout_millis = timeout_millis;
    }
    config.connect_timeout_millis = file.connect_timeout_millis;
    config.read_timeout_millis = file.read_timeout_millis;
    config.write_timeout_millis = file.write_timeout_millis;
    config.request_timeout_millis = file.request_timeout_millis;
    config.stream = file.stream;
    config.context_window_tokens = file.context_window_tokens;
    config.compatibility = file.compatibility;

    Ok(Some(RuntimeProvider {
        config,
        source_path: path,
    }))
}

pub fn is_tui_exit_command(input: &str) -> bool {
    matches!(input.trim(), "/exit" | "/quit" | "/q")
}

pub fn is_tui_help_command(input: &str) -> bool {
    matches!(input.trim(), "/help" | "/commands")
}

pub fn is_tui_approval_command(input: &str) -> bool {
    input.trim() == "/approve"
}

pub fn is_tui_rejection_command(input: &str) -> bool {
    input.trim() == "/reject"
}

pub fn is_tui_copy_command(input: &str) -> bool {
    input.trim() == "/copy"
}

pub fn is_tui_memory_command(input: &str) -> bool {
    input.trim() == "/memory"
}

pub fn is_tui_state_snapshot_command(input: &str) -> bool {
    input.trim() == "/state"
}

pub fn is_tui_status_command(input: &str) -> bool {
    input.trim() == "/status"
}

pub fn is_tui_pending_command(input: &str) -> bool {
    input.trim() == "/pending"
}

pub fn is_tui_created_command(input: &str) -> bool {
    input.trim() == "/created"
}

pub fn is_tui_plan_preview_command(input: &str) -> bool {
    matches!(input.trim(), "/plan" | "/plan preview")
}

pub fn is_tui_reasoning_command(input: &str) -> bool {
    matches!(input.trim(), "/reasoning" | "/trace")
}

pub fn is_tui_tokens_command(input: &str) -> bool {
    input.trim() == "/tokens"
}

pub fn tui_tool_command_argument(input: &str) -> Option<&str> {
    input.trim().strip_prefix("/tool ").map(str::trim)
}

pub fn tui_permission_command_argument(input: &str) -> Option<Option<&str>> {
    let trimmed = input.trim();
    if matches!(trimmed, "/permissions" | "/policy") {
        return Some(None);
    }
    trimmed
        .strip_prefix("/permissions ")
        .or_else(|| trimmed.strip_prefix("/policy "))
        .map(str::trim)
        .map(|argument| {
            if argument.is_empty() {
                None
            } else {
                Some(argument)
            }
        })
}

pub fn is_tui_clear_command(input: &str) -> bool {
    matches!(input.trim(), "/clear" | "/new")
}

pub fn is_tui_cancel_command(input: &str) -> bool {
    input.trim() == "/cancel"
}

fn submit_tui_input<P>(
    shell: &mut elgar_tui::TuiShell,
    runtime: &AgentRuntime<P>,
    action_gate: &ActionGate<P>,
    session: &mut Session,
    input: &str,
) where
    P: ControllerProvider,
{
    if is_tui_approval_command(input) {
        shell.submit_approval(action_gate, session);
    } else if is_tui_rejection_command(input) {
        shell.submit_rejection(action_gate, session);
    } else if is_tui_cancel_command(input) {
        shell.push_local_message("No active provider turn to cancel.");
    } else if let Some(argument) = tui_permission_command_argument(input) {
        let message = shell.apply_permission_command(argument);
        shell.push_local_message(message);
    } else if let Some(tool_request) = tui_tool_command_argument(input) {
        shell.submit_agent_tool_input(runtime, session, tool_request);
    } else {
        shell.submit_agent_input(runtime, session, input);
    }
}

pub fn render_tui_help() -> &'static str {
    "Commands\n/commands              Show commands\n/clear                 Clear the visible conversation\n/new                   Clear the visible conversation\n/cancel                Cancel the active provider turn\n/tool <request>        Run an explicit tool-enabled turn\n/approve               Apply the pending action\n/reject                Reject the pending action\n/state                 Show verified state snapshot\n/status                Show session status\n/tokens                Show token and context usage\n/pending               Show pending action\n/created               Show verified creations\n/memory                Show verified memory\n/plan                  Preview latest structured plan\n/plan preview          Preview latest structured plan\n/reasoning             Show latest reasoning trace\n/trace                 Show latest reasoning trace\n/permissions           Show permission mode\n/permissions next      Cycle permission mode\n/permissions <mode>    Set permission mode\n/copy                  Copy the conversation\n/exit                  Quit\n/quit                  Quit\n/q                     Quit\n/help                  Show commands"
}

pub fn render_tui_script<I, S>(
    inputs: I,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    render_tui_script_with_policy(inputs, project_root, cwd, default_permission_policy_mode())
}

pub fn render_tui_script_with_policy<I, S>(
    inputs: I,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let action_gate = ActionGate::default();
    let runtime = AgentRuntime::default();
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::with_policy_mode(policy_mode);
    let mut rendered_turns = Vec::new();

    for input in inputs {
        let input = input.as_ref();
        if is_tui_exit_command(input) {
            break;
        }

        if is_tui_help_command(input) {
            rendered_turns.push(render_tui_help().to_string());
        } else if is_tui_clear_command(input) {
            shell.clear_conversation();
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_cancel_command(input) {
            shell.push_local_message("No active provider turn to cancel.");
            rendered_turns.push(shell.render_scripted_transcript());
        } else if is_tui_copy_command(input) {
            rendered_turns.push(shell.conversation_copy_text());
        } else if is_tui_state_snapshot_command(input) {
            rendered_turns.push(elgar_tui::render_session_state_snapshot(&session));
        } else if is_tui_status_command(input) {
            rendered_turns.push(elgar_tui::render_session_status(&session));
        } else if is_tui_tokens_command(input) {
            rendered_turns.push(elgar_tui::render_session_tokens(&session));
        } else if is_tui_pending_command(input) {
            rendered_turns.push(elgar_tui::render_session_pending_action(&session));
        } else if is_tui_created_command(input) {
            rendered_turns.push(elgar_tui::render_session_created_actions(&session));
        } else if is_tui_memory_command(input) {
            rendered_turns.push(elgar_tui::render_session_memory(&session));
        } else if is_tui_plan_preview_command(input) {
            rendered_turns.push(elgar_tui::render_session_plan_preview(&session));
        } else if is_tui_reasoning_command(input) {
            rendered_turns.push(elgar_tui::render_session_reasoning(&session));
        } else {
            submit_tui_input(&mut shell, &runtime, &action_gate, &mut session, input);
            rendered_turns.push(shell.render_scripted_transcript());
        }
    }

    rendered_turns.join("\n")
}

pub fn run_tui_loop<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    run_tui_loop_with_policy(
        reader,
        writer,
        project_root,
        cwd,
        default_permission_policy_mode(),
    )
}

pub fn run_tui_loop_with_policy<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    run_tui_loop_with_runtime(
        reader,
        writer,
        project_root,
        cwd,
        AgentRuntime::default(),
        None,
        policy_mode,
    )
}

pub fn run_tui_loop_from_runtime_config_with_policy<R, W>(
    reader: R,
    writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let project_root_ref = project_root.as_ref();
    let cwd_ref = cwd.as_ref();
    match load_runtime_provider(project_root_ref).map_err(runtime_provider_config_io_error)? {
        Some(runtime_provider) => {
            let context_window_tokens = runtime_provider.config.configured_context_window_tokens();
            run_tui_loop_with_runtime(
                reader,
                writer,
                project_root_ref,
                cwd_ref,
                AgentRuntime::with_lm_studio_provider(runtime_provider.config),
                context_window_tokens,
                policy_mode,
            )
        }
        None => run_tui_loop_with_runtime(
            reader,
            writer,
            project_root_ref,
            cwd_ref,
            AgentRuntime::default(),
            None,
            policy_mode,
        ),
    }
}

fn runtime_provider_config_io_error(error: RuntimeProviderConfigError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

fn run_tui_loop_with_runtime<R, W, P>(
    reader: R,
    mut writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    runtime: AgentRuntime<P>,
    context_window_tokens: Option<u64>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    P: ControllerProvider + Clone,
{
    let action_gate = ActionGate::new(runtime.provider.clone());
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    runtime.refresh_context_accounting(&mut session, context_window_tokens);
    let mut shell = elgar_tui::TuiShell::with_policy_mode(policy_mode);

    writeln!(writer, "Elgar TUI. Type /exit, /quit, or /q to leave.")?;
    for line in reader.lines() {
        let input = line?;
        if is_tui_exit_command(&input) {
            writeln!(writer, "Exiting Elgar TUI.")?;
            break;
        }

        if is_tui_help_command(&input) {
            writeln!(writer, "{}", render_tui_help())?;
        } else if is_tui_clear_command(&input) {
            shell.clear_conversation();
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_cancel_command(&input) {
            shell.push_local_message("No active provider turn to cancel.");
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        } else if is_tui_copy_command(&input) {
            writeln!(writer, "{}", shell.conversation_copy_text())?;
        } else if is_tui_state_snapshot_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_state_snapshot(&session)
            )?;
        } else if is_tui_status_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_status(&session))?;
        } else if is_tui_tokens_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_tokens(&session))?;
        } else if is_tui_pending_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_pending_action(&session)
            )?;
        } else if is_tui_created_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_created_actions(&session)
            )?;
        } else if is_tui_memory_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_memory(&session))?;
        } else if is_tui_plan_preview_command(&input) {
            writeln!(
                writer,
                "{}",
                elgar_tui::render_session_plan_preview(&session)
            )?;
        } else if is_tui_reasoning_command(&input) {
            writeln!(writer, "{}", elgar_tui::render_session_reasoning(&session))?;
        } else {
            runtime.refresh_context_accounting(&mut session, context_window_tokens);
            submit_tui_input(&mut shell, &runtime, &action_gate, &mut session, &input);
            writeln!(writer, "{}", shell.render_scripted_transcript())?;
        }
    }

    Ok(())
}

pub fn run_tui_terminal() -> io::Result<()> {
    let paths = RuntimePaths::from_current_dir();
    let policy_mode = runtime_permission_policy_mode(&paths.project_root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    match load_runtime_provider(&paths.project_root) {
        Ok(Some(runtime)) => elgar_tui::run_terminal_shell_with_lm_studio_provider_at_with_policy(
            runtime.config,
            &paths.project_root,
            &paths.cwd,
            policy_mode,
        ),
        Ok(None) => elgar_tui::run_terminal_shell_at_with_policy(
            &paths.project_root,
            &paths.cwd,
            policy_mode,
        ),
        Err(error) => Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSmokeConfig {
    pub model: String,
    pub base_url: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSmokeError {
    MissingModel,
    InvalidEnvironment { name: &'static str },
    Provider(ProviderError),
}

impl std::fmt::Display for ProviderSmokeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingModel => write!(
                formatter,
                "LM Studio smoke failed: missing required environment variable {LM_STUDIO_MODEL_ENV}; set it to the loaded LM Studio model name"
            ),
            Self::InvalidEnvironment { name } => write!(
                formatter,
                "LM Studio smoke failed: environment variable {name} is not valid Unicode"
            ),
            Self::Provider(error) => write!(formatter, "LM Studio smoke failed: {error}"),
        }
    }
}

impl std::error::Error for ProviderSmokeError {}

pub fn provider_smoke_prompt(args: &[String]) -> String {
    normalize_prompt(args.join(" "))
}

pub fn provider_smoke_config_from_env(
    prompt: impl Into<String>,
) -> Result<ProviderSmokeConfig, ProviderSmokeError> {
    let model = read_env(LM_STUDIO_MODEL_ENV)?;
    let base_url = read_env(LM_STUDIO_BASE_URL_ENV)?;

    provider_smoke_config(model, base_url, prompt)
}

pub fn provider_smoke_config(
    model: Option<String>,
    base_url: Option<String>,
    prompt: impl Into<String>,
) -> Result<ProviderSmokeConfig, ProviderSmokeError> {
    let model = model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .ok_or(ProviderSmokeError::MissingModel)?;
    let base_url = base_url
        .map(|base_url| base_url.trim().to_string())
        .filter(|base_url| !base_url.is_empty())
        .unwrap_or_else(|| LM_STUDIO_DEFAULT_BASE_URL.to_string());

    Ok(ProviderSmokeConfig {
        model,
        base_url,
        prompt: normalize_prompt(prompt.into()),
    })
}

pub fn run_provider_smoke_from_env(prompt: &str) -> Result<String, ProviderSmokeError> {
    let config = provider_smoke_config_from_env(prompt)?;
    run_provider_smoke(config)
}

pub fn run_provider_smoke(config: ProviderSmokeConfig) -> Result<String, ProviderSmokeError> {
    let provider_config = provider_config_from_smoke_config(&config);

    chat_lm_studio(&provider_config, vec![ChatMessage::user(config.prompt)])
        .map(|output| output.text)
        .map_err(ProviderSmokeError::Provider)
}

fn provider_config_from_smoke_config(config: &ProviderSmokeConfig) -> ProviderConfig {
    ProviderConfig {
        base_url: config.base_url.clone(),
        model: Some(config.model.clone()),
        ..ProviderConfig::default()
    }
}

fn normalize_prompt(prompt: impl Into<String>) -> String {
    let prompt = prompt.into();
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        PROVIDER_SMOKE_DEFAULT_PROMPT.to_string()
    } else {
        trimmed.to_string()
    }
}

fn read_env(name: &'static str) -> Result<Option<String>, ProviderSmokeError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(ProviderSmokeError::InvalidEnvironment { name })
        }
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::{
        agent_runtime::AgentRuntime,
        event::ProviderOutput,
        policy::PermissionPolicyMode,
        provider::{
            ChatMessage, ControllerProvider, ProviderError, ProviderRequestMetadata,
            LM_STUDIO_DEFAULT_BASE_URL,
        },
    };
    use std::{fs, path::PathBuf};

    use super::{
        default_permission_policy_mode, is_tui_approval_command, is_tui_clear_command,
        is_tui_copy_command, is_tui_created_command, is_tui_exit_command, is_tui_help_command,
        is_tui_memory_command, is_tui_pending_command, is_tui_plan_preview_command,
        is_tui_reasoning_command, is_tui_rejection_command, is_tui_state_snapshot_command,
        is_tui_status_command, is_tui_tokens_command, load_runtime_provider, provider_smoke_config,
        provider_smoke_prompt, render_cli_turn_from_runtime_config, render_tui_help,
        render_tui_script, resolve_runtime_project_root, run_tui_loop, run_tui_loop_with_runtime,
        runtime_permission_policy_mode, should_launch_terminal_tui_by_default,
        tui_permission_command_argument, tui_tool_command_argument, ProviderSmokeError,
        RuntimePaths, RuntimeProviderConfigError, PROVIDER_CONFIG_FILE,
        PROVIDER_SMOKE_DEFAULT_PROMPT, TUI_COMMAND, TUI_TERMINAL_COMMAND,
    };

    #[derive(Debug, Clone)]
    struct ThinkingProvider;

    impl ControllerProvider for ThinkingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "thinking-provider",
                Some("test-model".to_string()),
                "request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(
                ProviderOutput::new("{\"route\":\"chat\",\"content\":\"visible answer\"}")
                    .with_thinking("Internal reasoning should stay hidden."),
            )
        }

        fn chat_messages_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<ProviderOutput, ProviderError> {
            self.chat("")
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-cli-lib-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn provider_smoke_prompt_defaults_when_no_prompt_is_passed() {
        assert_eq!(provider_smoke_prompt(&[]), PROVIDER_SMOKE_DEFAULT_PROMPT);
        assert_eq!(
            provider_smoke_prompt(&["   ".to_string()]),
            PROVIDER_SMOKE_DEFAULT_PROMPT
        );
    }

    #[test]
    fn provider_smoke_prompt_joins_terminal_args() {
        assert_eq!(
            provider_smoke_prompt(&["Say".to_string(), "hello.".to_string()]),
            "Say hello."
        );
    }

    #[test]
    fn default_terminal_launch_requires_interactive_stdio() {
        assert!(should_launch_terminal_tui_by_default(true, true));
        assert!(!should_launch_terminal_tui_by_default(false, true));
        assert!(!should_launch_terminal_tui_by_default(true, false));
        assert!(!should_launch_terminal_tui_by_default(false, false));
    }

    #[test]
    fn provider_smoke_config_requires_model_env_value() {
        let error = provider_smoke_config(None, None, "hello").unwrap_err();

        assert_eq!(error, ProviderSmokeError::MissingModel);
        assert!(error.to_string().contains("ELGAR_LM_STUDIO_MODEL"));

        let blank = provider_smoke_config(Some("   ".to_string()), None, "hello").unwrap_err();
        assert_eq!(blank, ProviderSmokeError::MissingModel);
    }

    #[test]
    fn provider_smoke_config_uses_default_base_url_and_prompt() {
        let config = provider_smoke_config(Some("local-model".to_string()), None, "  ").unwrap();

        assert_eq!(config.model, "local-model");
        assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(config.prompt, PROVIDER_SMOKE_DEFAULT_PROMPT);
    }

    #[test]
    fn provider_smoke_config_accepts_custom_base_url() {
        let config = provider_smoke_config(
            Some("local-model".to_string()),
            Some(" http://localhost:4321/v1 ".to_string()),
            "hello",
        )
        .unwrap();

        assert_eq!(config.base_url, "http://localhost:4321/v1");
        assert_eq!(config.prompt, "hello");
    }

    #[test]
    fn runtime_provider_config_loads_live_lm_studio_file() {
        let root = temp_root("runtime-provider-live");
        fs::write(
            root.join(PROVIDER_CONFIG_FILE),
            r#"{
              "provider": "lm-studio",
              "base_url": "http://127.0.0.1:1234/v1",
              "default_model": "openai/gpt-oss-20b",
              "mode": "live",
              "connect_timeout_millis": 1000,
              "read_timeout_millis": 120000,
              "write_timeout_millis": 2000,
              "request_timeout_millis": 180000,
              "context_window_tokens": 128000,
              "stream": true
            }"#,
        )
        .unwrap();

        let runtime = load_runtime_provider(&root).unwrap().unwrap();

        assert_eq!(runtime.config.provider, "lm-studio");
        assert_eq!(runtime.config.base_url, "http://127.0.0.1:1234/v1");
        assert_eq!(runtime.config.model.as_deref(), Some("openai/gpt-oss-20b"));
        assert_eq!(runtime.config.connect_timeout_millis(), 1000);
        assert_eq!(runtime.config.read_timeout_millis(), 120000);
        assert_eq!(runtime.config.write_timeout_millis(), 2000);
        assert_eq!(runtime.config.request_timeout_millis(), 180000);
        assert_eq!(runtime.config.context_window_tokens, Some(128_000));
        assert_eq!(
            runtime.config.configured_context_window_tokens(),
            Some(128_000)
        );
        assert!(runtime.config.stream);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_provider_config_loads_compatibility_metadata() {
        let root = temp_root("runtime-provider-compatibility");
        fs::write(
            root.join(PROVIDER_CONFIG_FILE),
            r#"{
              "provider": "lm-studio",
              "base_url": "http://127.0.0.1:1234/v1",
              "default_model": "openai/gpt-oss-20b",
              "mode": "live",
              "context_window_tokens": 32000,
              "compatibility": {
                "context_window_tokens": 128000,
                "output_token_limit_field": "max_tokens",
                "reasoning": {
                  "response_fields": ["reasoning_content"],
                  "stream_fields": ["reasoning_content", "thinking"]
                },
                "supports_streaming_usage": false,
                "supports_developer_role": true
              }
            }"#,
        )
        .unwrap();

        let runtime = load_runtime_provider(&root).unwrap().unwrap();

        assert_eq!(runtime.config.context_window_tokens, Some(32_000));
        assert_eq!(
            runtime.config.configured_context_window_tokens(),
            Some(128_000)
        );
        assert!(runtime.config.supports_developer_role());
        assert_eq!(
            runtime.config.compatibility.supports_streaming_usage,
            Some(false)
        );
        assert_eq!(
            runtime.config.compatibility.reasoning.stream_fields,
            vec!["reasoning_content", "thinking"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_provider_config_absent_keeps_stub_fallback() {
        let root = temp_root("runtime-provider-absent");

        assert_eq!(load_runtime_provider(&root).unwrap(), None);
        assert_eq!(
            runtime_permission_policy_mode(&root).unwrap(),
            default_permission_policy_mode()
        );

        let rendered = render_cli_turn_from_runtime_config("hello", &root, &root).unwrap();
        assert!(rendered.contains("provider started: stub-provider"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_permission_policy_mode_loads_from_provider_config() {
        let root = temp_root("runtime-policy-config");
        fs::write(
            root.join(PROVIDER_CONFIG_FILE),
            r#"{
              "provider": "lm-studio",
              "mode": "off",
              "permission_policy_mode": "review_all"
            }"#,
        )
        .unwrap();

        let mode = runtime_permission_policy_mode(&root).unwrap();

        assert_eq!(mode, PermissionPolicyMode::ReviewAll);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_permission_policy_mode_rejects_invalid_config_value() {
        let root = temp_root("runtime-policy-invalid");
        fs::write(
            root.join(PROVIDER_CONFIG_FILE),
            r#"{
              "permission_policy_mode": "trust_everything"
            }"#,
        )
        .unwrap();

        let error = runtime_permission_policy_mode(&root).unwrap_err();

        assert!(matches!(
            error,
            RuntimeProviderConfigError::InvalidPermissionPolicyMode { .. }
        ));
        assert!(error.to_string().contains("trust_everything"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_project_root_uses_installed_root_when_cwd_has_no_config() {
        let installed = temp_root("runtime-installed-root");
        let outside = temp_root("runtime-outside-root");
        fs::write(installed.join(PROVIDER_CONFIG_FILE), "{}").unwrap();

        let resolved = resolve_runtime_project_root(&outside, Some(installed.clone()));

        assert_eq!(resolved, installed);

        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_dir_all(installed);
    }

    #[test]
    fn runtime_project_root_prefers_cwd_config_over_installed_root() {
        let installed = temp_root("runtime-installed-root-cwd-loses");
        let workspace = temp_root("runtime-workspace-root");
        let child = workspace.join("child");
        fs::create_dir_all(&child).unwrap();
        fs::write(installed.join(PROVIDER_CONFIG_FILE), "{}").unwrap();
        fs::write(workspace.join(PROVIDER_CONFIG_FILE), "{}").unwrap();

        let resolved = resolve_runtime_project_root(&child, Some(installed.clone()));

        assert_eq!(resolved, workspace);

        let _ = fs::remove_dir_all(child);
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(installed);
    }

    #[test]
    fn runtime_paths_allow_cli_config_from_installed_root_outside_repo() {
        let installed = temp_root("runtime-installed-config-cli");
        let outside = temp_root("runtime-outside-config-cli");
        fs::write(
            installed.join(PROVIDER_CONFIG_FILE),
            r#"{
              "provider": "lm-studio",
              "base_url": "https://127.0.0.1:1234/v1",
              "default_model": "openai/gpt-oss-20b",
              "mode": "live"
            }"#,
        )
        .unwrap();

        let project_root = resolve_runtime_project_root(&outside, Some(installed.clone()));
        let paths = RuntimePaths {
            project_root,
            cwd: outside.clone(),
        };

        let rendered =
            render_cli_turn_from_runtime_config("Say hello.", &paths.project_root, &paths.cwd)
                .unwrap();

        assert!(rendered.contains("user: Say hello."));
        assert!(rendered.contains("provider started: lm-studio request lm-studio-request-"));
        assert!(rendered.contains("only http:// provider URLs are supported"));
        assert!(!rendered.contains("stub-provider"));

        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_dir_all(installed);
    }

    #[test]
    fn runtime_provider_config_live_requires_model() {
        let root = temp_root("runtime-provider-missing-model");
        let path = root.join(PROVIDER_CONFIG_FILE);
        fs::write(
            &path,
            r#"{
              "provider": "lm-studio",
              "mode": "live"
            }"#,
        )
        .unwrap();

        let error = load_runtime_provider(&root).unwrap_err();

        assert_eq!(error, RuntimeProviderConfigError::MissingModel { path });

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_exit_commands_are_explicit() {
        assert!(is_tui_exit_command("/exit"));
        assert!(is_tui_exit_command(" /quit "));
        assert!(is_tui_exit_command("/q"));
        assert!(!is_tui_exit_command("exit"));
        assert!(!is_tui_exit_command("q"));
        assert!(!is_tui_exit_command("quit"));
        assert!(!is_tui_exit_command("/help"));
    }

    #[test]
    fn terminal_tui_command_is_separate_from_line_loop_command() {
        assert_eq!(TUI_COMMAND, "tui");
        assert_eq!(TUI_TERMINAL_COMMAND, "tui-terminal");
    }

    #[test]
    fn tui_help_lists_only_supported_local_commands() {
        let help = render_tui_help();

        assert!(is_tui_help_command("/help"));
        assert!(is_tui_help_command(" /commands "));
        assert!(!is_tui_help_command("help"));
        assert!(!is_tui_help_command("/model"));
        assert!(help.starts_with("Commands\n/commands"));
        assert!(help.contains("/clear"));
        assert!(help.contains("/new"));
        assert!(help.contains("/cancel"));
        assert!(help.contains("/tool <request>"));
        assert!(help.contains("/approve"));
        assert!(help.contains("/reject"));
        assert!(help.contains("/state"));
        assert!(help.contains("/status"));
        assert!(help.contains("/pending"));
        assert!(help.contains("/created"));
        assert!(help.contains("/memory"));
        assert!(help.contains("/plan"));
        assert!(help.contains("/plan preview"));
        assert!(help.contains("/reasoning"));
        assert!(help.contains("/trace"));
        assert!(help.contains("/permissions"));
        assert!(help.contains("/copy"));
        assert!(help.contains("/exit"));
        assert!(help.contains("/quit"));
        assert!(help.contains("/q"));
        assert!(help.contains("/help"));
        assert!(!help.contains("/model"));
        assert!(!help.contains("/settings"));
        assert!(!help.contains("/login"));
        assert!(!help.contains("/provider"));
        assert!(!help.contains("/bash"));
        assert!(!help.contains("/api"));
    }

    #[test]
    fn tui_reasoning_command_is_explicit() {
        assert!(is_tui_reasoning_command("/reasoning"));
        assert!(is_tui_reasoning_command(" /trace "));
        assert!(!is_tui_reasoning_command("reasoning"));
        assert!(!is_tui_reasoning_command("trace"));
        assert!(!is_tui_reasoning_command("/plan"));
    }

    #[test]
    fn tui_permission_command_parses_show_cycle_and_set_forms() {
        assert_eq!(tui_permission_command_argument("/permissions"), Some(None));
        assert_eq!(tui_permission_command_argument("/policy"), Some(None));
        assert_eq!(
            tui_permission_command_argument("/permissions next"),
            Some(Some("next"))
        );
        assert_eq!(
            tui_permission_command_argument(" /policy full-access "),
            Some(Some("full-access"))
        );
        assert_eq!(tui_permission_command_argument("permissions"), None);
    }

    #[test]
    fn tui_tool_command_is_explicit() {
        assert_eq!(
            tui_tool_command_argument("/tool create file hello.py"),
            Some("create file hello.py")
        );
        assert_eq!(
            tui_tool_command_argument(" /tool create folder demo "),
            Some("create folder demo")
        );
        assert_eq!(tui_tool_command_argument("tool create file hello.py"), None);
        assert_eq!(tui_tool_command_argument("create file hello.py"), None);
    }

    #[test]
    fn tui_line_loop_cancel_command_is_documented_and_local() {
        let rendered = render_tui_script(["/cancel"], ".", ".");

        assert!(render_tui_help().contains("/cancel"));
        assert!(rendered.contains("No active provider turn to cancel."));
        assert!(!rendered.contains("> /cancel"));
        assert!(!rendered.contains("stub provider response"));
    }

    #[test]
    fn tui_approval_and_rejection_commands_are_explicit() {
        assert!(is_tui_approval_command("/approve"));
        assert!(is_tui_approval_command(" /approve "));
        assert!(is_tui_rejection_command("/reject"));
        assert!(is_tui_rejection_command(" /reject "));

        assert!(!is_tui_approval_command("approve"));
        assert!(!is_tui_rejection_command("reject"));
        assert!(!is_tui_approval_command("/approved"));
        assert!(!is_tui_rejection_command("/rejected"));
    }

    #[test]
    fn tui_copy_command_is_explicit() {
        assert!(is_tui_copy_command("/copy"));
        assert!(is_tui_copy_command(" /copy "));
        assert!(!is_tui_copy_command("copy"));
        assert!(!is_tui_copy_command("/clipboard"));
    }

    #[test]
    fn tui_memory_command_is_explicit() {
        assert!(is_tui_memory_command("/memory"));
        assert!(is_tui_memory_command(" /memory "));
        assert!(!is_tui_memory_command("memory"));
        assert!(!is_tui_memory_command("/mem"));
    }

    #[test]
    fn tui_state_snapshot_command_is_explicit() {
        assert!(is_tui_state_snapshot_command("/state"));
        assert!(is_tui_state_snapshot_command(" /state "));
        assert!(!is_tui_state_snapshot_command("state"));
        assert!(!is_tui_state_snapshot_command("what did you create?"));
        assert!(!is_tui_state_snapshot_command("/states"));
    }

    #[test]
    fn tui_plan_preview_command_is_explicit() {
        assert!(is_tui_plan_preview_command("/plan"));
        assert!(is_tui_plan_preview_command(" /plan preview "));
        assert!(!is_tui_plan_preview_command("plan"));
        assert!(!is_tui_plan_preview_command("preview plan"));
        assert!(!is_tui_plan_preview_command("/preview"));
    }

    #[test]
    fn tui_state_commands_are_explicit() {
        assert!(is_tui_status_command("/status"));
        assert!(is_tui_status_command(" /status "));
        assert!(is_tui_tokens_command("/tokens"));
        assert!(is_tui_state_snapshot_command("/state"));
        assert!(is_tui_pending_command("/pending"));
        assert!(is_tui_created_command("/created"));

        assert!(!is_tui_status_command("status"));
        assert!(!is_tui_tokens_command("tokens"));
        assert!(!is_tui_state_snapshot_command("state"));
        assert!(!is_tui_pending_command("pending"));
        assert!(!is_tui_created_command("created"));
        assert!(!is_tui_created_command("/create"));
    }

    #[test]
    fn tui_clear_commands_are_explicit() {
        assert!(is_tui_clear_command("/clear"));
        assert!(is_tui_clear_command(" /new "));
        assert!(!is_tui_clear_command("clear"));
        assert!(!is_tui_clear_command("new"));
        assert!(!is_tui_clear_command("/reset"));
    }

    #[test]
    fn tui_script_renders_default_stub_turns_and_stops_on_exit() {
        let rendered = render_tui_script(
            ["what does the harness do?", "/exit", "what should not run?"],
            ".",
            ".",
        );

        assert!(rendered.contains("> what does the harness do?"));
        assert!(rendered.contains("stub provider response"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("stub-request-1"));
        assert!(!rendered.contains("what should not run?"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_q_alias_exits_but_plain_q_is_text() {
        let rendered = render_tui_script(["q", "/q", "what should not run?"], ".", ".");

        assert!(rendered.contains("> q"));
        assert!(!rendered.contains("what should not run?"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_help_is_local_and_does_not_call_controller_or_provider() {
        let rendered = render_tui_script(["/help", "/commands"], ".", ".");

        assert!(rendered.contains("Commands\n/commands"));
        assert!(rendered.contains("/approve"));
        assert!(rendered.contains("/reject"));
        assert!(rendered.contains("/state"));
        assert!(rendered.contains("/status"));
        assert!(rendered.contains("/pending"));
        assert!(rendered.contains("/created"));
        assert!(rendered.contains("/memory"));
        assert!(rendered.contains("/plan"));
        assert!(rendered.contains("/plan preview"));
        assert!(rendered.contains("/reasoning"));
        assert!(rendered.contains("/trace"));
        assert!(rendered.contains("/copy"));
        assert!(!rendered.contains("> /help"));
        assert!(!rendered.contains("> /commands"));
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("stub-provider"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_reasoning_command_is_local_and_empty_without_provider_call() {
        let rendered = render_tui_script(["/reasoning", "/trace"], ".", ".");

        assert!(rendered.contains("Reasoning\n(none)"));
        assert!(!rendered.contains("> /reasoning"));
        assert!(!rendered.contains("stub provider response"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_copy_command_returns_full_conversation_without_provider_call() {
        let rendered = render_tui_script(["what does the harness do?", "/copy"], ".", ".");

        assert!(rendered.contains("> what does the harness do?"));
        assert!(rendered.contains("stub provider response"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("> /copy"));
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_memory_command_is_local_and_empty_without_provider_call() {
        let root = temp_root("memory-empty-command");

        let rendered = render_tui_script(["/memory", "/plan"], &root, &root);

        assert!(rendered.contains("Memory\n(empty)"));
        assert!(rendered.contains("Plan Preview\n(none)"));
        assert!(!rendered.contains("> /memory"));
        assert!(!rendered.contains("> /plan"));
        assert!(!rendered.contains("stub provider response"));
        assert!(!rendered.contains("lm-studio"));
        assert!(!rendered.contains("Input was not recognized"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_state_commands_are_local_and_empty_without_provider_call() {
        let root = temp_root("state-empty-command");

        let rendered =
            render_tui_script(["/state", "/status", "/pending", "/created"], &root, &root);

        assert!(rendered.contains("State\npending: none\napplied actions: 0\ncreated: (none)"));
        assert!(rendered.contains("memory: (none)"));
        assert!(rendered.contains("Status\nactions: 0\npending: none"));
        assert!(rendered.contains("Pending\nnone"));
        assert!(rendered.contains("Created\n(none)"));
        assert!(!rendered.contains("> /state"));
        assert!(!rendered.contains("> /status"));
        assert!(!rendered.contains("> /pending"));
        assert!(!rendered.contains("> /created"));
        assert!(!rendered.contains("stub provider response"));
        assert!(!rendered.contains("lm-studio"));
        assert!(!rendered.contains("Input was not recognized"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_clear_commands_clear_local_rendering_without_controller_call() {
        let root = temp_root("clear-command");

        let rendered = render_tui_script(
            ["what does the harness do?", "/clear", "/new"],
            &root,
            &root,
        );

        assert!(!rendered.contains("> /clear"));
        assert!(!rendered.contains("> /new"));
        assert!(rendered.contains("stub provider response"));
        assert!(rendered.contains("(empty conversation)"));
        assert_eq!(rendered.matches("stub provider response").count(), 1);
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_tool_command_is_explicit_and_stub_does_not_infer_actions() {
        let root = temp_root("explicit-tool-command");
        let target = root.join("hello.py");

        let rendered = render_tui_script(["/tool create file hello.py", "/status"], &root, &root);

        assert!(!target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains(
            "The model did not return any tool actions, so no files or commands were changed."
        ));
        assert!(rendered.contains("Status\nactions: 0\npending: none"));
        assert!(!rendered.contains("Wrote "));
        assert!(rendered.contains("Pending Action\nnone"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_permission_command_changes_runtime_policy_without_phrase_trigger() {
        let root = temp_root("permission-toggle-command");

        let rendered = render_tui_script(
            [
                "/permissions review_all",
                "create file gated.py",
                "/reject",
                "/permissions auto_create_review_modify",
                "create file allowed.py",
            ],
            &root,
            &root,
        );

        assert!(rendered.contains("Permission mode set to review_all"));
        assert!(!root.join("gated.py").exists());
        assert!(rendered.contains("Permission mode set to auto_create_review_modify"));
        assert!(!root.join("allowed.py").exists());
        assert!(rendered.contains("stub provider response"));
        assert!(!rendered.contains("Status: waiting for approval"));
        assert!(!rendered.contains("Wrote "));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_plain_approval_words_go_to_model_path() {
        let root = temp_root("plain-approval-command");
        let target = root.join("hello.py");

        let rendered =
            render_tui_script(["create file hello.py", "approve", "reject"], &root, &root);

        assert!(!target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains("> approve"));
        assert!(rendered.contains("> reject"));
        assert!(rendered.contains("stub provider response"));
        assert_eq!(rendered.matches("No proposed action is waiting").count(), 0);
        assert!(rendered.contains("No live provider call was made"));
        assert!(!rendered.contains("Status: applied and verified"));
        assert!(!rendered.contains("Rejected. Nothing was changed."));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_no_pending_approval_gets_controller_feedback() {
        let rendered = render_tui_script(["/approve", "/reject"], ".", ".");

        assert!(rendered.contains("> /approve"));
        assert!(rendered.contains("No proposed action is waiting for approval."));
        assert!(rendered.contains("> /reject"));
        assert!(rendered.contains("No proposed action is waiting for rejection."));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_loop_reads_lines_and_exits_cleanly() {
        let input = b"what does the harness do?\n/quit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, ".", ".").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Elgar TUI. Type /exit, /quit, or /q to leave."));
        assert!(rendered.contains("> what does the harness do?"));
        assert!(!rendered.contains("stub-request-1"));
        assert!(rendered.contains("Exiting Elgar TUI."));
    }

    #[test]
    fn tui_loop_scripted_transcript_omits_provider_thinking() {
        let input = b"hello\n/exit\n";
        let mut output = Vec::new();

        run_tui_loop_with_runtime(
            &input[..],
            &mut output,
            ".",
            ".",
            AgentRuntime::new(ThinkingProvider),
            None,
            PermissionPolicyMode::AutoCreateReviewModify,
        )
        .unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("> hello"));
        assert!(rendered.contains("visible answer"));
        assert!(!rendered.contains("Internal reasoning should stay hidden."));
        assert!(rendered.contains("Exiting Elgar TUI."));
    }

    #[test]
    fn tui_loop_help_is_local_and_normal_text_uses_agent_runtime() {
        let input = b"/help\nwhat does the harness do?\n/exit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, ".", ".").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Commands\n/commands"));
        assert!(rendered.contains("/commands"));
        assert!(rendered.contains("> what does the harness do?"));
        assert!(!rendered.contains("stub-request-1"));
        assert!(rendered.contains("Exiting Elgar TUI."));
        assert!(!rendered.contains("> /help"));
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_loop_plain_text_and_approval_command_do_not_create_without_tool_result() {
        let root = temp_root("loop-plain-approve-command");
        let target = root.join("hello.py");
        let input = b"create file hello.py\n/approve\n/exit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, &root, &root).unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(!target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains("> /approve"));
        assert!(!rendered.contains("Creating hello.py."));
        assert!(rendered.contains("No proposed action is waiting for approval."));
        assert!(rendered.contains("stub provider response"));
        assert!(!rendered.contains("Status: applied and verified"));
        assert!(!rendered.contains(&format!("Result: Wrote {}.", target.display())));
        assert!(!rendered.contains("Action: action-1 CreateFile"));
        assert!(!rendered.contains("Target: hello.py"));
        assert!(rendered.contains("Exiting Elgar TUI."));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }
}
