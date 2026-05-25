# elgar-core/src

## Purpose

Core module surface for agent runtime flow, actions, policy, providers, routing, sessions, filesystem helpers, shell helpers, legacy controller compatibility, and rendering.

## Important Files and Folders

- `agent_runtime.rs` is the normal chat entrypoint for live TUI and CLI script turns.
- `controller.rs` and `controller/` are legacy/review compatibility paths.
- `action.rs`, `policy.rs`, `fs.rs`, and `shell.rs` define permissioned work boundaries.
- `provider/` owns LM Studio and provider abstractions.
- `controller_tests/` keeps focused coverage for legacy controller behavior and regression guards.
- `lib.rs` exports the public core surface.

## Ownership

Keep modules narrow. If behavior is user-visible truth, it belongs in core rather than CLI or TUI.

## Checks

- `cargo test -p elgar-core`
- `cargo clippy -p elgar-core --all-targets -- -D warnings`
