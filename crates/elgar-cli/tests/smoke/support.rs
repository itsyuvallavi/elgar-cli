//! Shared helpers for CLI smoke tests.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use elgar_cli::render_cli_turn;

pub(super) fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub(super) fn render(input: &str, root: &Path) -> String {
    render_cli_turn(input, root, root)
}

pub(super) fn elgar_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_elgar"))
}

pub(super) fn scripted_tui_command() -> Command {
    let mut command = elgar_command();
    command
        .arg("tui")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub(super) fn force_stub_env(command: &mut Command) -> &mut Command {
    command
        .env("ELGAR_PROVIDER_CONFIG", "off")
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
}

pub(super) fn write_live_provider_config(root: &Path) {
    fs::write(
        root.join("elgar-provider.json"),
        r#"{
          "provider": "lm-studio",
          "base_url": "https://127.0.0.1:1234/v1",
          "default_model": "openai/gpt-oss-20b",
          "mode": "live"
        }"#,
    )
    .unwrap();
}
