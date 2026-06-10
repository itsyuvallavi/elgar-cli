# Terminal UI

## Purpose

The TUI is the user-facing terminal interface.

It should feel like a simple conversation, not a dashboard or raw log console.

## Startup

```text
elgar
-> elgar-cli startup
-> elgar-tui terminal shell
```

## Input Flow

```text
terminal key input
-> input buffer
-> slash command check
-> harness provider turn or local command
-> render conversation
```

Plain text goes to the model.

Known slash commands are handled locally.

Unknown slash commands show a local error.

## Approval Flow

Risky primitives such as `bash`, `write`, and `edit` are not executed directly
by the model. Core stores a pending approval record, and the TUI renders that
record after the provider turn.

The current controls are command-based:

- `/approve` executes the current pending approval through core.
- `/deny` or `/reject` rejects and clears the current pending approval.

The TUI renders a boxed approval card with action hints and shows a compact
`/approve` / `/deny` footer line while approval is pending. It only displays and
submits approval commands; it does not own permission policy or execution truth.

## Rendering

The TUI renders:

- user text
- provider-authored assistant text
- pending approval prompts
- capped reasoning preview
- response timing/token usage
- prompt/footer state
- copy/details views

## Current Folders

Important TUI source areas:

- `crates/elgar-tui/src/terminal/`
- `crates/elgar-tui/src/terminal/input/`
- `crates/elgar-tui/src/terminal/turn/`
- `crates/elgar-tui/src/terminal/ui/`
- `crates/elgar-tui/src/panes/`

## Boundary

TUI should not own provider behavior, runtime truth, or permission policy. It
submits input and renders state.
