# elgar-cli

## Purpose

Command-line entrypoint for Elgar. It resolves runtime configuration, dispatches smoke commands, and launches the TUI when appropriate.

## Important Files

- `src/main.rs` parses CLI arguments and exits with user-facing output.
- `src/lib.rs` owns runtime config loading and command helpers.
- `src/perf.rs` owns local performance baseline reporting.
- `tests/smoke.rs` covers CLI smoke behavior.

## Ownership

Keep this crate thin. Put runtime behavior in `elgar-core` and TUI rendering in `elgar-tui`.

## Checks

- `cargo test -p elgar-cli`
- `cargo run -p elgar-cli -- provider-smoke "Say hello in one sentence."`
