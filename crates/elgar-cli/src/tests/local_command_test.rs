//! Direct CLI local command routing tests.

use std::fs;

use crate::{render_cli_local_command, render_cli_turn, render_cli_turn_from_runtime_config};

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-local-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn direct_cli_raw_slash_command_is_local_unknown_command() {
    let output = render_cli_local_command("/raw hello").unwrap();

    assert!(output.contains("Unknown command: /raw hello"));
    assert!(output.contains("Plain text without / is sent to the model."));
}

#[test]
fn direct_cli_slash_command_does_not_call_stub_provider() {
    let root = temp_root("stub-raw");

    let output = render_cli_turn("/raw hello", &root, &root);

    assert!(output.contains("Unknown command: /raw hello"));
    assert!(!output.contains("provider started"));
    assert!(!output.contains("stub provider response"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_cli_slash_command_does_not_call_runtime_provider() {
    let root = temp_root("runtime-raw");
    fs::write(
        root.join("elgar-provider.json"),
        r#"{
          "provider": "lm-studio",
          "base_url": "https://127.0.0.1:1234/v1",
          "default_model": "test-model",
          "mode": "live"
        }"#,
    )
    .unwrap();

    let output = render_cli_turn_from_runtime_config("/raw hello", &root, &root).unwrap();

    assert!(output.contains("Unknown command: /raw hello"));
    assert!(!output.contains("lm-studio"));
    assert!(!output.contains("only http:// provider URLs are supported"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_cli_quoted_logs_latest_is_local_diagnostic() {
    let root = temp_root("runtime-logs-latest");
    fs::write(
        root.join("elgar-provider.json"),
        r#"{
          "provider": "lm-studio",
          "base_url": "https://127.0.0.1:1234/v1",
          "default_model": "test-model",
          "mode": "live"
        }"#,
    )
    .unwrap();

    let output = render_cli_turn_from_runtime_config("logs latest", &root, &root).unwrap();

    assert!(
        output.contains("system log directory does not exist")
            || output.contains("no system log files found")
            || output.contains("no turn_perf_summary found")
    );
    assert!(!output.contains("only http:// provider URLs are supported"));
    assert!(!output.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_cli_logs_latest_phrase_still_goes_to_model() {
    let root = temp_root("logs-latest-phrase");

    let output = render_cli_turn("please explain logs latest", &root, &root);

    assert!(output.contains("user: please explain logs latest"));
    assert!(output.contains("provider started"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_cli_plain_raw_word_still_goes_to_model() {
    let root = temp_root("plain-raw");

    let output = render_cli_turn("raw hello", &root, &root);

    assert!(output.contains("user: raw hello"));
    assert!(output.contains("provider started"));

    let _ = fs::remove_dir_all(root);
}
