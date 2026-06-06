# panes

## Purpose

Pane-specific state and render helpers for visible TUI session areas.

## Files

- `conversation.rs` stores and renders conversation history, raw details, and turn metrics.
- `event_rendering.rs` converts core events into conversation text.
- `provider_reasoning.rs` renders provider reasoning state.
- `status.rs` stores the prompt input area, status line, and copy hint state.

## Ownership

Keep panes presentational. They should accept structured state and avoid owning provider or command decisions.

## Checks

- `cargo test -p elgar-tui panes`
- `cargo test -p elgar-tui`
