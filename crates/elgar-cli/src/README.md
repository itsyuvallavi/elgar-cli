# elgar-cli/src

## Purpose

Implementation files for CLI command dispatch, runtime config resolution, and performance reporting.

## Important Files

- `main.rs` handles argument routing and process exit behavior.
- `lib.rs` exposes testable CLI helpers and runtime provider configuration.
- `perf.rs` renders deterministic performance baseline output.

## Ownership

Keep argument parsing and IO here. Do not let CLI code mutate files directly or bypass core policy.

## Checks

- `cargo test -p elgar-cli`
- `cargo test -p elgar-cli --test smoke`
