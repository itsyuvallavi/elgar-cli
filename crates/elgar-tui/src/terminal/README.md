# elgar-tui/src/terminal

## Purpose

Interactive terminal shell implementation for command handling, prompts, footer layout, provider tasks, and text rendering.

## Important Files and Folders

- `commands.rs` parses terminal commands.
- `prompt.rs`, `footer.rs`, and `text.rs` render terminal UI text.
- `provider_task.rs` handles provider task display.
- `tests.rs` wires terminal module tests.
- `tests/` contains focused terminal behavior tests.

## Ownership

Keep terminal interaction here, but route agent behavior through core runtime APIs. Clipboard, input, and display details should stay isolated.

## Checks

- `cargo test -p elgar-tui terminal`
- `cargo test -p elgar-tui terminal::tests`
