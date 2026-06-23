# Elgar TUI Complex-Data Rendering Handoff

## Purpose

Hand off the next Elgar TUI rendering pass to a fresh agent without losing the
current context.

Use the existing **TUI Agent** role from `AGENT_ROSTER.md`. Do not create a new
standing agent role unless the project scope changes.

## Current Situation

The core harness is behaving better than the TUI display:

- current-folder project review now routes into local inspection
- shell cwd/path anchoring is fixed for the tested `playground/Nextjs-1` case
- recursive project listings are bounded and skip generated/vendor folders
- duplicate successful shell commands in one turn are suppressed from the
  normal transcript
- completed shell results still preserve raw command, cwd, stdout, stderr, exit
  code, timeout, truncation, and elapsed time in verified events

The remaining problem is user-facing rendering. The conversation pane still
prints raw verified shell details such as `Command:`, `Cwd:`, and flattened
`stdout:` blocks. That is technically truthful but visually unacceptable.

The fix should be a TUI display-layer change, not a routing/model/core
semantics change.

## Required Reading

Start with:

```text
zz_elgar_agent_docs/AGENTS.md
zz_elgar_agent_docs/AGENT_ROSTER.md
docs/elgar-product-architecture-plan.md
docs/pi-like-tui-direction.md
docs/pi-like-terminal-tui-visual-spec.md
docs/tui-visual-qa-checklist.md
docs/local-checks.md
```

Then inspect current implementation files:

```text
crates/elgar-tui/src/panes/conversation.rs
crates/elgar-tui/src/panes/event_rendering.rs
crates/elgar-tui/src/shell_result.rs
crates/elgar-tui/src/panes/tool_activity.rs
crates/elgar-tui/src/action_panel.rs
crates/elgar-tui/src/memory.rs
crates/elgar-tui/src/terminal/commands.rs
crates/elgar-tui/src/terminal/keymap.rs
```

Useful core context, read only if needed:

```text
crates/elgar-core/src/event.rs
crates/elgar-core/src/action.rs
crates/elgar-core/src/agent_loop.rs
crates/elgar-core/src/shell.rs
```

## Display Contract

Raw truth and display are separate:

- raw verified events stay in trace/session memory
- main conversation UI renders clean typed summaries by default
- raw shell command, cwd, stdout, stderr, truncation info, and elapsed time stay
  accessible through details/trace/raw copy paths
- the model is not responsible for formatting shell output
- runtime validation, policy, execution, and verification remain unchanged

## Implementation Plan

1. Add a small typed conversation display layer in `elgar-tui`.
   - Internal display block ideas:
     - `UserMessage`
     - `AssistantMessage`
     - `ToolSummary`
     - `ProjectTree`
     - `FileList`
     - `ErrorSummary`
     - `Metrics`
     - `HiddenDetails`
   - Keep the code scoped and files small.

2. Replace default shell result rendering in the main conversation.
   - Stop showing raw `Command:`, `Cwd:`, `stdout:`, and `stderr:` by default.
   - Render compact summaries, for example:
     - `✓ listed files · 13 entries · exit 0`
     - `✗ shell command failed · exit 1 · stderr available`
     - `✗ shell command timed out · 10s`
   - Keep raw details available outside the default conversation block.

3. Add specialized renderers.
   - Project tree/list renderer for `find`/`ls` style output.
   - File list renderer for verified created files and state answers.
   - Plan status renderer if current plan output still feels log-like.
   - Error renderer with concise reason plus hidden details marker.

4. Add details access.
   - Prefer a small slash-command path such as `/details last`, `/copy raw`, or
     an equivalent existing command.
   - Normal `/copy` should remain readable.
   - Raw copy/details must preserve command, cwd, stdout, stderr, truncation,
     exit code, and timing.

5. Add duplicate/noise suppression at the display layer.
   - If the same command/output repeats nearby, render a compact
     `same listing as previous` style summary.
   - Do not dump identical tree/list output repeatedly.

6. Test and dogfood.
   - Unit-test shell success/failure/timeout summaries.
   - Unit-test that raw stdout is not shown by default in the conversation pane.
   - Unit-test project tree/file list rendering at narrow widths.
   - Regression-test normal copy versus raw copy/details behavior.
   - Run a live TUI smoke with:
     - `hello, please review this project.`
     - `show me the project tree.`
   - Screenshot-check that the output is readable in an 80-column terminal.

## Guardrails

- Do not change model routing, prompts, or verified-state semantics unless
  absolutely necessary.
- Do not weaken verified action recording.
- Do not hide errors completely; show short error summaries and preserve full
  stderr in details/raw paths.
- Do not add natural-language trigger tables in core routing.
- Do not make the model format shell output.
- Keep plain chat cheap and unaffected.
- Keep runtime/core as source of truth; TUI renders events.

## Acceptance Criteria

- `show me the project tree` produces a clean tree/list, not raw
  `Command/Cwd/stdout` text.
- `review this project` shows a concise answer plus compact tool summaries.
- Full raw shell details remain available.
- Existing verified action recording remains intact.
- Plain chat remains cheap and unaffected.
- `cargo fmt`, `cargo check`, and tests pass.
- Live TUI dogfood demonstrates readable output in an 80-column terminal.

## Prompt For The Next Agent

```text
You are the Elgar TUI Agent. Continue from the current Elgar v0.10 branch.

First read:
- zz_elgar_agent_docs/AGENTS.md
- zz_elgar_agent_docs/AGENT_ROSTER.md
- zz_elgar_agent_docs/ORCHESTRATOR_SITUATION_2026-06-01_TUI_RENDERING.md
- docs/elgar-product-architecture-plan.md
- docs/pi-like-tui-direction.md
- docs/pi-like-terminal-tui-visual-spec.md
- docs/tui-visual-qa-checklist.md
- docs/local-checks.md

Task:
Implement the TUI complex-data rendering pass. The current harness stores correct
verified shell truth, but the conversation pane prints raw command/cwd/stdout
blocks. Replace that default display with compact typed summaries and clean
project-tree/file-list rendering while preserving raw details in trace/session
logs and a details/raw-copy path.

Scope:
- Work primarily in elgar-tui.
- Inspect:
  - crates/elgar-tui/src/panes/conversation.rs
  - crates/elgar-tui/src/panes/event_rendering.rs
  - crates/elgar-tui/src/shell_result.rs
  - crates/elgar-tui/src/panes/tool_activity.rs
  - crates/elgar-tui/src/action_panel.rs
  - crates/elgar-tui/src/memory.rs
  - crates/elgar-tui/src/terminal/commands.rs
  - crates/elgar-tui/src/terminal/keymap.rs
- Read core event/action types only as needed.

Do not:
- change model routing
- change prompts
- weaken verified action recording
- add natural-language trigger tables
- make the model responsible for formatting shell output

Acceptance:
- “show me the project tree” renders a clean tree/list, not raw Command/Cwd/stdout text.
- “review this project” shows concise tool summaries.
- raw command/cwd/stdout/stderr remain available through details/trace/raw copy.
- plain chat remains cheap and unaffected.
- tests cover shell summaries, hidden stdout by default, tree/list rendering, and copy/details behavior.
- run cargo fmt/check/tests and a live TUI smoke before reporting back.

Before code changes, find or create the relevant Linear issue and move it to In
Progress. If Linear auth is unavailable, continue locally and provide exact
Linear update text in the final report.
```

