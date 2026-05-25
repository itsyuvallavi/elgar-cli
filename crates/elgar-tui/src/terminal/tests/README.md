# elgar-tui/src/terminal/tests

## Purpose

Focused tests for terminal commands, input, rendering frames, startup footer layout, memory commands, and model-first live flow rendering.

## Important Files and Folders

- `commands_and_input.rs`, `memory_commands.rs`, and `copy_clipboard.rs` cover terminal command behavior.
- `rendering_frames.rs` and `startup_footer_layout.rs` cover visual frame output.
- `model_first_live_flow/` covers provider task and loop routing behavior.

## Ownership

Prefer deterministic fixtures and explicit expected text. Do not require a live provider for terminal rendering tests.

## Checks

- `cargo test -p elgar-tui terminal`
- `cargo test -p elgar-tui model_first_live_flow`
