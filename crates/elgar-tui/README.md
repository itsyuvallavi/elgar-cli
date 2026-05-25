# elgar-tui

## Purpose

Terminal UI crate for rendering Elgar sessions, approvals, provider activity, and shell interaction.

## Important Files and Folders

- `src` contains TUI layout, panes, terminal loop, shell, theme, and smoke helpers.
- `tests/smoke.rs` covers crate-level TUI smoke behavior.

## Ownership

TUI reports core state. It should not own routing, permissions, provider policy, or filesystem mutation rules.

## Checks

- `cargo test -p elgar-tui`
- `cargo run -p elgar-cli -- tui-terminal`
