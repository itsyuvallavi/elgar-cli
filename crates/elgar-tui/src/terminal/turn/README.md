# terminal/turn

## Purpose

This folder owns what happens after the user submits text.

## Files

- `mod.rs` exposes the provider-turn modules to the parent terminal module.
- `submitted.rs` handles submitted text while the provider is idle.
- `submitted/` contains local command execution and submitted-input logging
  helpers.
- `provider.rs` runs one provider turn and applies completed events to the TUI.
- `active.rs` handles typing and `/cancel` while a provider request is running.
- `provider_worker.rs` runs the provider request in a background worker.

## Rule

Turn code can coordinate input, provider calls, session updates, and rendering, but individual responsibilities should stay split across these files.

## Approval Commands

`submitted.rs` handles approval actions locally while the provider is idle.
Keyboard-first `[Approve]` / `[Deny]` buttons and the `/approve`, `/deny`, and
`/reject` command fallbacks all delegate to core approval functions so pending
approval state and execution stay in `elgar-core`.
