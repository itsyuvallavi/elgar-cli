# Linear Sync Queue

Last updated: 2026-06-02 14:04 WEST

Use this file while Linear auth is unavailable. Keep it current after each
implementation, research, verification, or blocker update. When Linear works
again, sync each queued item to Linear, then mark the item synced here.

## Sync Rules

- Add or update an entry before reporting a completed local task.
- Include exact Linear-ready title, status, and comment text.
- Keep status values explicit: `Needs Linear`, `In Progress`, `Done`, or
  `Blocked`.
- Preserve verification commands and known limitations.
- Do not delete synced history; mark it `Synced` with the date.

## Queue

### Project Review File Inspection

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-358`

Issue title:

```text
TUI: make project review inspect key files before summarizing
```

Issue description:

```text
Improve the project-review path so "review this project" does not stop after a project listing or repeated listing summary.

Acceptance:
- "review this project" inspects relevant files such as package/config/app entry files.
- It produces concise project review findings or a useful review summary.
- It does not stop after only repeating/summarizing the project tree.
- Existing tree/list rendering remains unchanged.
- Raw details remain available through /details last and /copy raw.
- Plain chat and explicit file-read code panels remain unaffected.
```

Implementation note:

```text
Added model-selected execute intent `project_review`, injected a review-mode tool instruction only for that intent, and allowed provider findings to render after verified read-only inspection for project review while preserving suppression for ordinary verified write/action completion claims.
```

Verification so far:

```text
cargo fmt
cargo test -p elgar-core normal_turn_decision -- --nocapture
cargo test -p elgar-core project_review -- --nocapture
cargo test -p elgar-core agent_prompts_describe_plan_artifact_before_same_turn_execution -- --nocapture
cargo check -p elgar-core
cargo test -p elgar-core --lib
cargo test -p elgar-tui
cargo test -p elgar-cli
./bin/check-local
printf 'review this project\n/exit\n' | cargo run -p elgar-cli -- tui
./bin/install-local
```

Completion comment:

```text
Completed project-review inspection pass.

What changed:
- Added a model-selected `project_review` execute intent.
- Injected a review-mode tool instruction only for that intent.
- The review path now asks the model to inspect representative source/config/package files before finalizing.
- Provider findings are allowed to render after verified read-only inspection for project review.
- Ordinary verified write/action completion claim suppression remains unchanged.
- Added a narrow visible-text sanitizer for chat-template channel wrappers observed during live smoke.

Verification:
- cargo fmt
- cargo check -p elgar-core
- cargo test -p elgar-core normal_turn_decision -- --nocapture
- cargo test -p elgar-core project_review -- --nocapture
- cargo test -p elgar-core agent_prompts_describe_plan_artifact_before_same_turn_execution -- --nocapture
- cargo test -p elgar-core provider_visible -- --nocapture
- cargo test -p elgar-core --lib
- cargo test -p elgar-tui
- cargo test -p elgar-cli
- ./bin/check-local
- Live TUI smoke: `review this project` in playground/Nextjs-1 listed the project, read package/config/app files, and produced concise findings without raw stdout or leaked channel markers.
- ./bin/install-local
```

### TUI Complex-Data Rendering Pass

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-351`

Issue title:

```text
TUI: render shell/listing results as compact summaries
```

Issue description:

```text
Implement the TUI complex-data rendering pass for verified shell results.

Acceptance:
- "show me the project tree" renders a clean tree/list, not raw Command/Cwd/stdout text.
- "review this project" shows concise tool summaries.
- raw command/cwd/stdout/stderr remain available through /details last and /copy raw.
- plain chat remains cheap and unaffected.
- tests cover shell summaries, hidden stdout by default, tree/list rendering, and copy/details behavior.
- run cargo fmt/check/tests and a live TUI smoke before reporting back.
```

Completion comment:

```text
Completed TUI complex-data rendering pass.

What changed:
- Default shell results now render compact Tool result summaries instead of raw Command/Cwd/stdout/stderr blocks.
- find/ls/rg/fd/git ls-files output renders as a clean project tree/list.
- Generic stdout/stderr are hidden by default with details hints.
- /details last displays the latest raw shell details.
- /copy raw and /copy details copy raw captured shell details.
- Repeated equivalent listings collapse to same listing as previous.
- Truncated listing output drops incomplete final path lines.

Verification:
- cargo fmt
- cargo test -p elgar-tui
- ./bin/check-local
- live TUI smoke against local LM Studio

Result:
All local checks passed.

Known limitation:
Linear auth was unavailable during implementation, so the issue/status/comment could not be synced live.
```

Files changed:

```text
crates/elgar-tui/src/action_panel.rs
crates/elgar-tui/src/lib.rs
crates/elgar-tui/src/panes.rs
crates/elgar-tui/src/panes/conversation.rs
crates/elgar-tui/src/panes/event_rendering.rs
crates/elgar-tui/src/panes/status.rs
crates/elgar-tui/src/panes/verification_rendering.rs
crates/elgar-tui/src/shell.rs
crates/elgar-tui/src/shell_listing.rs
crates/elgar-tui/src/shell_result.rs
crates/elgar-tui/src/terminal.rs
crates/elgar-tui/src/terminal/commands.rs
crates/elgar-tui/src/terminal/inline.rs
crates/elgar-tui/src/terminal/keymap.rs
crates/elgar-tui/src/terminal/tests.rs
crates/elgar-tui/src/terminal/tests/commands_and_input.rs
crates/elgar-tui/src/terminal/tests/copy_clipboard.rs
```

### Linear Connector Auth Failure

Status: `Blocked`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-357`

Issue title:

```text
Linear connector auth returns 401 in Codex chat
```

Support summary:

```text
Local Linear MCP setup and login succeeds, but Codex in-chat Linear tools still fail with:

401: "Server returned 401: 'Reauthentication required'"

Confirmed:
- codex mcp add linear --url https://mcp.linear.app/mcp succeeds.
- codex mcp logout linear succeeds.
- codex mcp login linear succeeds.
- ~/.mcp-auth does not exist on this machine.
- experimental_use_rmcp_client is not supported by this Codex build.
- ~/.codex/config.toml has [mcp_servers.linear] url = "https://mcp.linear.app/mcp".

Still failing:
mcp__codex_apps__linear._list_teams({ limit: 5, includeArchived: false })
returns 401.
```

Next action:

```text
Route this to Linear/OpenAI connector support. Once auth is restored, sync this queue to Linear.
```

### TUI Long Response And Code Block UX Review

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-352`

Issue title:

```text
TUI: plan polished long-response and code-block rendering
```

Issue description:

```text
Goal:
Improve Elgar's terminal UI so long assistant responses, code blocks, config snippets, diagnostics, tool summaries, and state answers are readable, structured, and easy to copy, while keeping Elgar terminal-native and avoiding a dashboard rewrite.

Background:
We reviewed Codex, Claude Code, Gemini CLI, Aider, OpenCode, Rich/Glow, and LazyGit-style terminal patterns.

Key lesson:
Polished coding TUIs separate:
1. Raw truth: exact assistant text, tool output, stdout/stderr, diagnostics.
2. Readable display: styled markdown, compact tool rows, collapsed long blocks.
3. Copy/export paths: exact raw copy, markdown copy, block-level copy/export.

Elgar should borrow that separation.

Current Elgar gaps:
- Markdown rendering returns plain strings, not structured render blocks.
- Fenced code blocks become `code (lang):` plus indented text, but there is no real block identity, collapse state, copy affordance, filename label, or line count.
- Inline code is stripped instead of styled.
- Long code/config/diagnostic blocks can overwhelm the transcript.
- Copy paths need to preserve exact raw text separately from pretty display.
- Latest/current state is mostly implicit.
- Tables are simplified but not width-prioritized.
- Existing `/details last` and `/copy raw` are good escape hatches and must be preserved.

Focus:
- Markdown rendering for long assistant messages.
- Fenced code block layout.
- Language/file labels.
- Copy affordances.
- Collapsed long blocks with line counts.
- Wrapping vs horizontal overflow.
- Inline code styling.
- Progress/status rows.
- Latest/current markers.
- Keyboard-only usability.
- Preserving exact raw copy text separately from pretty rendering.
- Terminal width constraints and small-window behavior.

Constraints:
- Keep Elgar terminal-native, not a boxed dashboard.
- Do not add broad layout rewrites unless necessary.
- Keep plain chat cheap and unaffected.
- Keep raw details/copy paths exact.
- Do not change provider routing, policy, action truth, or runtime validation.
- Do not change model prompts.
```

Files to inspect:

```text
crates/elgar-tui/src/markdown.rs
crates/elgar-tui/src/terminal/render.rs
crates/elgar-tui/src/terminal/text.rs
crates/elgar-tui/src/terminal/prompt.rs
crates/elgar-tui/src/panes/conversation.rs
crates/elgar-tui/src/panes/event_rendering.rs
crates/elgar-tui/src/theme.rs
crates/elgar-tui/src/terminal/tests/
```

Implementation plan:

```text
Phase 1: Add a TUI-only render block model

Create a small focused module, likely:
- crates/elgar-tui/src/render_blocks.rs

Represent assistant/provider display as typed blocks derived from raw text:
- Paragraph
- List
- CodeBlock
- Table
- ToolSummary
- Diagnostic
- Metrics
- HiddenDetails

Each entry should preserve raw text separately from display text.

Suggested shape:

ConversationEntry:
- id
- source
- raw_text
- blocks
- is_latest

RenderBlock:
- Paragraph { spans }
- List { items }
- CodeBlock { language, label, raw, line_count, collapsed }
- Table { rows }
- ToolSummary { status, label, detail_ref }
- Diagnostic { severity, summary, raw_ref }
- Metrics { duration, tokens }

Phase 2: Refactor markdown rendering into parse-then-render

Update markdown rendering so `markdown.rs` can parse assistant markdown into render blocks first, then render blocks to terminal lines.

Requirements:
- Fenced code blocks keep raw contents exactly.
- Language labels are preserved.
- Optional filename/path labels are preserved if present.
- Inline code becomes visually distinct instead of stripped.
- Tables remain readable and width-aware.
- Live streaming preview and completed transcript share rendering rules.

Phase 3: Improve code/config block display

Render fenced code/config blocks as visually distinct terminal blocks.

Target default display:
- Header like `code rust · 18 lines`.
- Optional label like `app/page.tsx · tsx · 42 lines`.
- Muted/different styling from prose.
- Preserved indentation.
- Width-safe wrapping or clipping rules.
- Long block collapse threshold, for example over 80 lines or 4k chars.
- Collapsed summary like `code rust · 142 lines · collapsed, /details last or /copy block 2`.

Do not add syntax highlighting yet unless it is very small and low-risk. Start with labels, spacing, structure, and copy correctness.

Phase 3a: Visual and color styling contract

Make the overall visual treatment explicit, not just the text layout.

Requirements:
- Code/config containers have a visible terminal-native border or equivalent block boundary.
- Container border color is muted so it separates the block without reading as an alert.
- Code block header/meta text uses a subtle accent or muted label color distinct from body text.
- Code body text remains high-contrast and readable, with preserved indentation.
- Collapsed/hidden-line hints are muted but legible.
- Inline code is visually distinct from prose without stripping backticks from copy/raw paths.
- Tool/result summaries use consistent status color: success calm green, warning amber, error red, neutral muted gray.
- User message blocks, model prose, tool summaries, state answers, and code containers must have a documented style mapping in `theme.rs`.
- Avoid saturated dashboard colors, one-note palettes, and visual noise; color supports hierarchy only.

Testing/QA:
- Add tests or snapshots for `ConversationLineStyle` to theme mapping where practical.
- Manual QA must inspect dark terminal contrast for code container border, header, body, collapsed hint, tool summaries, and copied/raw details.
- Verify narrow and wide terminal widths do not make colored containers overlap or wrap incoherently.

Phase 3b: Lightweight code-block syntax styling

Add conservative, dependency-free token coloring inside code/config containers.

Requirements:
- Preserve exact raw markdown and copy/details behavior.
- Use language labels from fenced code headers only; do not infer from prose.
- Apply only low-risk token styles:
  - config keys: subtle accent
  - strings: calm green
  - numbers: amber
  - booleans/null-like literals: warm accent
  - comments: muted
- Support common config/code labels: TOML, YAML, JSON, Bash/Shell, Python, Rust, JavaScript, TypeScript.
- Unknown or `text` blocks remain plain code body styling.
- Live preview and completed transcript use the same styling path.

Testing/QA:
- Add renderer tests that assert keys, strings, numbers, literals, and comments get distinct styles.
- Keep syntax highlighting lightweight; this is not a grammar-aware parser.

Phase 4: Copy/export behavior

Preserve and extend copy paths:
- `/copy` remains readable pretty/markdown transcript copy.
- `/copy raw` remains exact raw details copy.
- Add `/copy block N` or `/copy code N` if scoped and simple.
- `/details last` remains exact and should show the raw underlying data.
- Pretty rendering must never become the source of truth for raw copy.

Phase 5: Long diagnostics and tool details

For diagnostics, shell output, and tool results:
- Normal transcript shows summary only.
- Full raw stdout/stderr/command/cwd remains available through details/raw copy.
- Failures show enough actionable info: status, exit code, first/last stderr lines when available, and a details hint.
- Do not dump huge stdout/stderr in normal chat.

Phase 6: Latest/current markers

Add small, non-noisy current/latest affordances where useful:
- Current active streaming block should be visually obvious.
- Latest assistant answer should be easy to identify.
- Avoid repeating labels on every paragraph.
- Prefer subtle footer/status/metrics or block-level marker.

Phase 7: Tests and visual QA

Add or update tests for:
- Markdown parser/render block behavior.
- Fenced code blocks with language, filename labels, line counts, and collapse.
- Code block and tool-summary color/style mapping where terminal renderer exposes styles.
- Inline code styling or marker preservation.
- Tables at 60/80/120 columns.
- Live streaming preview and completed transcript consistency.
- Copy behavior: pretty copy readable, raw copy exact, block copy exact if added.
- `/details last` still exact.
- Long diagnostics do not dump raw stdout/stderr by default.
```

Verification plan:

```text
cargo fmt --check
cargo test -p elgar-tui
./bin/check-local
```

Manual QA:

```text
Use docs/tui-visual-qa-checklist.md. Verify:
- fresh open
- after one prompt
- long assistant response
- code-heavy response
- config snippet response
- failed diagnostic/tool output
- narrow terminal
- wide terminal
- code container border/header/body/hint contrast
- tool summary success/warning/error/neutral color contrast
- text selection still works
- /copy, /copy raw, and /details last
```

Acceptance criteria:

```text
- Long assistant replies remain readable in 80 columns.
- Fenced code/config blocks show language, line count, and distinct styling.
- Code/config block containers have an explicit visual/color treatment for border, header, body, and collapsed hints.
- Tool/result summaries have explicit success/warning/error/neutral text color treatment.
- Long blocks collapse with a visible omitted-line count.
- Inline code is muted but legible.
- /copy raw preserves exact raw content.
- /copy remains readable.
- Raw tool details remain available but hidden by default.
- No provider routing, policy, runtime validation, or model prompt behavior changes.
- Plain chat remains cheap and unaffected.
- Elgar remains terminal-native and calm, not a boxed dashboard.
```

Risks/tradeoffs:

```text
- Styled display can corrupt copy if rendered text becomes source of truth. Keep raw separate.
- Syntax highlighting can add dependency/performance cost. Defer unless clearly worth it.
- Collapse can hide important failures. Always show failure summary and raw-details path.
- Too many hints can clutter the UI. Show copy/details hints only where useful.
- Width-aware table rendering can get complex. Prefer simple fallback over horizontal overflow.
```

Next action:

```text
Implementation completed locally on 2026-06-02 10:25 WEST. Create or update this Linear issue when auth is restored.
```

Completion comment:

```text
Completed the TUI long-response and code-block rendering pass.

What changed:
- Added a focused render-block helper for fenced code blocks.
- Fenced code/config blocks now render as terminal-native bordered panels with language, optional file label, and line count in the top border.
- Long fenced blocks collapse by default after 40 visible lines, with an omitted-line hint.
- Inline code markers are preserved instead of stripped.
- Collapsed provider replies store exact raw markdown in raw details.
- /details last and /copy raw expose the exact raw hidden markdown.
- Added explicit terminal visual styling for code block border, header, body, and collapsed hints.
- Added a separate raw-details visual style so /details last does not read as normal assistant prose.
- Tool result summaries now use status color treatment for success, warning, error, and neutral rows.
- Live streaming previews and completed transcript rendering share the same code-line ANSI styling path.
- Code/config block body lines now use lightweight language-tag-based token styling for config keys, strings, numbers, booleans/null-like literals, and comments.
- Unknown and `text` code blocks remain plain body styling to avoid noisy false positives.
- The scripted `elgar tui < stdin` loop now supports /details last, /copy raw, and /copy details, matching the TUI command contract.
- Updated the live dogfood script to assert the current clean tree/list plan preview format.

Files changed:
- bin/dogfood-tui-flow
- crates/elgar-cli/src/lib.rs
- crates/elgar-cli/src/tui_loop.rs
- crates/elgar-tui/src/lib.rs
- crates/elgar-tui/src/markdown.rs
- crates/elgar-tui/src/panes.rs
- crates/elgar-tui/src/panes/conversation.rs
- crates/elgar-tui/src/panes/event_rendering.rs
- crates/elgar-tui/src/render_blocks.rs
- crates/elgar-tui/src/terminal.rs
- crates/elgar-tui/src/terminal/commands.rs
- crates/elgar-tui/src/terminal/prompt.rs
- crates/elgar-tui/src/terminal/render.rs
- crates/elgar-tui/src/terminal/tests/copy_clipboard.rs
- crates/elgar-tui/src/terminal/tests/rendering_frames.rs
- crates/elgar-tui/src/theme.rs

Verification:
- cargo fmt
- cargo test -p elgar-cli -p elgar-tui
- cargo check -p elgar-tui -p elgar-cli
- ./bin/check-local
- ./bin/install-local
- ./bin/dogfood-tui-flow
- live focused TUI markdown smoke:
  - prompt requested a fenced text block with code-block-smoke-001 through code-block-smoke-085.
  - visible transcript rendered `╭─ code (text) · 85 lines · collapsed, showing 40`.
  - visible transcript showed body rows such as `│ code-block-smoke-040`.
  - visible transcript showed `│ ... 45 lines hidden; use /details last or /copy raw`.
  - /details last appended raw fenced markdown through code-block-smoke-085.
- renderer style tests assert:
  - code container border/header/body/hint styles are distinct.
  - lightweight code syntax spans color config keys, strings, numbers, literals, and comments.
  - raw details use the raw-details style.
  - tool summaries use success/error/warning status colors.

Result:
All local checks and live TUI smokes passed.

Known limitations:
- Syntax styling is intentionally lightweight and language-tag based, not full grammar-aware syntax highlighting.
- The scripted stdin smoke does not show ANSI colors in captured output; color behavior is covered by terminal renderer tests and visible in interactive terminal sessions.
- A follow-up routing fix now keeps the installed live TUI TOML code-block smoke on the chat route; see `TUI: keep text-only fenced code prompts on chat route`.
- Block-level copy commands were not added. Existing /copy remains readable; /copy raw and /details last provide exact hidden raw content.
- Linear auth was unavailable, so this could not be synced to Linear live.
```

### TUI: keep text-only fenced code prompts on chat route

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-353`

Issue title:

```text
TUI: keep text-only fenced code prompts on chat route
```

Issue description:

```text
Fix a routing regression where a pure text-generation prompt such as:

Write only one fenced TOML code block with [features], js_repl = false, count = 42, url = "https://mcp.linear.app/mcp", and a # disabled comment. No prose.

could be treated as local work because URL fragments and code-like assignment syntax looked like local filesystem/shell syntax. The TUI then surfaced:

Model routing response was not valid JSON; no filesystem action was applied.

Acceptance:
- Text-only fenced code/config prompts stay on the chat route.
- The response renders through the existing code-block UI.
- No tool-enabled turn is started.
- URL fragments like `//mcp.linear.app/mcp` are not counted as local file paths.
- Existing project-plan/artifact repair protections still execute instead of rendering fake local work.
```

Completion comment:

```text
Completed.

What changed:
- URL fragments produced by `https://...` tokenization no longer count as local path-like syntax.
- Unstructured provider output can fall back to visible chat only when it is a single fenced code/config block, contains no local path-like content, and the input is not local-work-shaped.
- Existing artifact/project-plan repair gates remain in place.

Files changed:
- crates/elgar-core/src/agent_loop.rs
- zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md

Verification:
- cargo fmt
- cargo test -p elgar-core agent_loop::tests::text_only_code_block_prompt_falls_back_to_visible_chat_without_tools
- cargo test -p elgar-core route
- cargo test -p elgar-core
- cargo test -p elgar-tui -p elgar-cli
- ./bin/check-local
- ./bin/install-local
- live installed TUI smoke with the exact TOML prompt:
  - rendered `╭─ code (toml) · 5 lines`
  - route: chat
  - provider requests: 1
  - tools: 0
  - no pending action

Known limitations:
- The fallback is intentionally narrow. It covers single fenced code/config output; broader unstructured non-JSON prose still goes through existing route repair/error behavior.
- Linear auth was unavailable, so this could not be synced to Linear live.
```

### TUI: remove provider label and widen code panels

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-354`

Issue title:

```text
TUI: remove provider label and widen code panels
```

Issue description:

```text
Polish the terminal conversation rendering after the fenced-code routing fix.

Problem:
- Completed provider responses still showed a stray `model` label above assistant output.
- Short fenced code/config responses rendered as cramped mini boxes, which looked noisy compared with the desired calm terminal-native UI.

Acceptance:
- Provider answers render without an extra `model` label.
- Fenced code/config panels have enough minimum width to read as transcript panels, not tiny widgets.
- Existing code block language, line count, syntax color, collapsed hint, /details last, and /copy raw behavior remains unchanged.
- Plain chat and routing remain unaffected.
```

Completion comment:

```text
Completed.

What changed:
- Removed the repeated `model` heading from completed provider output in the terminal transcript.
- Adjusted terminal conversation line counting to match the unlabeled provider output.
- Increased the minimum code panel content width from 56 to 64 characters.
- Tightened the render-block test contract so code blocks cannot regress back to tiny widgets.
- Updated renderer tests to assert completed provider responses are unlabeled.

Files changed:
- crates/elgar-tui/src/render_blocks.rs
- crates/elgar-tui/src/terminal/render.rs
- crates/elgar-tui/src/terminal/tests/rendering_frames.rs
- zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md

Verification:
- cargo fmt
- cargo test -p elgar-tui render_blocks
- cargo test -p elgar-tui rendering_frames
- cargo test -p elgar-tui -p elgar-cli
- ./bin/check-local
- ./bin/install-local
- live installed TUI smoke with the exact TOML prompt:
  - rendered `╭─ code (toml) · 5 lines` as a wider panel.
  - no `model` label appeared above the answer.
  - route: chat
  - provider requests: 1
  - tools: 0
  - no pending action

Result:
All local checks and installed smoke passed.

Known limitations:
- This is still terminal-native box drawing, not a GUI card. A future visual pass can flatten the border treatment further if we want it closer to Codex/Linear's web-card feel.
- Linear auth was unavailable, so this could not be synced to Linear live.
```

### TUI: render explicit file reads as code panels

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-355`

Issue title:

```text
TUI: render explicit file reads as code panels
```

Issue description:

```text
Fix the TUI display gap where prompts such as `read tailwind.config.ts to me` successfully executed `cat tailwind.config.ts`, but the normal transcript only showed `stdout hidden`.

Acceptance:
- Successful explicit file reads render the captured stdout as a terminal-native code panel.
- The panel includes the file path, inferred language from extension, and line count.
- Generic shell stdout remains hidden by default.
- Failed commands, stderr output, binary-looking output, and complex piped commands remain summarized with raw details only.
- /details last and /copy raw continue to preserve exact command/cwd/stdout/stderr.
```

Completion comment:

```text
Completed.

What changed:
- Successful `cat`, `bat`, `batcat`, `head`, `tail`, and `sed -n ... file` reads now render stdout as a code/file panel.
- File read summaries no longer show as green generic tool-result blocks; they render in the normal transcript with the same code panel styling as assistant fenced code.
- Generic shell commands still hide stdout by default.
- Raw shell truth remains available through /details last and /copy raw.

Files changed:
- crates/elgar-tui/src/shell_result.rs
- crates/elgar-tui/src/panes.rs
- zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md

Verification:
- cargo fmt
- cargo test -p elgar-tui shell_result
- cargo test -p elgar-tui conversation_renders_shell_file_read_as_code_panel
- cargo test -p elgar-tui -p elgar-cli
- ./bin/check-local
- ./bin/install-local
- live installed TUI smoke in playground/Nextjs-1:
  - prompt: `read tailwind.config.ts to me`
  - model executed `cat tailwind.config.ts`
  - transcript rendered `Read file · tailwind.config.ts · 11 lines`
  - transcript rendered `╭─ code (ts) · tailwind.config.ts · 11 lines`
  - `/details last` showed exact command, cwd, stdout, and stderr.

Known limitations:
- This detects common file-read shell commands only. More complex commands with pipes/redirection intentionally stay summarized to avoid misclassifying arbitrary stdout.
- `review this project` still depends on the model choosing deeper inspection commands; this patch only changes how explicit file-read stdout is displayed.
- Linear auth was unavailable, so this could not be synced to Linear live.
```

### TUI: mark wrapped code-panel continuations

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-356`

Issue title:

```text
TUI: mark wrapped code-panel continuations
```

Issue description:

```text
Polish code/file panel wrapping after explicit file reads started rendering as code panels.

Problem:
Long source lines in code panels wrapped as plain split text. In JSX, a className line could split mid-token, for example:

justify-cen
ter p-24">

That made the wrapped display look like separate source lines.

Acceptance:
- Code panel wrapping prefers whitespace breaks when possible.
- Wrapped continuation display lines are clearly marked.
- Raw details and raw copy preserve the exact original stdout/markdown.
- Existing code panel width, line count, syntax style, and collapsed behavior remain intact.
```

Completion comment:

```text
Completed.

What changed:
- Code panel display wrapping now prefers whitespace breaks.
- Continuation display rows use a visible `↳` marker.
- Raw source remains unchanged in /details last and /copy raw.
- Added a regression test for the JSX className wrapping case from the screenshot.

Files changed:
- crates/elgar-tui/src/render_blocks.rs
- zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md

Verification:
- cargo fmt
- cargo test -p elgar-tui render_blocks
- cargo test -p elgar-tui -p elgar-cli
- ./bin/check-local
- ./bin/install-local
- live installed TUI smoke in playground/Nextjs-1:
  - prompt: `read page.tsx`
  - output rendered `Read file · app/page.tsx · 12 lines`
  - long JSX className wrapped as:
    `<main className="flex min-h-screen flex-col items-center`
    `↳ justify-center p-24">`
  - no mid-token `justify-cen` / `ter p-24` split appeared.

Known limitations:
- The wrap marker is display-only; it is not a syntax-aware formatter.
- Linear auth was unavailable, so this could not be synced to Linear live.
```

## Entry Template

```text
### <Issue Title>

Status: `Needs Linear | In Progress | Done | Blocked`
Linear sync: `Needs Linear | Synced YYYY-MM-DD | Needs External Support`
Target team: `Elgar`

Issue title:

<title>

Issue description:

<description>

Update/comment:

<comment>

Files changed:

<paths>

Verification:

<commands/results>

Known limitations:

<limitations>
```
