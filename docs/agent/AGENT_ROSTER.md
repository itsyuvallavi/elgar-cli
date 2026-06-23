# Elgar Agent Roster

## Purpose

Stable role names for focused Elgar work.

Use one role at a time unless the user explicitly asks for parallel review.

## Roles

### Core Runtime Agent

Owns `crates/elgar-core`.

Use for:

- harness runtime
- provider/session/events
- future tool/runtime boundaries
- logs owned by core

### TUI Agent

Owns `crates/elgar-tui`.

Use for:

- terminal input
- slash commands
- prompt/footer rendering
- conversation display

### CLI Agent

Owns `crates/elgar-cli`.

Use for:

- startup
- runtime path/config lookup
- diagnostic commands
- CLI smoke tests

### Docs And Logs Agent

Owns docs and observability documentation.

Use for:

- `docs/`
- `.elgar/log` documentation
- README updates
- stale doc cleanup

### Testing Agent

Owns test cleanup and verification strategy.

Use for:

- test cleanup
- boundary tests
- smoke tests
- dogfood scripts

## Rule

Do not create new standing roles unless the project shape changes.
