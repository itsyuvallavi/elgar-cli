# panes

## Purpose

Pane-specific state and render helpers for visible TUI session areas.

## Files

- `conversation.rs` stores and renders conversation history, raw details, and turn metrics.
- `conversation/` contains conversation line styles, scrollback state, and
  provider-reasoning visibility helpers.
- `event_rendering.rs` converts core events into conversation text.
- `provider_reasoning.rs` renders provider reasoning state.
- `status.rs` stores the prompt input area, status line, and copy hint state.
- `tests/` contains pane behavior tests.

## Ownership

Keep panes presentational. They should accept structured state and avoid owning provider or command decisions.

## Checks

- `cargo test -p elgar-tui panes`
- `cargo test -p elgar-tui`
