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

The primary controls are keyboard-first text buttons rendered in the inline
terminal prompt:

- `[Approve]` executes the current pending approval through core.
- `[Deny]` rejects and clears the current pending approval.
- `Tab` switches the selected button.
- `Enter` activates the selected button when the prompt is otherwise empty.

`/approve`, `/deny`, and `/reject` remain command fallbacks. The TUI renders a
boxed approval card with action hints and shows a compact selected-button footer
line while approval is pending. It only displays and submits approval actions; it
does not own permission policy or execution truth.

If the model asks for approval in prose but core did not create a pending
approval record, the harness retries the model instead of showing a dead
approval action. A visible approval card means core has an executable pending
approval.

The terminal keeps the inline scrollback model. Approval controls must not use
alternate-screen rendering or mouse capture unless a future plan proves native
text selection and scrolling remain intact.

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
