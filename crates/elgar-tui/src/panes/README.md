# elgar-tui/src/panes

## Purpose

Pane-specific renderers for provider thinking and tool activity.

## Important Files

- `provider_thinking.rs` renders provider reasoning or thinking state.
- `tool_activity.rs` renders tool and command activity.

## Ownership

Keep panes presentational. They should accept structured state and avoid owning controller decisions.

## Checks

- `cargo test -p elgar-tui panes`
- `cargo test -p elgar-tui`
