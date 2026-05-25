# elgar-core/src/controller_tests

## Purpose

Focused unit tests for legacy controller behavior, review paths, and provider-facing regression coverage.

## Important Files and Folders

- `basic_turns.rs` covers baseline controller turns.
- `action_lifecycle.rs` covers proposed, approved, rejected, and applied action behavior.
- `provider_prompt_memory.rs` and `provider_streaming_errors.rs` cover provider-facing edge cases.
- `model_first_policy/` covers model-first policy decisions.
- `model_first_project_plan/` covers model-first project planning behavior.

## Ownership

Add narrow tests here when legacy controller or review-path behavior changes. Prefer deterministic no-provider tests unless live provider behavior is the feature.

## Checks

- `cargo test -p elgar-core controller_tests`
- `cargo test -p elgar-core model_first`
