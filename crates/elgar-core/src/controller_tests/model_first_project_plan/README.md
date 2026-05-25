# model_first_project_plan

## Purpose

Regression tests for model-first project planning, desktop path handling, and verified plan execution around legacy controller paths.

## Important Files

- `desktop_paths.rs` covers desktop and path-sensitive requests.
- `project_requests.rs` covers project creation and planning requests.
- `verified_plan_execution.rs` covers plan execution after verification.

## Ownership

Use these tests for planner behavior, not provider formatting. Keep filesystem expectations explicit.

## Checks

- `cargo test -p elgar-core model_first_project_plan`
- `cargo test -p elgar-core model_first`
