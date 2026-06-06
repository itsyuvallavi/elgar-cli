# elgar-cli/src

## Purpose

Implementation files for CLI command dispatch, startup, diagnostics, and tests.

## Files and Folders

- `main.rs` is the binary entrypoint for the `elgar` command.
- `lib.rs` re-exports CLI helper modules and owns the simple single-turn CLI render path.
- `startup/` holds the real launch path: path resolution, provider config, and TUI startup.
- `diagnostics/` holds support commands: provider smoke and scripted TUI.
- `tests/` holds focused unit tests for the active CLI helper modules.
- `_legacy/` holds archived CLI code that is not part of the active raw-chat path.

## Ownership

Keep argument parsing, config lookup, and process IO here. Model/provider behavior belongs in `elgar-core`; terminal rendering belongs in `elgar-tui`.

## Checks

- `cargo check -p elgar-cli`
- `cargo test -p elgar-cli`
- `cargo test -p elgar-cli --test smoke`
