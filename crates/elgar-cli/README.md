# elgar-cli

## Purpose

Command-line entrypoint for Elgar. It resolves runtime configuration, dispatches smoke commands, and launches the TUI when appropriate.

## Files and Folders

- `src/main.rs` parses CLI arguments and exits with user-facing output.
- `src/lib.rs` re-exports CLI helpers and owns the simple single-turn CLI render path.
- `src/startup/` owns the real app launch path.
- `src/diagnostics/` owns provider smoke and scripted TUI support commands.
- `src/tests/` holds active CLI unit tests split out of `src/lib.rs`.
- `src/_legacy/` contains archived CLI code that is not active.
- `tests/smoke.rs` covers CLI smoke behavior.

## Ownership

Keep this crate thin. Put runtime behavior in `elgar-core` and TUI rendering in `elgar-tui`.

## Checks

- `cargo test -p elgar-cli`
- `cargo run -p elgar-cli -- provider-smoke "Say hello in one sentence."`
