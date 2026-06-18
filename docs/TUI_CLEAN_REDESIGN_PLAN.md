# TUI Clean Redesign Plan

## Purpose

Make the terminal UI quieter, clearer, and easier to trust while preserving the
current harness contract.

The TUI should feel like a clean conversation surface for a coding agent:

```text
user prompt
-> calm working state
-> organized answer
-> readable files / commands / verification
-> optional details
```

It must not become a dashboard, alternate-screen app, or second runtime.

## Non-Negotiables

- Keep normal terminal scrollback.
- Keep mouse/text selection working.
- Do not enable alternate-screen rendering for the interactive TUI.
- Do not enable mouse capture.
- Do not move permission policy, provider behavior, or runtime truth into TUI.
- Do not add hardcoded natural-language trigger rules.
- Keep files under 300 lines; split immediately when a file approaches that
  size.
- Keep `/approve`, `/deny`, `/prompt`, `/cancel`, `/details last`, and
  `/permissions` behavior intact.

## Current State

What already works:

- Interactive TUI uses inline stdout rendering, so native scrollback and text
  selection are preserved.
- Scripted `elgar tui` supports `/prompt` and `/end` for one multiline harness
  turn.
- Provider turns run in a worker and can be canceled through `/cancel`.
- Provider turns stream reasoning/text chunks into logs and the active prompt
  preview before final completion.
- Conversation rendering supports user messages, assistant messages, provider
  reasoning previews, metrics, approval cards, code blocks, and hidden raw
  details.
- Code blocks and long raw details are capped, with `/details last` and
  `/copy raw` fallback paths.
- Approval cards are core-state-driven. A visible approval means core has a
  real pending approval.

Current UX gaps:

- Responses are rendered as generic markdown, not as semantic answer sections.
- Bash output, file trees, verification, and files changed are not consistently
  formatted.
- Reasoning is collapsed, but the summary can still feel raw or noisy.
- Approval cards show too much raw detail for common cases.
- Footer combines cwd, model, context, and approval hints into dense text.
- Active-turn animation is a simple `Thinking...` pulse with little useful
  structure.
- There is no e2e visual transcript gate for a full project-generation flow.

## Target Experience

### Rendering Model

The TUI should prefer typed rendering from harness events before falling back to
markdown formatting:

```text
verified harness event -> typed display block
assistant markdown     -> structured response sections
plain prose            -> plain text
```

This keeps runtime truth in core while giving the terminal a cleaner view.

Typed containers should be used for:

- file writes, edits, and changed-file summaries
- bash and shell command results
- file trees and generated project structures
- approval requests and batch approval steps
- MCP/tool results when they are shown to the user
- verification results, failures, and recovery steps

Inline marking should be used for:

- file paths
- shell commands
- statuses such as passed, failed, pending, canceled, skipped
- diff counts and file counts
- model/tool timing and token facts when shown

Normal explanation should remain plain text. The TUI must not infer hidden
truth from prose; it may only improve how visible text and verified events are
displayed.

### Conversation

Assistant responses should scan as stable sections when the content supports
it:

```text
Summary
Files
Commands
Verification
Notes
```

The TUI should improve presentation only. The model still decides what to say,
and core logs remain the source of truth.

### Reasoning

Default visible reasoning should stay quiet but present:

```text
The user is asking what I can do; answer from the available local tools.
```

Completed provider reasoning remains in normal chat scrollback before the final
assistant answer, but normal chat shows only compact readable reasoning text.
Full raw reasoning stays available through `/details last` and raw diagnostic
logs. Do not replace visible reasoning with diagnostic metadata counts.

### Commands

Bash/shell output should render as a quiet command block:

```text
$ npm run build
exit 0 · 10.1s
```

Stdout/stderr should be capped and selectable text, not a noisy full dump.

### File Trees

Tree-like output should keep indentation and alignment:

```text
project/
  app/
    page.tsx
    globals.css
```

No icons or mouse-only interactions.

### Approval

Approval cards should show the minimum exact information:

```text
Action prepared
write · app/page.tsx
scope: inside launch folder
[Approve]   Deny
```

Batch approvals should show the step count and concise step list.

### Footer

Footer should stay quiet:

```text
elgar/playground/demo                    full_access · qwen3.6 · 12.4k/128k (9%)
```

The context percentage must be provider-backed and should use cumulative
provider-reported token usage for the active session. It is a running session
usage indicator, not a guarantee that every historical token is still inside the
next provider prompt. If token usage or the configured context window is
missing, the footer must show an unknown marker rather than a computed
estimate.

## Implementation Slices

### Slice 1: Display Sections

Goal: make final answers visually organized without changing model behavior.

Status: implemented as the first display-only pass.

Follow-up status: loose terminal tables such as `Entry | Description` followed
by dashed `-----+-----` separator rows are normalized into compact rows, so
directory summaries do not render as wide ASCII tables.

Follow-up status: long multi-section answers now render as plain sections
instead of bordered response boxes, avoiding broken borders after resize or in
narrow terminals. Short structured summaries still use the quiet response box.

Add:

- `crates/elgar-tui/src/terminal/ui/sections.rs`
- `crates/elgar-tui/src/terminal/ui/section_render.rs`

Edit:

- `crates/elgar-tui/src/markdown/mod.rs`
- `crates/elgar-tui/src/panes/event_rendering.rs`
- `crates/elgar-tui/src/terminal/ui/mod.rs`

Behavior:

- Detect simple heading-like sections from rendered assistant markdown.
- Normalize common section titles.
- Render multi-section answers inside one quiet response container when that
  improves scanning.
- Render loose two-column terminal tables as compact readable rows.
- Mark file paths, commands, and status words distinctly without changing their
  text.
- Preserve content as selectable plain text.
- Keep raw details available.

Tests:

- Section parsing does not alter plain answers.
- Section rendering keeps bullets and code blocks readable.
- Section rendering keeps file paths and commands visible and copyable.
- Wrapped list continuations remain aligned inside the section container.
- Loose table separator rows are removed, and wrapped cells stay attached to
  their row.
- Long content remains capped through existing details flow.

### Slice 2: Event Containers

Goal: render verified harness events as clean typed blocks before relying on
assistant markdown.

Add:

- `crates/elgar-tui/src/terminal/ui/event_blocks.rs`

Edit:

- `crates/elgar-tui/src/panes/event_rendering.rs`
- `crates/elgar-tui/src/terminal/ui/mod.rs`

Behavior:

- Render writes, edits, bash, approvals, and MCP/tool evidence from typed
  events when those events are visible in the conversation.
- Group related file changes into concise file rows with counts or status.
- Keep command status, exit code, duration, and capped output together.
- Never let TUI event rendering execute, approve, retry, or synthesize
  behavior.

Tests:

- Write/edit events render as file rows.
- Bash success and failure render as command result blocks.
- MCP/tool evidence stays compact and does not dump raw JSON by default.
- Raw details remain available.

### Slice 3: Command Blocks

Goal: make shell/build/install output readable and compact.

Status: partially implemented for code block header cleanup; command result
blocks remain a follow-up.

Add:

- `crates/elgar-tui/src/terminal/ui/command_block.rs`

Edit:

- `crates/elgar-tui/src/markdown/code.rs`
- `crates/elgar-tui/src/markdown/tests/markdown_test.rs`

Behavior:

- Render fenced `bash`, `sh`, `shell`, and command-like blocks with a compact
  command header.
- Preserve normal code block rendering for source code.
- Do not parse or invent exit codes unless the text includes them.
- Prefer concise code headers. Use filenames when known; otherwise use the
  language label without noisy line counts for normal-sized blocks.

Tests:

- Bash block renders compactly.
- Source code block remains source-code styled.
- Long command output is capped and still exposes raw details.

### Slice 4: File Tree Blocks

Goal: make generated project structure easy to inspect.

Add:

- `crates/elgar-tui/src/terminal/ui/file_tree.rs`

Edit:

- `crates/elgar-tui/src/markdown/mod.rs`

Behavior:

- Detect tree-like plain blocks conservatively.
- Preserve indentation.
- Do not rewrite arbitrary prose.

Tests:

- Tree output remains aligned.
- Non-tree text is unchanged.
- Long trees are capped.

### Slice 5: Reasoning Display

Goal: keep provider reasoning visible in the chat without turning the TUI into
a raw log console.

Status: live streamed reasoning is shown as compact status while a turn is
active, and completed provider reasoning is visible as compact status in the
conversation transcript.

Edit:

- `crates/elgar-tui/src/panes/provider_reasoning.rs`
- `crates/elgar-tui/src/panes/conversation.rs`
- `crates/elgar-tui/src/terminal/turn/provider_worker.rs`
- `crates/elgar-tui/src/terminal/ui/prompt/live_output.rs`

Behavior:

- Render completed reasoning as quiet status before the assistant answer.
- Stream reasoning/text chunks from the harness worker while the turn is active.
- Persist streamed chunks to logs so canceled calls are diagnosable.
- Keep raw reasoning out of normal chat and preserve it in details/logs.
- Keep raw event detail available through `/details last`.
- Do not hide reasoning solely because the request mode is tool decision or
  synthesis.

Tests:

- Completed tool-decision and synthesis reasoning status remains visible.
- Assistant answer still appears after reasoning.
- Copy output can omit reasoning when using normal copy.
- Canceled provider calls retain streamed chunk evidence in JSONL.

### Slice 6: Approval Card Cleanup

Goal: make approvals clear without raw argument noise.

Status: implemented for compact approval cards, in-card approval controls, and
compact verified execution display.

Edit:

- `crates/elgar-tui/src/terminal/ui/approval_card.rs`
- `crates/elgar-tui/src/terminal/ui/approval_card/tests.rs`
- `docs/TUI.md`

Behavior:

- Common single-action card shows action, target, scope, and controls.
- Batch card shows exact steps but avoids raw JSON unless no target preview
  exists.
- Approval controls render inside the live card, not in the footer.
- Safe approval cards hide approval ids, resolved paths, raw arguments, and
  fallback command prose.
- Unsafe/outside-path approvals keep the warning visible.
- Verified execution results render as short lines such as
  `Done · hello-world.md created`, with raw proof retained in details/logs.

Tests:

- Single write approval is compact.
- Batch approval lists step count and targets.
- Absolute/outside path warning remains visible.
- Tab/Arrow selection changes the in-card selected action.

### Slice 7: Footer And Working State

Goal: calm status without hiding critical state.

Edit:

- `crates/elgar-tui/src/terminal/display_context/mod.rs`
- `crates/elgar-tui/src/terminal/ui/footer.rs`
- `crates/elgar-tui/src/terminal/ui/prompt/frame.rs`
- `crates/elgar-tui/src/terminal/ui/prompt/live_output.rs`

Behavior:

- Show cwd, permission mode, model, and context window cleanly.
- Show context-window percentage only from provider-reported usage plus a
  configured window.
- Keep approval controls out of the footer; pending approvals belong to the live
  card.
- Replace `Thinking...` with stable `working · Ns · /cancel`.
- Cancel stuck interactive provider turns with a configurable watchdog instead
  of allowing indefinite spinner state.
- Keep prompt separators full-width while avoiding stale resize artifacts.
- Avoid layout jumps where possible.

Tests:

- Footer fits common 80-column terminal width.
- Permission mode is visible.
- Footer remains free of approval controls.
- Working line is stable across ticks.
- Watchdog timeout logs and shows a safe local cancellation message.
- Prompt frame separators span the drawable terminal width after resize.

### Slice 8: MCP Config And Visibility

Goal: make MCP availability explicit so the user and model do not have to guess
whether `mcp_call` exists in the current launch.

Add:

- `elgar-mcp.json`

Edit:

- `crates/elgar-tui/src/startup/`
- `crates/elgar-tui/src/terminal/display_context/mod.rs`
- `crates/elgar-tui/src/terminal/ui/footer.rs`
- `docs/MCP.md`
- `docs/TUI.md`

Behavior:

- Add a repo-level `elgar-mcp.json` with the default local project-index MCP
  config and the documented Context7 HTTP server entry.
- Support launching with `ELGAR_MCP_CONFIG=/path/to/config.json` when a caller
  wants an explicit alternate config.
- Startup display shows `MCP active` with configured server ids, or
  `MCP inactive` when no config is loaded.
- Footer shows compact MCP status only when useful; it must not crowd out cwd,
  model, or real context-window usage.
- The model-facing tool schema remains source-of-truth driven: `mcp_call` is
  exposed only when the loaded MCP config has available tools.
- Do not hardcode natural-language MCP triggers.

Tests:

- Startup renders `MCP inactive` when no config is loaded.
- Startup renders configured server ids when repo-level `elgar-mcp.json` is
  loaded.
- `ELGAR_MCP_CONFIG=/path/to/config.json` overrides repo-level config.
- Model tool schema includes `mcp_call` only when MCP is active.
- Footer/status display does not claim MCP access when the schema is hidden.

### Slice 9: Logs-Only Memory, Context, And MCP Diagnostics

Goal: make memory, session context, and MCP availability easy to inspect from
JSONL and `elgar logs --follow`, without adding another TUI command surface.

Add:

- `crates/elgar-core/src/session/status_logging.rs`
- `crates/elgar-cli/src/diagnostics/logs/follow_render.rs`

Edit:

- `crates/elgar-core/src/session.rs`
- `crates/elgar-core/src/harness/harness_loop/control/loop_setup.rs`
- `crates/elgar-cli/src/diagnostics/logs/follow.rs`
- `crates/elgar-cli/src/diagnostics/logs/summary.rs`
- `crates/elgar-cli/src/diagnostics/logs/render.rs`
- `docs/LOGGING.md`
- `crates/elgar-cli/src/diagnostics/logs/README.md`

Behavior:

- Log `harness_session_context_status` after provider metrics are recorded.
- Include latest-turn tokens, cumulative session tokens, context window,
  percent used when known, permission mode, and compact pending approval status.
- Keep exact memory prompt stats in `harness_turn_prompt_context_built`.
- Log `harness_mcp_status` once per harness loop setup with active/inactive
  state, config source, server ids, and exposed tool count.
- Extend `elgar logs --follow` to print memory/context/MCP state in compact
  lines.
- Extend `elgar logs latest` to summarize memory/context/MCP state when present.
- Do not add `/context`; diagnostics remain logs-only.

Tests:

- A normal turn writes `harness_session_context_status`.
- A harness turn writes `harness_mcp_status` even when MCP is inactive.
- `elgar logs --follow` shows memory, session token context, MCP, approvals,
  provider timing, and render handoff lines.
- `elgar logs latest` includes context/memory/MCP summary lines.
- Normal follow output does not add raw prompt, raw response, full reasoning,
  tool arguments, or secrets.

### Slice 10: Command Palette

Goal: make local commands discoverable without replacing normal terminal input.

Add:

- `crates/elgar-tui/src/terminal/ui/command_palette.rs`
- `crates/elgar-tui/src/terminal/input/command_palette.rs`

Edit:

- `crates/elgar-tui/src/terminal/input/read.rs`
- `crates/elgar-tui/src/terminal/input/keymap.rs`
- `crates/elgar-tui/src/terminal/commands/messages.rs`
- `docs/TUI.md`

Behavior:

- Open an inline palette when the input starts with `/`.
- Filter local commands as the user types.
- Show command name and short description in selectable rows.
- Use `Up`/`Down` or `Tab` to move selection.
- Use `Enter` to fill or execute the selected command.
- Use `Esc` to close the palette and return to normal input.
- Keep `/commands` as a plain text fallback.
- Do not use alternate-screen rendering or mouse capture.
- First version lists local commands only.

Later expansion:

- permission modes
- MCP server status/actions
- installed skills

Tests:

- Palette opens only for slash input.
- Filtering narrows command rows.
- Keyboard selection is deterministic.
- Enter fills or executes the intended command.
- Esc closes without submitting.
- Normal text input, paste, approval buttons, and `/prompt` behavior stay
  unchanged.

## E2E Test Plan

### Automated

Run:

```text
cargo fmt
cargo test -p elgar-tui
cargo test -p elgar-cli tui -- --nocapture
cargo test -p elgar-core harness::tests::loop_flow -- --nocapture
./bin/install-local
```

### Dogfood

Run these scripted or manual dogfoods:

1. `elgar tui` scripted `/prompt` project-generation flow with
   `/permissions full_access`.
2. Approval flow with one pending write and one batch write.
3. `/cancel` during a long generation.
4. `/details last` after a collapsed long block.

The dogfood report should include:

- transcript path
- session/system JSONL paths
- provider calls, rounds, duration, tokens
- whether scrollback-style transcript remains plain selectable text
- whether sections, command blocks, file trees, reasoning, footer, and approval
  cards render cleanly
- whether wrapped bullets align inside section containers
- whether the command palette opens for `/`, filters commands, and preserves
  normal input behavior
- whether `/prompt`, `/cancel`, `/approve`, `/deny`, `/permissions`, and
  `/details last` still work

### Manual Visual Check

Manually inspect:

- text selection works in terminal
- mouse scrollback works
- no alternate-screen behavior
- no mouse capture
- no overlapping footer/input text
- no noisy raw JSON unless explicitly requested

## Pre-Mortem

Failure: display formatter hides important truth.

Mitigation: keep raw details and copy raw paths; do not delete evidence text,
only render a calmer default view.

Failure: TUI accidentally owns runtime decisions.

Mitigation: formatting modules accept text/events and return display lines only.
No permission, provider, tool, or policy decisions.

Failure: terminal selection or scrollback breaks.

Mitigation: preserve inline stdout rendering; no alternate screen or mouse
capture.

Failure: formatter becomes framework-specific.

Mitigation: generic sections, code blocks, command blocks, and file trees only.
No Next.js-specific or model-specific rules.

Failure: files become oversized.

Mitigation: new modules per responsibility and file-size check after each
slice.

Failure: animation flickers or shifts layout.

Mitigation: one-line stable pulse; no dynamic cards while provider is active.

Failure: command palette breaks normal typing or approval selection.

Mitigation: enable it only while editing slash input, keep approval selection
precedence when the prompt is empty, and test paste/submit/escape paths.

## Completion Criteria

- TUI output is visibly cleaner on a live project-generation dogfood.
- Reasoning is compact and organized.
- Bash commands and file trees are readable.
- Approval card is clear and compact.
- Footer shows permission mode without clutter.
- Command palette makes local slash commands discoverable without breaking
  normal typing.
- `/prompt`, `/cancel`, `/approve`, `/deny`, `/permissions`, `/details last`,
  `/copy`, and `/copy raw` still work.
- Text selection and scrollback are preserved.
- Tests and Cursor dogfood pass.
- Docs and Linear are updated.
