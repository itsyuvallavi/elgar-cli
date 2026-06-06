# _legacy/tests

## Purpose

Archived tests from the old tool/action terminal harness.

These files are not part of the active raw-only TUI test suite. They are kept
only as reference while rebuilding features slowly.

## Important Files and Folders

- `commands_and_input.rs`, `memory_commands.rs`, and `copy_clipboard.rs` cover old terminal command behavior.
- `rendering_frames.rs` and `startup_footer_layout.rs` cover old visual frame output.
- `provider_live_flow/` covers old provider task and loop routing behavior.

## Ownership

Do not add new active tests here. New tests should live beside the active module they cover.

## Checks

- Active tests: `cargo test -p elgar-tui`
