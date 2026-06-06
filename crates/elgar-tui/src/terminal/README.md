# elgar-tui/src/terminal

## Purpose

Interactive terminal shell implementation for command handling, prompt input, provider turns, and terminal rendering.

## Important Files and Folders

- `inline.rs` prints the startup banner before the first prompt.
- `commands/` parses and executes local slash-command helpers.
- `display_context/` builds the live terminal display context for footer/startup/prompt rendering.
- `input/` reads keys, paste events, transcript cleanup, and terminal raw mode.
- `turn/` runs submitted prompts, active provider requests, and provider worker tasks.
- `ui/` owns prompt/footer/text rendering and Ratatui frame rendering.

## Ownership

Keep terminal interaction here, but route model behavior through core chat/provider APIs. UI drawing, input handling, command handling, and provider turns should stay in separate folders.

## Checks

- `cargo test -p elgar-tui --lib terminal`
- `cargo test -p elgar-tui`
