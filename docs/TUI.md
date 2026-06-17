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

While a provider turn is active, the TUI stays responsive and `/cancel` aborts
the request. Interactive provider turns also have a watchdog timeout so a stuck
model call returns a local message instead of leaving the terminal spinning
indefinitely. The watchdog can be overridden with
`ELGAR_TUI_PROVIDER_WATCHDOG_MILLIS`.

When the provider streams reasoning or text, the active inline prompt can show a
larger bounded live reasoning preview while the request is still running.
Streamed chunks are recorded in session/system logs before `provider_finished`,
so a canceled request can still be diagnosed from whatever Elgar received.
Finished provider events log first-reasoning, first-text, reasoning-to-text,
and total stream timings.

When visible answer text has already streamed, the TUI treats that preview as
the visible answer. On provider close, it compares the rendered live preview
with the rendered final provider message. If they match exactly, the prompt
chrome is cleared while the streamed answer stays in place. It must not append
a late `response ...` metrics line under the answer, because that makes the
conversation visibly change several seconds after the answer appeared. If the
final rendered content differs, the TUI falls back to the full final render
path.

## Approval Flow

Risky primitives such as `bash`, `write`, and `edit` are not executed directly
by the model. Core stores a pending approval record, and the TUI renders that
record after the provider turn. A pending approval may contain one exact action
or a serial batch of exact actions from one provider response.

The local `/permissions` command shows or changes the session permission mode.
`/permissions workspace_write` allows safe relative writes inside the launch
folder to run without approval for scaffold-style work. It does not auto-run
`bash`, `edit`, absolute paths, parent paths, symlink paths, or outside-folder
writes. `/permissions full_access` is an explicit trusted mode for local
dogfood/project generation: launch-folder writes, edits, and bash can run
without approval, while unsafe paths remain rejected by execution checks.
`/permissions review_all` restores the default approval behavior.

The primary controls are keyboard-first text buttons rendered in the inline
terminal prompt:

- `[Approve]` executes the current pending approval through core.
- `[Deny]` rejects and clears the current pending approval.
- `Tab` switches the selected button.
- `Enter` activates the selected button when the prompt is otherwise empty.

`/approve`, `/approve continue`, `/deny`, and `/reject` remain command
fallbacks. `/approve continue` executes the pending approval and then starts one
generic follow-up harness turn so the model can continue from verified approval
output. The TUI renders a boxed approval card with action hints inside the live
prompt frame while approval is pending. The footer stays reserved for stable
status such as cwd/model/context. The TUI only displays and submits approval
actions; it does not own permission policy or execution
truth.

If the model asks for approval in prose but core did not create a pending
approval record, the harness retries the model instead of showing a dead
approval action. A visible approval card means core has an executable pending
approval.

For batch approvals, the card lists the exact typed steps. One approval accepts
the batch, but core executes and logs the steps serially.

The terminal keeps the inline scrollback model. Approval controls must not use
alternate-screen rendering or mouse capture unless a future plan proves native
text selection and scrolling remain intact.

## Scripted TUI

`elgar tui` is a diagnostics surface for tests and dogfoods. It is line-based
by default, but scripts can submit one multiline prompt with `/prompt` and
`/end` on their own lines. This framing is scripted-only; the interactive TUI
continues to use normal terminal input behavior.

## Rendering

The TUI renders:

- user text
- provider-authored assistant text
- simple structured assistant sections as quiet selectable containers
- compact pending approval prompts
- compact verified execution results (`created`, `updated`, or `unchanged` for
  writes) with raw proof available through details
- larger capped reasoning preview
- live streamed reasoning preview during active provider turns
- quiet live-answer finalization when streamed and final rendered text match
- response timing/token usage
- prompt/footer state
- copy/details views

Prompt frame separators should span the drawable terminal width. Resize cleanup
must not leave stale line fragments above the prompt, but the steady-state frame
should still look full-width.

Structured rendering is display-only. The TUI may format visible assistant text
and verified event data, but core remains the source of truth for tools,
approvals, permissions, and execution.

Approval and execution displays should stay concise by default. The screen
should show the action, target, status, and warning when needed; raw approval
ids, resolved paths, arguments, and `VERIFIED_*` proof blocks belong in logs or
raw details.

## Footer Context Window

The footer may show context-window pressure as:

```text
1.2k/16k (7%)
```

The left number is the provider-reported prompt/input tokens for the latest
request, so it reflects current context-window pressure rather than cumulative
session spend. The percentage is shown only when it comes from provider token
usage and a configured context window. Estimated or unknown context snapshots
must not display a percentage; they render as `?/16k` so the TUI never presents
an inferred value as real.

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
