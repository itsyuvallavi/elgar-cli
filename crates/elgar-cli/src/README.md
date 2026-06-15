# elgar-cli/src

## Purpose

Implementation files for CLI command dispatch, startup, diagnostics, and tests.

## Files and Folders

- `main.rs` is the binary entrypoint for the `elgar` command.
- `lib.rs` re-exports CLI helper modules and owns the simple single-turn CLI render path.
- `startup/` holds the real launch path: path resolution, provider config, and TUI startup.
- `diagnostics/` holds support commands: provider smoke, log viewing, and scripted TUI.
- `tests/` holds focused unit tests for the active CLI helper modules.
- `_legacy/` holds archived CLI code that is not part of the active harness path.

## Ownership

Keep argument parsing, config lookup, and process IO here. Model/provider behavior belongs in `elgar-core`; terminal rendering belongs in `elgar-tui`.

## Active Commands

- `elgar` launches the interactive terminal TUI when stdin/stdout are terminals.
- `elgar tui-terminal` explicitly launches the interactive terminal TUI.
- `elgar tui` runs the scripted TUI for tests/scripts. It is line-based by
  default and supports `/prompt` ... `/end` blocks for one multiline prompt.
- `elgar provider-smoke` sends one direct provider smoke request.
- `elgar logs latest` prints the latest system-log turn summary.

## Checks

- `cargo check -p elgar-cli`
- `cargo test -p elgar-cli`
- `cargo test -p elgar-cli --test smoke`
