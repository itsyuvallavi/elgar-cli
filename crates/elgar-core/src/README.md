# elgar-core/src

## Purpose

Core module surface for the harness, providers,
sessions/events, token accounting, context helpers, rendering, and local logs.

## Important Files and Folders

- `harness/` owns the single model route, primitive loop, permissions, and
  approval execution.
- `provider/` owns LM Studio and provider abstractions.
- `logs/` owns local JSONL session/system logging.
- `session.rs` stores in-memory session events and token/accounting snapshots.
- `session/` contains session id rotation and session event metadata helpers.
- `event/` defines core event and provider output types.
- `token_accounting.rs` tracks provider-reported usage and context-window snapshots.
- `context/` owns bounded context helper types for future use.
- `renderer.rs` renders core events for simple non-TUI output.
- `lib.rs` exports the public core surface.

## Ownership

Keep modules narrow. If behavior is user-visible truth, it belongs in core rather than CLI or TUI.

## Checks

- `cargo test -p elgar-core`
- `cargo clippy -p elgar-core --all-targets -- -D warnings`
