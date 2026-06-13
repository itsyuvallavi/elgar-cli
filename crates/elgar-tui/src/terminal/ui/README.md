# terminal/ui

## Purpose

This folder owns terminal display and formatting.

## Files

- `mod.rs` exposes the UI modules to the parent terminal module.
- `approval.rs` renders core-owned pending approval state for risky primitives.
- `approval_action.rs` defines the selected approval button action for the
  inline prompt.
- `approval_card.rs` renders the boxed approval card and selected-button footer.
- `prompt.rs` draws the editable prompt and live provider preview.
- `prompt/` contains prompt frame construction, live-output preview state, and
  wrapping helpers used by the inline prompt renderers.
- `footer.rs` formats the footer location/model/context line.
- `render.rs` renders conversation output and Ratatui frames.
- `code_syntax.rs` detects rendered code block borders, headers, and body
  lines for ANSI/Ratatui styling.
- `code_tokens.rs` classifies code body text into simple token styles.
- `code_tokens/` contains language normalization and token scanner helpers for
  code body styling.
- `text.rs` wraps and formats transcript text for printing.

## Rule

UI code should draw state it receives. It should not decide slash commands or start provider requests.

Approval controls are text-rendered buttons inside the inline terminal flow.
Do not add alternate-screen rendering or mouse capture unless native terminal
scrollback and text selection are explicitly preserved.
