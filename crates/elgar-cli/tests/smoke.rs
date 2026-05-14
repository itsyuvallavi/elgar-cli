use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_cli::render_cli_turn;

fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn render(input: &str, root: &Path) -> String {
    render_cli_turn(input, root, root)
}

#[test]
fn cli_renders_core_events_from_controller_output() {
    let root = smoke_root("core-events");

    let rendered = render("what does the harness do?", &root);

    assert!(rendered.contains("user: what does the harness do?"));
    assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
    assert!(rendered.contains("provider finished: stub-provider request stub-request-1"));
    assert!(rendered.contains("assistant Provider: stub provider response"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn cli_reports_controller_proposal_without_mutating_files() {
    let root = smoke_root("proposal-no-write");
    let target = root.join("hello.py");

    let rendered = render("create file hello.py", &root);

    assert!(rendered.contains("user: create file hello.py"));
    assert!(rendered.contains("action proposed: action-1 WriteFile write hello.py"));
    assert!(rendered.contains(
        "assistant Controller: Proposed WriteFile action. Approve or reject before any file is written."
    ));
    assert!(!target.exists());

    let _ = fs::remove_dir_all(root);
}
