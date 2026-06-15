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

Default visible reasoning should be compact:

```text
reasoning · planning files and verification
```

Long reasoning stays hidden and available through details/copy commands.

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
elgar/playground/demo                         full_access · qwen3.6 · 12.4k/128k
Approval pending (write) - [Approve] Deny - Tab switches - Enter selects
```

## Implementation Slices

### Slice 1: Display Sections

Goal: make final answers visually organized without changing model behavior.

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
- Preserve content as selectable plain text.
- Keep raw details available.

Tests:

- Section parsing does not alter plain answers.
- Section rendering keeps bullets and code blocks readable.
- Long content remains capped through existing details flow.

### Slice 2: Command Blocks

Goal: make shell/build/install output readable and compact.

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

Tests:

- Bash block renders compactly.
- Source code block remains source-code styled.
- Long command output is capped and still exposes raw details.

### Slice 3: File Tree Blocks

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

### Slice 4: Reasoning Display

Goal: reduce reasoning noise while keeping useful visibility.

Edit:

- `crates/elgar-tui/src/panes/provider_reasoning.rs`
- `crates/elgar-tui/src/panes/conversation.rs`

Behavior:

- Use a stable `reasoning · ...` line.
- Prefer action-oriented summaries.
- Keep raw detail hidden.
- Avoid showing tool protocol or JSON-like planning.

Tests:

- Tool-call reasoning stays hidden.
- Useful reasoning summary is compact.
- Long reasoning truncates predictably.

### Slice 5: Approval Card Cleanup

Goal: make approvals clear without raw argument noise.

Edit:

- `crates/elgar-tui/src/terminal/ui/approval_card.rs`
- `crates/elgar-tui/src/terminal/ui/approval_card/tests.rs`
- `docs/TUI.md`

Behavior:

- Common single-action card shows action, target, scope, and controls.
- Batch card shows exact steps but avoids raw JSON unless no target preview
  exists.
- Footer remains keyboard-first.

Tests:

- Single write approval is compact.
- Batch approval lists step count and targets.
- Absolute/outside path warning remains visible.

### Slice 6: Footer And Working State

Goal: calm status without hiding critical state.

Edit:

- `crates/elgar-tui/src/terminal/display_context/mod.rs`
- `crates/elgar-tui/src/terminal/ui/footer.rs`
- `crates/elgar-tui/src/terminal/ui/prompt/frame.rs`
- `crates/elgar-tui/src/terminal/ui/prompt/live_output.rs`

Behavior:

- Show cwd, permission mode, model, and context window cleanly.
- Keep approval state on its own second line only when needed.
- Replace `Thinking...` with stable `working · Ns · /cancel`.
- Avoid layout jumps where possible.

Tests:

- Footer fits common 80-column terminal width.
- Permission mode is visible.
- Approval footer remains visible.
- Working line is stable across ticks.

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

### Cursor Dogfood

Ask Cursor to run:

1. `elgar tui` scripted `/prompt` project-generation flow with
   `/permissions full_access`.
2. Approval flow with one pending write and one batch write.
3. `/cancel` during a long generation.
4. `/details last` after a collapsed long block.

Cursor should report:

- transcript path
- session/system JSONL paths
- provider calls, rounds, duration, tokens
- whether scrollback-style transcript remains plain selectable text
- whether sections, command blocks, file trees, reasoning, footer, and approval
  cards render cleanly
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

## Completion Criteria

- TUI output is visibly cleaner on a live project-generation dogfood.
- Reasoning is compact and organized.
- Bash commands and file trees are readable.
- Approval card is clear and compact.
- Footer shows permission mode without clutter.
- `/prompt`, `/cancel`, `/approve`, `/deny`, `/permissions`, `/details last`,
  `/copy`, and `/copy raw` still work.
- Text selection and scrollback are preserved.
- Tests and Cursor dogfood pass.
- Docs and Linear are updated.
