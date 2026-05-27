use std::{fs, path::Path};

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

#[test]
fn slash_command_surfaces_do_not_reuse_natural_language_routing() {
    let root = repo_root();

    let agent_docs = fs::read_to_string(root.join("zz_elgar_agent_docs/AGENTS.md"))
        .expect("agent instructions should be readable");
    assert!(agent_docs.contains("No Natural-Language Trigger Tables"));
    assert!(agent_docs.contains("Normal user text belongs to the model path"));

    assert!(
        !root
            .join("crates/elgar-core/src/controller_state_answers.rs")
            .exists(),
        "state inspection must not be implemented as a natural-language phrase gate"
    );

    for relative_path in [
        "crates/elgar-cli/src/lib.rs",
        "crates/elgar-tui/src/terminal.rs",
    ] {
        let source =
            fs::read_to_string(root.join(relative_path)).expect("source should be readable");
        assert!(
            !source.contains("route_input("),
            "{relative_path} must keep local commands slash-only instead of routing normal words"
        );
        assert!(
            !source.contains("Route::ApproveAction") && !source.contains("Route::RejectAction"),
            "{relative_path} must not map ordinary approval words to local commands"
        );
    }
}

#[test]
fn active_runtime_does_not_reintroduce_phrase_trigger_helpers() {
    let root = repo_root();

    let active_sources = [
        (
            "crates/elgar-core/src/agent_loop.rs",
            &[
                "should_use_plain_chat_first",
                "tool_enabled_turn_required",
                "explicit_user_tool_intent",
                "starts_with_create_request",
                "repeated_plan_create_response",
                "read_existing_plan_response",
                "anchor_unrooted",
                "sanitize_plan",
                "repair_directory_only",
                "requested_project_base",
                "followup_base",
            ][..],
        ),
        (
            "crates/elgar-core/src/provider/stub.rs",
            &[
                "deterministic_stub",
                "contains_any",
                "extract_create_",
                "RawModelToolCall",
                "ModelToolName",
            ][..],
        ),
        (
            "crates/elgar-core/src/provider_visible.rs",
            &["contains(\"", "contains('"][..],
        ),
        (
            "crates/elgar-tui/src/reasoning.rs",
            &["contains(\"", "contains('", "strip_prefix"][..],
        ),
    ];

    for (relative_path, forbidden) in active_sources {
        let source =
            fs::read_to_string(root.join(relative_path)).expect("source should be readable");
        for token in forbidden {
            assert!(
                !source.contains(token),
                "{relative_path} must not reintroduce phrase trigger helper `{token}`"
            );
        }
    }
}

#[test]
fn legacy_controller_phrase_surfaces_stay_deleted() {
    let root = repo_root();

    for relative_path in [
        "crates/elgar-core/src/controller/legacy_controller_model_first.rs",
        "crates/elgar-core/src/controller_prompt.rs",
        "crates/elgar-core/src/controller_scaffold.rs",
        "crates/elgar-core/src/controller_shell_commands.rs",
        "crates/elgar-core/src/followup_action_paths.rs",
        "crates/elgar-core/src/legacy_controller_model_first_apply.rs",
        "crates/elgar-core/src/legacy_controller_model_first_continuation.rs",
        "crates/elgar-core/src/legacy_controller_model_first_decision.rs",
        "crates/elgar-core/src/legacy_controller_model_first_plan_completion.rs",
        "crates/elgar-core/src/controller_tests/model_first_policy.rs",
        "crates/elgar-core/src/controller_tests/model_first_project_plan.rs",
        "crates/elgar-core/src/controller_tests/provider_prompt_memory.rs",
    ] {
        assert!(
            !root.join(relative_path).exists(),
            "{relative_path} must stay deleted; normal words belong to the provider path"
        );
    }
}
