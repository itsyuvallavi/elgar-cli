# crates

## Purpose

Rust workspace crates for the active Elgar v0.2 implementation.

## Important Folders

- `elgar-core` owns agent runtime flow, the action gate, routing, provider boundaries, actions, sessions, and rendering.
- `elgar-cli` owns command parsing and runtime configuration glue.
- `elgar-tui` owns the terminal UI shell and rendering surfaces.

## Ownership

Keep behavior in the narrowest crate that owns it. UI and CLI code should call core instead of owning runtime or permission policy.

## Checks

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `./bin/check-local`
