//! Tests for the TUI startup block.

use std::fs;

use super::super::StartupBlock;

#[test]
fn startup_block_lists_only_real_context_files_and_provider() {
    let root = temp_root("startup-context");
    fs::write(root.join("AGENTS.md"), "agent instructions").unwrap();

    let block = StartupBlock::new(
        &root,
        &root,
        Some("lm-studio".to_string()),
        Some("openai/gpt-oss-20b".to_string()),
    );

    let rendered = block.render();

    assert_eq!(
        rendered,
        "elgar v0.10\n/commands · /clear · /copy · /exit\n\nElgar uses your local LM Studio model.\n\n[Context]\n  AGENTS.md\n\n[Provider]\n  lm-studio · openai/gpt-oss-20b"
    );
    assert!(!rendered.contains("elgar-provider.json"));
    assert!(!rendered.contains("Commands:"));
    assert!(!rendered.contains("Skills"));
    assert!(!rendered.contains("MCP"));
    assert!(!rendered.contains("Bash"));
    assert!(!rendered.contains("API"));
    assert!(!rendered.contains("settings"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_block_uses_none_for_missing_context_provider_and_model() {
    let root = temp_root("startup-empty");

    let rendered = StartupBlock::new(&root, &root, None, None).render();

    assert!(!rendered.contains("local LM Studio model"));
    assert!(rendered.contains("[Context]\n  (none)"));
    assert!(rendered.contains("[Provider]\n  none · none"));
    assert!(!rendered.contains("[Policy]"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_block_does_not_claim_lm_studio_for_stub_provider() {
    let root = temp_root("startup-stub");

    let rendered =
        StartupBlock::new(&root, &root, Some("stub-provider".to_string()), None).render();

    assert!(!rendered.contains("local LM Studio model"));
    assert!(rendered.contains("[Provider]\n  stub-provider · none"));
    assert!(!rendered.contains("[Policy]"));

    let _ = fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-startup-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}
