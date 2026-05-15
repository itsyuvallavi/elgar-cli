use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use elgar_core::{
    controller::Controller,
    provider::{
        chat_lm_studio, ChatMessage, ProviderConfig, ProviderError, LM_STUDIO_DEFAULT_BASE_URL,
    },
    renderer::render_session,
    session::Session,
};

pub const PROVIDER_SMOKE_COMMAND: &str = "provider-smoke";
pub const CONTROLLER_SMOKE_COMMAND: &str = "controller-smoke";
pub const TUI_CONTROLLER_SMOKE_COMMAND: &str = "tui-controller-smoke";
pub const TUI_COMMAND: &str = "tui";
pub const PROVIDER_SMOKE_DEFAULT_PROMPT: &str = "Say hello in one sentence.";
pub const LM_STUDIO_MODEL_ENV: &str = "ELGAR_LM_STUDIO_MODEL";
pub const LM_STUDIO_BASE_URL_ENV: &str = "ELGAR_LM_STUDIO_BASE_URL";

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

pub fn is_tui_exit_command(input: &str) -> bool {
    matches!(input.trim(), "/exit" | "/quit")
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
    } else {
        shell.submit_input(controller, session, input);
    }
}

pub fn render_tui_help() -> &'static str {
    "Elgar TUI commands:\n  /help      Show these commands.\n  /commands  Show these commands.\n  /approve   Approve the pending action.\n  /reject    Reject the pending action.\n  /exit      Exit the TUI.\n  /quit      Exit the TUI."
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

    writeln!(writer, "Elgar TUI. Type /exit or /quit to leave.")?;
    for line in reader.lines() {
        let input = line?;
        if is_tui_exit_command(&input) {
            writeln!(writer, "Exiting Elgar TUI.")?;
            break;
        }

        if is_tui_help_command(&input) {
            writeln!(writer, "{}", render_tui_help())?;
        } else {
            submit_tui_input(&mut shell, &controller, &mut session, &input);
            writeln!(writer, "{}", shell.render())?;
        }
    }

    Ok(())
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
        is_tui_approval_command, is_tui_exit_command, is_tui_help_command,
        is_tui_rejection_command, provider_smoke_config, provider_smoke_prompt,
        render_controller_smoke, render_tui_controller_smoke, render_tui_help, render_tui_script,
        run_tui_loop, ProviderSmokeConfig, ProviderSmokeError, PROVIDER_SMOKE_DEFAULT_PROMPT,
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
        assert!(rendered.contains("provider started: lm-studio request lm-studio-request-1"));
        assert!(rendered.contains("error: lm-studio provider request lm-studio-request-1 failed"));
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

        assert!(rendered.contains("You: Say hello in one sentence."));
        assert!(rendered.contains("Thinking with lm-studio..."));
        assert!(rendered.contains(
            "Provider error from lm-studio: Configuration provider error: only http:// provider URLs are supported"
        ));
        assert!(rendered.contains("[Status]\nprovider error"));
        assert!(!rendered.contains("stub-provider"));
    }

    #[test]
    fn tui_exit_commands_are_explicit() {
        assert!(is_tui_exit_command("/exit"));
        assert!(is_tui_exit_command(" /quit "));
        assert!(!is_tui_exit_command("exit"));
        assert!(!is_tui_exit_command("/help"));
    }

    #[test]
    fn tui_help_lists_only_supported_local_commands() {
        let help = render_tui_help();

        assert!(is_tui_help_command("/help"));
        assert!(is_tui_help_command(" /commands "));
        assert!(!is_tui_help_command("help"));
        assert!(!is_tui_help_command("/model"));
        assert!(help.contains("/help"));
        assert!(help.contains("/commands"));
        assert!(help.contains("/approve"));
        assert!(help.contains("/reject"));
        assert!(help.contains("/exit"));
        assert!(help.contains("/quit"));
        assert!(!help.contains("/model"));
        assert!(!help.contains("/settings"));
        assert!(!help.contains("/login"));
        assert!(!help.contains("/provider"));
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
    fn tui_script_renders_default_stub_turns_and_stops_on_exit() {
        let rendered = render_tui_script(
            ["what does the harness do?", "/exit", "what should not run?"],
            ".",
            ".",
        );

        assert!(rendered.contains("You: what does the harness do?"));
        assert!(rendered.contains("Thinking with stub-provider..."));
        assert!(rendered.contains("Assistant: stub provider response"));
        assert!(!rendered.contains("what should not run?"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_help_is_local_and_does_not_call_controller_or_provider() {
        let rendered = render_tui_script(["/help", "/commands"], ".", ".");

        assert!(rendered.contains("Elgar TUI commands:"));
        assert!(rendered.contains("/approve"));
        assert!(rendered.contains("/reject"));
        assert!(!rendered.contains("You: /help"));
        assert!(!rendered.contains("You: /commands"));
        assert!(!rendered.contains("Input was not recognized"));
        assert!(!rendered.contains("stub-provider"));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_script_approval_command_applies_pending_action() {
        let root = temp_root("approve-command");
        let target = root.join("hello.py");

        let rendered = render_tui_script(["create file hello.py", "/approve"], &root, &root);

        assert!(target.exists());
        assert!(rendered.contains("You: create file hello.py"));
        assert!(rendered.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("You: approve"));
        assert!(rendered.contains("Approved: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("Applied and verified: action-1 WriteFile"));
        assert!(rendered.contains("hello.py was written"));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_rejection_command_rejects_pending_action_without_writing() {
        let root = temp_root("reject-command");
        let target = root.join("hello.py");

        let rendered = render_tui_script(["create file hello.py", "/reject"], &root, &root);

        assert!(!target.exists());
        assert!(rendered.contains("You: create file hello.py"));
        assert!(rendered.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("You: reject"));
        assert!(
            rendered.contains("Rejected: action-1 WriteFile write hello.py. No file was changed.")
        );
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tui_script_no_pending_approval_gets_controller_feedback() {
        let rendered = render_tui_script(["/approve", "/reject"], ".", ".");

        assert!(rendered.contains("You: approve"));
        assert!(rendered.contains("No proposed action is waiting for approval."));
        assert!(rendered.contains("You: reject"));
        assert!(rendered.contains("No proposed action is waiting for rejection."));
        assert!(!rendered.contains("lm-studio"));
    }

    #[test]
    fn tui_loop_reads_lines_and_exits_cleanly() {
        let input = b"what does the harness do?\n/quit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, ".", ".").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Elgar TUI. Type /exit or /quit to leave."));
        assert!(rendered.contains("You: what does the harness do?"));
        assert!(rendered.contains("Thinking with stub-provider..."));
        assert!(rendered.contains("Exiting Elgar TUI."));
    }

    #[test]
    fn tui_loop_help_is_local_and_normal_text_still_uses_controller() {
        let input = b"/help\nwhat does the harness do?\n/exit\n";
        let mut output = Vec::new();

        run_tui_loop(&input[..], &mut output, ".", ".").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Elgar TUI commands:"));
        assert!(rendered.contains("/commands"));
        assert!(rendered.contains("You: what does the harness do?"));
        assert!(rendered.contains("Thinking with stub-provider..."));
        assert!(rendered.contains("Exiting Elgar TUI."));
        assert!(!rendered.contains("You: /help"));
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
        assert!(rendered.contains("You: create file hello.py"));
        assert!(rendered.contains("You: approve"));
        assert!(rendered.contains("Applied and verified: action-1 WriteFile"));
        assert!(rendered.contains("hello.py was written"));
        assert!(rendered.contains("Exiting Elgar TUI."));
        assert!(!rendered.contains("lm-studio"));

        let _ = fs::remove_dir_all(root);
    }
}
