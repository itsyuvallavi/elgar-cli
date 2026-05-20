use std::fs;

use elgar_core::context::{ContextAccounting, ContextBundle};

#[test]
fn unknown_context_has_no_fake_counts_or_window() {
    let context = ContextAccounting::unknown();

    assert!(context.loaded_files.is_empty());
    assert!(context.omitted_files.is_empty());
    assert_eq!(context.estimated_tokens, None);
    assert_eq!(context.max_window_tokens, None);
}

#[test]
fn default_local_context_tracks_real_files_and_estimated_tokens() {
    let root = temp_root("context-accounting");
    fs::write(root.join("AGENTS.md"), "12345678").unwrap();

    let context = ContextAccounting::from_default_local_files(&root, &root, Some(128_000));

    assert_eq!(context.loaded_files.len(), 1);
    assert_eq!(context.loaded_files[0].display_path, "AGENTS.md");
    assert_eq!(context.loaded_files[0].bytes, 8);
    assert_eq!(context.loaded_files[0].estimated_tokens, 2);
    assert!(!context.loaded_files[0].truncated);
    assert!(context.omitted_files.is_empty());
    assert_eq!(context.estimated_tokens, Some(2));
    assert_eq!(context.max_window_tokens, Some(128_000));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_local_context_keeps_usage_unknown() {
    let root = temp_root("context-accounting-empty");

    let context = ContextAccounting::from_default_local_files(&root, &root, Some(128_000));

    assert!(context.loaded_files.is_empty());
    assert!(context.omitted_files.is_empty());
    assert_eq!(context.estimated_tokens, None);
    assert_eq!(context.max_window_tokens, Some(128_000));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn context_bundle_formats_selected_context_before_user_request() {
    let root = temp_root("context-bundle-prompt");
    fs::write(root.join("AGENTS.md"), "Keep replies short.").unwrap();

    let bundle = ContextBundle::from_default_local_files(&root, &root, None);
    let prompt = bundle.prompt_for("what can you do?");

    assert!(prompt.contains("Local context selected by Elgar controller:"));
    assert!(prompt.contains("--- AGENTS.md ---\nKeep replies short."));
    assert!(prompt.contains("User request:\nwhat can you do?"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn context_bundle_trims_large_file_to_budget() {
    let root = temp_root("context-bundle-trim");
    fs::write(root.join("AGENTS.md"), "a".repeat(128)).unwrap();

    let bundle =
        ContextBundle::from_local_files_with_budget(&root, &root, ["AGENTS.md"], Some(128_000), 16);

    assert_eq!(bundle.accounting.loaded_files.len(), 1);
    assert_eq!(bundle.accounting.loaded_files[0].bytes, 64);
    assert_eq!(bundle.accounting.loaded_files[0].estimated_tokens, 16);
    assert!(bundle.accounting.loaded_files[0].truncated);
    assert_eq!(bundle.accounting.estimated_tokens, Some(16));
    assert!(bundle.prompt_for("go").contains("AGENTS.md (truncated)"));
    assert!(!bundle.prompt_for("go").contains(&"a".repeat(80)));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn context_bundle_omits_when_remaining_budget_is_too_small() {
    let root = temp_root("context-bundle-omit");
    fs::write(root.join("AGENTS.md"), "12345678").unwrap();
    fs::write(root.join("elgar-provider.json"), "abcdefghijklmnop").unwrap();

    let bundle = ContextBundle::from_local_files_with_budget(
        &root,
        &root,
        ["AGENTS.md", "elgar-provider.json"],
        None,
        3,
    );

    assert_eq!(bundle.accounting.loaded_files.len(), 1);
    assert_eq!(bundle.accounting.loaded_files[0].display_path, "AGENTS.md");
    assert_eq!(bundle.accounting.omitted_files.len(), 1);
    assert_eq!(
        bundle.accounting.omitted_files[0].display_path,
        "elgar-provider.json"
    );
    assert_eq!(
        bundle.accounting.omitted_files[0].reason,
        "context budget exceeded"
    );

    let _ = fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}
