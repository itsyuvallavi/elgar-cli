use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

use elgar_core::{
    controller::Controller,
    provider::{
        chat_lm_studio, ChatMessage, ProviderConfig, ProviderError, LM_STUDIO_DEFAULT_BASE_URL,
    },
    renderer::render_session,
    router::{route_input, Route},
    session::Session,
};
use serde::Deserialize;

pub mod perf;

pub const PROVIDER_SMOKE_COMMAND: &str = "provider-smoke";
pub const CONTROLLER_SMOKE_COMMAND: &str = "controller-smoke";
pub const TUI_CONTROLLER_SMOKE_COMMAND: &str = "tui-controller-smoke";
pub const TUI_COMMAND: &str = "tui";
pub const TUI_TERMINAL_COMMAND: &str = "tui-terminal";
pub const PERF_BASELINE_COMMAND: &str = "perf-baseline";
pub const PROVIDER_SMOKE_DEFAULT_PROMPT: &str = "Say hello in one sentence.";
pub const LM_STUDIO_MODEL_ENV: &str = "ELGAR_LM_STUDIO_MODEL";
pub const LM_STUDIO_BASE_URL_ENV: &str = "ELGAR_LM_STUDIO_BASE_URL";
pub const PROVIDER_CONFIG_ENV: &str = "ELGAR_PROVIDER_CONFIG";
pub const PROVIDER_CONFIG_FILE: &str = "elgar-provider.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProvider {
    pub config: ProviderConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeProviderConfigError {
    InvalidEnvironment { name: &'static str },
    ReadFailed { path: PathBuf, message: String },
    ParseFailed { path: PathBuf, message: String },
    UnsupportedProvider { provider: String },
    MissingModel { path: PathBuf },
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
}

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

pub fn render_cli_turn_from_runtime_config(
    input: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, RuntimeProviderConfigError> {
    let cwd_ref = cwd.as_ref();
    let Some(runtime) = load_runtime_provider(cwd_ref)? else {
        return Ok(render_cli_turn(input, project_root, cwd_ref));
    };

    let controller = Controller::with_lm_studio_provider(runtime.config);
    let mut session = Session::new("cli-runtime-session", project_root.as_ref(), cwd_ref);

    controller.turn(&mut session, input);
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

pub fn is_tui_clear_command(input: &str) -> bool {
    matches!(input.trim(), "/clear" | "/new")
}

fn submit_tui_input(
    shell: &mut elgar_tui::TuiShell,
    controller: &Controller,
    session: &mut Session,
    input: &str,
) {
    if is_tui_approval_command(input) {
        shell.submit_approval(controller, session);
    } else if is_tui_rejection_command(input) {
        shell.submit_rejection(controller, session);
    } else if matches!(
        route_input(input),
        Route::ApproveAction | Route::RejectAction
    ) {
        shell.push_local_message("Action commands must use /approve or /reject.");
    } else {
        shell.submit_input(controller, session, input);
    }
}

pub fn render_tui_help() -> &'static str {
    "Commands\n/commands  Show commands\n/clear     Clear the visible conversation\n/new       Clear the visible conversation\n/approve   Apply the pending action\n/reject    Reject the pending action\n/copy      Copy the conversation\n/exit      Quit\n/quit      Quit\n/q         Quit\n/help      Show commands"
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
    let controller = Controller::default();
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::new();
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
            rendered_turns.push(shell.render());
        } else if is_tui_copy_command(input) {
            rendered_turns.push(shell.conversation_copy_text());
        } else {
            submit_tui_input(&mut shell, &controller, &mut session, input);
            rendered_turns.push(shell.render());
        }
    }

    rendered_turns.join("\n")
}

pub fn run_tui_loop<R, W>(
    reader: R,
    mut writer: W,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let controller = Controller::default();
    let mut session = Session::new("cli-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = elgar_tui::TuiShell::new();

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
            writeln!(writer, "{}", shell.render())?;
        } else if is_tui_copy_command(&input) {
            writeln!(writer, "{}", shell.conversation_copy_text())?;
        } else {
            submit_tui_input(&mut shell, &controller, &mut session, &input);
            writeln!(writer, "{}", shell.render())?;
        }
    }

    Ok(())
}

pub fn run_tui_terminal() -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match load_runtime_provider(&cwd) {
        Ok(Some(runtime)) => elgar_tui::run_terminal_shell_with_lm_studio_provider(runtime.config),
        Ok(None) => elgar_tui::run_terminal_shell(),
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

pub fn render_controller_smoke_from_env(
    prompt: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, ProviderSmokeError> {
    let config = provider_smoke_config_from_env(prompt)?;
    Ok(render_controller_smoke(config, project_root, cwd))
}

pub fn render_controller_smoke(
    config: ProviderSmokeConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let provider_config = provider_config_from_smoke_config(&config);
    let controller = Controller::with_lm_studio_provider(provider_config);
    let mut session = Session::new(
        "cli-controller-smoke-session",
        project_root.as_ref(),
        cwd.as_ref(),
    );

    controller.turn(&mut session, &config.prompt);
    render_session(&session)
}

pub fn render_tui_controller_smoke_from_env(
    prompt: &str,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> Result<String, ProviderSmokeError> {
    let config = provider_smoke_config_from_env(prompt)?;
    Ok(render_tui_controller_smoke(config, project_root, cwd))
}

pub fn render_tui_controller_smoke(
    config: ProviderSmokeConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    let provider_config = provider_config_from_smoke_config(&config);
    elgar_tui::run_lm_studio_controller_smoke(provider_config, &config.prompt, project_root, cwd)
        .rendered
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
    use elgar_core::provider::LM_STUDIO_DEFAULT_BASE_URL;
    use std::{fs, path::PathBuf};

    use super::{
        is_tui_approval_command, is_tui_clear_command, is_tui_copy_command, is_tui_exit_command,
        is_tui_help_command, is_tui_rejection_command, load_runtime_provider,
        provider_smoke_config, provider_smoke_prompt, render_cli_turn_from_runtime_config,
        render_controller_smoke, render_tui_controller_smoke, render_tui_help, render_tui_script,
        run_tui_loop, ProviderSmokeConfig, ProviderSmokeError, RuntimeProviderConfigError,
        PROVIDER_CONFIG_FILE, PROVIDER_SMOKE_DEFAULT_PROMPT, TUI_COMMAND, TUI_TERMINAL_COMMAND,
    };

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
        assert!(runtime.config.stream);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_provider_config_absent_keeps_stub_fallback() {
        let root = temp_root("runtime-provider-absent");

        assert_eq!(load_runtime_provider(&root).unwrap(), None);

        let rendered = render_cli_turn_from_runtime_config("hello", &root, &root).unwrap();
        assert!(rendered.contains("provider started: stub-provider"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
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
    fn controller_smoke_renders_live_provider_error_event_without_network() {
        let rendered = render_controller_smoke(
            ProviderSmokeConfig {
                model: "local-model".to_string(),
                base_url: "https://127.0.0.1:1234/v1".to_string(),
                prompt: "Say hello in one sentence.".to_string(),
            },
            ".",
            ".",
        );

        assert!(rendered.contains("user: Say hello in one sentence."));
        assert!(rendered.contains("provider started: lm-studio request lm-studio-request-"));
        assert!(rendered.contains("error: lm-studio provider request lm-studio-request-"));
        assert!(rendered.contains("failed"));
        assert!(rendered.contains("only http:// provider URLs are supported"));
        assert!(!rendered.contains("action proposed"));
        assert!(!rendered.contains("action applied"));
    }

    #[test]
    fn tui_controller_smoke_renders_live_provider_error_with_tui_copy_without_network() {
        let rendered = render_tui_controller_smoke(
            ProviderSmokeConfig {
                model: "local-model".to_string(),
                base_url: "https://127.0.0.1:1234/v1".to_string(),
                prompt: "Say hello in one sentence.".to_string(),
            },
            ".",
            ".",
        );

        assert!(rendered.contains("> Say hello in one sentence."));
        assert!(!rendered.contains("lm-studio-request-1"));
        assert!(rendered.contains(
            "Provider error from lm-studio: Configuration provider error: only http:// provider URLs are supported"
        ));
        assert!(rendered.contains("Status\nprovider error"));
        assert!(!rendered.contains("stub-provider"));
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
        assert!(help.contains("/approve"));
        assert!(help.contains("/reject"));
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
        assert!(rendered.contains("/copy"));
        assert!(!rendered.contains("> /help"));
        assert!(!rendered.contains("> /commands"));
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("stub-provider"));
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
    fn tui_script_approval_command_applies_pending_action() {
        let root = temp_root("approve-command");
        let target = root.join("hello.py");

        let rendered = render_tui_script(["create file hello.py", "/approve"], &root, &root);

        assert!(target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains("Review needed: action-1 CreateFile write hello.py"));
        assert!(rendered.contains("> approve"));
        assert!(rendered.contains("Approved: action-1 CreateFile write hello.py"));
        assert!(rendered.contains("Applied and verified: action-1 CreateFile"));
        assert!(rendered.contains("hello.py was written"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_plain_approval_words_do_not_apply_pending_actions() {
        let root = temp_root("plain-approval-command");
        let target = root.join("hello.py");

        let rendered =
            render_tui_script(["create file hello.py", "approve", "reject"], &root, &root);

        assert!(!target.exists());
        assert!(rendered.contains("Action commands must use /approve or /reject."));
        assert!(!rendered.contains("Applied and verified"));
        assert!(!rendered.contains("Rejected: action-1"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_rejection_command_rejects_pending_action_without_writing() {
        let root = temp_root("reject-command");
        let target = root.join("hello.py");

        let rendered = render_tui_script(["create file hello.py", "/reject"], &root, &root);

        assert!(!target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains("Review needed: action-1 CreateFile write hello.py"));
        assert!(rendered.contains("> reject"));
        assert!(
            rendered.contains("Rejected: action-1 CreateFile write hello.py. No file was changed.")
        );
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_no_pending_approval_gets_controller_feedback() {
        let rendered = render_tui_script(["/approve", "/reject"], ".", ".");

        assert!(rendered.contains("> approve"));
        assert!(rendered.contains("No proposed action is waiting for approval."));
        assert!(rendered.contains("> reject"));
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
    fn tui_loop_help_is_local_and_normal_text_still_uses_controller() {
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
    fn tui_loop_routes_approval_command_through_controller() {
        let root = temp_root("loop-approve-command");
        let target = root.join("hello.py");
        let input = b"create file hello.py\n/approve\n/exit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, &root, &root).unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(target.exists());
        assert!(rendered.contains("> create file hello.py"));
        assert!(rendered.contains("> approve"));
        assert!(rendered.contains("Applied and verified: action-1 CreateFile"));
        assert!(rendered.contains("hello.py was written"));
        assert!(rendered.contains("Exiting Elgar TUI."));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }
}
