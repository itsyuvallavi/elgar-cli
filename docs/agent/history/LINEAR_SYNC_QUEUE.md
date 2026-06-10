# Linear Sync Queue

Last updated: 2026-06-03 13:40 WEST

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

### TUI Approval Card UX

Status: `Done`
Linear sync: `Needs Linear`
Target team: `Elgar`
Linear issue: `Create new issue`

Issue title:

```text
TUI: boxed approval card with /approve and /deny action hints
```

Comment:

```text
Added boxed pending-approval card rendering and footer action hints on top of core pending_approval.

Files:
- crates/elgar-tui/src/terminal/ui/approval_card.rs
- crates/elgar-tui/src/terminal/ui/approval.rs
- crates/elgar-tui/src/terminal/display_context/mod.rs
- docs/TUI.md
- bin/install-local (unset CARGO_TARGET_DIR before build)

Tests:
- cargo test -p elgar-tui approval
- ./bin/check-local
```

### Memory Slice 2: Cross-Turn Session Context

Status: `Done`
Linear sync: `Needs Linear`
Target team: `Elgar`
Linear issue: `Create new issue`

Issue title:

```text
Memory slice 2: inject verified facts and bounded chat history into harness prompts
```

Comment:

```text
Implemented memory slice 2 on branch codex/raw-chat-baseline.

What changed:
- Inject compact verified JSONL facts + bounded prior user/assistant turns at harness turn start.
- /clear and /new reset core Session events and rotate session id so durable memory starts fresh.
- Write/edit execution logs now include path metadata for recall facts.
- Repair prompts reuse session context.

Key files:
- crates/elgar-core/src/harness/harness_loop/provider/session_context.rs
- crates/elgar-core/src/harness/memory/render.rs
- crates/elgar-core/src/session.rs (reset_conversation)

Tests:
- cargo test -p elgar-core harness
- cargo test -p elgar-cli
- cargo test -p elgar-tui
- ./bin/check-local

Known limitations:
- JSONL is re-read each turn (no in-memory index cache yet).
- Live model recall quality still depends on LM Studio model behavior.
- Dogfood Test 1 replay in playground/Nextjs-1 still recommended.
```

### Plain Chat Reasoning Latency Audit

Status: `In Progress`
Linear sync: `Needs Linear`
Target team: `Elgar`
Linear issue: `Create new issue`

Issue title:

```text
Provider: audit plain-chat reasoning latency in LM Studio
```

Issue description:

```text
Narrow audit for the remaining Qwen/LM Studio latency issue after the tool-loop optimization pass.

Problem:
- Elgar now keeps trivial/plain chat on one no-tool provider request.
- Live TUI still showed `hello!` taking 19.6s with 757 reasoning tokens.
- That means the current bottleneck may be provider payload/config or LM Studio model behavior, not the harness loop.

Scope:
- Confirm the actual `plain_chat` request payload.
- Confirm whether Elgar sends `reasoning: off` or `reasoning: low`.
- Confirm the request uses the intended `lm_studio_native_chat` path.
- Confirm whether LM Studio honors the field.
- Compare the same prompt directly against LM Studio with the same reasoning setting.
- If Qwen still reasons heavily, decide between smaller-model routing, request-mode routing, or streaming.

Non-goals:
- Do not cap output tokens as the primary fix.
- Do not hardcode greetings or success/failure answers.
- Do not change runtime validation, action truth, prompts, or verified-state semantics except where required by measured payload evidence.

Acceptance:
- Trace or test output proves the exact outgoing plain-chat payload shape.
- Live/direct comparison shows whether reasoning control is honored.
- Recommendation identifies whether the next fix is config/payload, LM Studio behavior, smaller-model routing, or streaming.
- Existing tool-enabled and tool-result synthesis paths remain unchanged.
```

Latest local update:

```text
Linear MCP read failed again with:
401: "Server returned 401: 'Reauthentication required'"

Local finding:
- `elgar-provider.json` currently configures `plain_chat` with backend `lm_studio_native_chat` and stats true.
- It does not currently set `reasoning: off`, so the latest live Qwen `hello!` run was not proof that LM Studio ignored the field.

Audit results:
- Elgar live `hello!` stayed on one `plain_chat` request, zero tools, zero actions, but took 21.2s with 862 reasoning tokens.
- Direct LM Studio with a tiny system prompt took 4.1s with 135 reasoning tokens.
- Direct LM Studio with the exact Elgar plain-chat prompt took 22.8s with 1100 reasoning tokens.
- LM Studio rejects `reasoning: off`, `low`, and `high` for `qwen3.6-35b-a3b-ud-mlx`: model does not expose reasoning configuration.
- Native LM Studio streaming is supported, but this Qwen model streams reasoning before visible message text.

Verdict:
The remaining plain-chat latency is prompt-induced hidden reasoning on this Qwen model, not a tool-loop regression.

Recommended next fix:
Add request-mode-specific prompt profiles, provider capability guards for reasoning fields, native no-tool streaming, and optional small-model routing for trivial/plain chat and classifiers.

Evidence doc:
docs/plain-chat-reasoning-latency-audit.md
```

### TUI Dogfood Bad Output Fixes

Status: `Done`
Linear sync: `Needs Linear`
Target team: `Elgar`
Linear issue: `Update ELG-361`

Issue title:

```text
TUI: suppress display-only duplicate model output and raw tool protocol
```

Update/comment:

```text
Dogfood transcript showed several bad outputs:
- `show me the project tree` rendered the verified tree, then leaked raw `<tool_call>` text in the installed binary.
- Current source no longer leaked raw XML but still added a second model-generated tree/code block and summary after the verified tree.
- `read app/page.tsx` in the installed binary proposed a redundant cat command; current source fixed the pending proposal but still duplicated the code panel through shell-result synthesis.
- Shell report prompts in the installed binary ended after the tool result; current source had already fixed this, but the user's installed `elgar` binary was stale.

What changed:
- Centralized provider-visible filtering now drops raw XML/tool protocol text such as `<tool_call>`, `<function=...>`, and `<parameter=...>`.
- Display-only project tree/list prompts now stop after the verified compact tree rendering.
- Display-only file read prompts now stop after the verified code panel.
- These display-only paths keep raw details available through `/details last` and `/copy raw`.
- Actual shell-report prompts still use model-authored `tool_result_synthesis`.

Verification:
- cargo fmt --check
- cargo test -p elgar-core display_only_project_tree_stops_after_verified_listing -- --nocapture
- cargo test -p elgar-core display_only_file_read_stops_after_verified_code_panel -- --nocapture
- cargo test -p elgar-core provider_visible_drops_raw_tool_protocol_text -- --nocapture
- cargo build -p elgar-cli

Live source smoke:
- `show me the project tree` rendered only the compact verified tree. No raw tool XML and no duplicate model tree. provider_requests: 2.
- `read app/page.tsx` rendered only the verified code panel. No duplicate model code panel. provider_requests: 2.
- `/permissions full_access` then `Run npm run lint and tell me whether it passed or failed with the key reason. Do not edit files.` ran once, recorded exit 1, and produced a model-authored failure summary. provider_requests: 3.

Additional finding:
- The user's shell was running `/Users/yuval/.cargo/bin/elgar`, built Jun 2 22:09.
- The verified source binary was `/Users/yuval/__git/elgar/target/debug/elgar`, rebuilt Jun 3.
- Install the current build before further dogfood or the old bad outputs will persist.
```

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

### Harness project review approval gate and trivial Qwen latency

Status: `Done`
Linear sync: `Synced 2026-06-02`
Linear issue: `ELG-359`
Target team: `Elgar`

Issue title:

Agent: npm repro works but explicit fix stalls on repeated read

Update/comment:

```text
Update: traced and patched the latest harness regression.

Findings:
- `review the project` on Qwen proposed `find . -maxdepth 3 -not -path '*/node_modules/*' -not -path '*/.git/*' | head -80`.
- Elgar rejected all pipes in the read-only shell allowlist, so policy marked the safe inspection as `RequireReview` and stopped at a shell proposal.
- `hello!` latency was not executor overhead: Qwen spent about 16s / 780 completion tokens / 3k hidden thinking chars to route a trivial greeting.

Implementation:
- Allow one narrow read-only pipe shape: allowlisted read-only command piped only to bounded `head` (`head -80` or `head -n 80`).
- Rewrite piped/sorted project listing inspections into Elgar’s bounded safe project listing command before policy/application.
- Rewrite `cat <file> 2>/dev/null || echo "MISSING"` read fallbacks to direct `cat <file>` before policy/application.
- Add a controller route fast path for exact project-review prompts so Qwen does not spend a no-tool classifier turn before obvious project review work.
- Removed the Qwen-only controller fast path for trivial greetings. It was wrong because it made visible assistant prose harness-authored instead of model-authored.
- Add model-authored `project_review_synthesis` when project review stalls on repeated verified inspection; this uses `tool_count: 0` and asks the model to write final findings from verified tool results.
- Updated CLI tests that were using `hello` as a generic provider prompt to use non-trivial chat prompts instead.

Verification:
- `cargo fmt`
- `cargo test -p elgar-core shell_allowlist`
- `cargo test -p elgar-core piped_project_listing_shell_command_is_rewritten_to_bounded_inspection`
- `cargo test -p elgar-core sorted_project_listing_shell_command_is_rewritten_to_bounded_inspection`
- `cargo test -p elgar-core read_only_cat_missing_fallback_is_rewritten_before_policy`
- `cargo test -p elgar-core project_review_fast_path_skips_plain_route_classifier`
- `cargo test -p elgar-core project_review_repeated_inspection_synthesizes_final_findings`
- `cargo test -p elgar-core trivial_greeting_uses_plain_provider_request`
- `cargo test -p elgar-core agent_runtime_policy_allows_read_only_shell_commands_without_review`
- `./bin/check-local`
- `./bin/install-local`
- Live installed TUI smoke in `playground/Nextjs-1`:
  - prompt: `review the project`
  - rendered clean project tree and `package.json` code panel
  - produced model-authored review findings
  - no pending action remained
  - trace `cli-tui-20719-1780415295885-1.jsonl` shows:
    - no `plain_chat` route classifier request
    - `action-1` bounded safe `find` policy `AllowApply`
    - `action-2` `cat package.json` policy `AllowApply`
    - repeated read triggered `project_review_synthesis`
    - synthesis request had `tool_count: 0` and `tool_call_count: 0`

Known limitations:
- Qwen can still be slow on real review prose and simple chat; performance fixes must not replace model-authored assistant text with canned harness replies.
```

Files changed:

```text
crates/elgar-core/src/shell_allowlist.rs
crates/elgar-core/src/agent_loop.rs
crates/elgar-core/src/agent_runtime.rs
crates/elgar-cli/src/lib.rs
zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md
```

### ELG-359 correction: remove hardcoded trivial greeting

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`

Issue title:

Remove hardcoded trivial greeting reply from Elgar runtime

Update/comment:

```text
Correction: removed the hardcoded controller reply for exact trivial greetings.

Reason:
- The fast path returned `Hello! How can I help you today?` from the harness for `hello`/`hi`/`hey` on Qwen.
- That violates the Elgar contract: visible assistant prose for normal chat must be model-authored.

Implementation:
- Deleted the trivial greeting controller bypass from `agent_loop.rs`.
- Replaced the regression test with `trivial_greeting_uses_plain_provider_request`, which proves `hello!` sends one no-tool plain provider request and displays provider-authored text.
- Added a stronger `No Harness-Authored Assistant Prose` rule to `zz_elgar_agent_docs/AGENTS.md`.

Known performance implication:
- Simple greetings may be slower again until latency is optimized through fewer rounds, smaller prompts, tighter token budgets, or provider/runtime improvements. Do not use canned visible replies as a performance fix.
```

Files changed:

```text
crates/elgar-core/src/agent_loop.rs
zz_elgar_agent_docs/AGENTS.md
zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md
```

### ELG-359 Qwen latency split and request-mode optimization

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`

Issue title:

Optimize Elgar Qwen latency without hardcoded assistant replies or output caps

Issue description:

```text
Improve Qwen/LM Studio runtime latency while preserving the Elgar contract:

- Normal visible assistant replies must remain model-authored.
- Plain chat must use a plain provider request first.
- Tool-capable turns should expose tools only when needed.
- Verified shell/file truth remains in actions, traces, details, and raw copy.
- No model routing, policy truth, runtime validation, or core action truth may be weakened.
```

Implementation plan:

```text
1. Split provider request modes:
   - plain_chat
   - tool_enabled
   - tool_result_synthesis
   - project_review_synthesis

2. Do not cap provider output tokens; legitimate user requests may require long answers.

3. Keep simple chat model-authored and no-tool.

4. Optimize exact project review by doing deterministic verified read-only inspection first, then one model-authored synthesis request.

5. Verify trace/jsonl facts for request mode, provider round count, tool count, action count, token usage, and provider-authored final text.
```

Completion comment:

```text
Completed the Qwen latency split and request-mode optimization.

What changed:
- Added typed provider request modes for observability and cleaner routing.
- Removed provider output-token cap plumbing. Elgar does not send `max_tokens`, `max_completion_tokens`, or `max_output_tokens` as a latency shortcut.
- Exact `review the project` now skips the route-classifier/tool-loop round trip, runs verified deterministic read-only inspection through the existing policy/action path, then asks the model for one concise project-review synthesis.
- Trivial greetings remain provider-authored; no hardcoded assistant prose was added.
- Strengthened the AGENTS.md invariant against harness-authored natural-language assistant replies and output-token caps as a latency shortcut.

Verification:
- cargo fmt
- cargo test -p elgar-core request_modes_split_tool_and_tool_result_synthesis_without_caps
- cargo test -p elgar-core trivial_greeting_uses_plain_provider_request
- cargo test -p elgar-core project_review_fast_path_skips_plain_route_classifier
- cargo test -p elgar-core project_review_repeated_inspection_synthesizes_final_findings
- cargo test -p elgar-core project_review_surfaces_final_findings_after_verified_inspection
- cargo test -p elgar-cli runtime_provider_config_loads_compatibility_metadata
- ./bin/check-local

Live TUI smoke:
- `hello!` used one `plain_chat` provider request with tools 0 and provider-authored visible text. It still took about 10.9s on qwen3.6-35b-a3b-ud-mlx because hidden reasoning dominated.
- `review the project` used verified shell/file inspection plus one `project_review_synthesis` provider request with tools 0. The smoke completed in about 18.3s with model-authored findings, down from the previous multi-request path that could take around 60s.

Known limitation:
- Qwen hidden reasoning remains the main latency driver for plain chat. Further improvement should target model/provider settings, prompt/context slimming, streaming UX, request round reduction, or a separate small-model/router strategy, not canned visible replies or output caps.
```

Files changed:

```text
crates/elgar-cli/src/lib.rs
crates/elgar-cli/src/provider_config.rs
crates/elgar-core/src/agent_loop.rs
crates/elgar-core/src/agent_request_mode.rs
crates/elgar-core/src/agent_runtime.rs
crates/elgar-core/src/lib.rs
crates/elgar-core/src/provider/config.rs
crates/elgar-core/src/provider/lm_studio.rs
crates/elgar-core/src/provider/lm_studio_format.rs
crates/elgar-core/src/provider/mod.rs
crates/elgar-core/src/provider/types.rs
crates/elgar-core/src/shell_allowlist.rs
elgar-provider.json
zz_elgar_agent_docs/AGENTS.md
zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md
```

Correction synced 2026-06-02:

```text
Correction to the prior optimization update: output-token caps were removed.

Reason:
- Latency is the problem, not answer length.
- Elgar cannot know in advance how many tokens a legitimate user request needs.
- Capping provider output is the wrong lever and can make real tasks fail or truncate useful answers.

What is now true:
- Elgar does not send `max_tokens`, `max_completion_tokens`, or `max_output_tokens` as a latency shortcut.
- The `request_output_token_limits` config surface was removed.
- The `output_token_limit_field` compatibility surface was removed.
- Request-mode splitting remains only for observability/routing clarity: `plain_chat`, `tool_enabled`, `tool_result_synthesis`, and `project_review_synthesis`.
- The project-review optimization remains: deterministic verified inspection first, then one model-authored synthesis request.
- Trivial/simple chat remains provider-authored and no-tool.
- AGENTS.md now explicitly forbids output-token caps as a performance shortcut.

Verification:
- cargo fmt
- cargo test -p elgar-core request_modes_split_tool_and_tool_result_synthesis_without_caps
- cargo test -p elgar-core trivial_greeting_uses_plain_provider_request
- cargo test -p elgar-core project_review_fast_path_skips_plain_route_classifier
- cargo test -p elgar-cli runtime_provider_config_loads_compatibility_metadata
- ./bin/check-local
- ./bin/install-local
- Live TUI smoke: `hello!` used one `plain_chat` provider request, tools 0, provider-authored answer, about 12.9s on qwen3.6-35b-a3b-ud-mlx.

Current performance conclusion:
- The remaining latency is model/provider generation behavior, especially Qwen reasoning/generation time. Next improvements should target fewer provider rounds, smaller prompts/context, streaming UX, provider/model settings, or a small-router/split-agent strategy, not token caps or canned replies.
```

### ELG-360 per-turn provider latency breakdown

Status: `Done`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-360`

Issue title:

Perf: add per-turn provider latency breakdown report

Completion comment:

```text
Completed the first measurement pass for Qwen/LM Studio latency.

What changed:
- Added a first-class `TurnPerfSummary` and `ProviderRequestPerfSummary` in session state.
- Each finished trace turn now appends a `turn_perf_summary` JSONL event.
- The summary captures route, provider request count, request modes, exposed tool count, action count, provider duration, first chunk latency when available, message count, serialized request bytes, prompt/completion/total tokens, visible chars, thinking chars, tool call count, and per-request details.
- Added `elgar perf-trace` and `./bin/perf-trace` to render the latest local trace summary without calling the provider.
- The trace selector prefers the newest summary with provider duration metrics, so local stub/no-metrics traces do not hide the latest live LM Studio measurement.
- No model behavior changed. No token caps. No hardcoded visible replies.

Verification:
- cargo fmt
- cargo test -p elgar-core finish_trace_turn_records_plain_turn_perf_summary
- cargo test -p elgar-core finish_trace_turn_records_multi_request_tool_perf_summary
- cargo test -p elgar-cli renders_latest_trace_perf_summary_report
- ./bin/check-local
- ./bin/install-local
- Live TUI smoke: `hello!` in playground/Nextjs-1 used one `plain_chat` request and produced provider-authored text.
- Installed `elgar perf-trace` reported:
  - provider_requests: 1
  - route: chat
  - tools_exposed: 0
  - actions: 0
  - provider_time_ms: 10911
  - tokens: prompt 233, completion 353, total 586
  - context_shape: messages 3, request_bytes 1094
  - output_shape: visible_chars 61, thinking_chars 1390

Next performance work:
- Compare the same report across prompt/context slimming, streaming behavior, Qwen reasoning settings, and small-router/big-worker experiments.
```

Files changed:

```text
bin/README.md
bin/perf-trace
crates/elgar-cli/src/lib.rs
crates/elgar-cli/src/main.rs
crates/elgar-cli/src/perf.rs
crates/elgar-core/src/session.rs
docs/local-checks.md
docs/performance-baselines.md
zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md
```

### TUI Dogfood: Ground Verified Reads And Filter Tool Markup

Status: `Needs Linear`
Linear sync: `Synced 2026-06-02`
Target team: `Elgar`
Linear issue: `ELG-361`
Linear URL: `https://linear.app/elgar/issue/ELG-361/tui-ground-verified-read-file-responses-and-filter-tool-call-markup`

Issue title:

```text
TUI: ground verified read-file responses and filter tool-call markup leaks
```

Issue description:

```text
Live TUI dogfood on 2026-06-02 found two correctness/UI regressions in the model/tool path.

Observed cases:

1. `read page.tsx`
- Verified shell result correctly read and rendered `app/page.tsx`.
- The displayed code panel showed the actual 12-line file:
  - `Hello, Next.js + Tailwind!`
  - `Your scaffold is ready.`
- The model follow-up prose then hallucinated unsupported content: responsive layout, header, nav links, hero section, feature cards, CTA buttons, footer content.
- JSONL confirmed the verified `stdout_tail` was correct, then the final provider request produced the false prose.

2. `show me the project tree`
- The TUI rendered the compact tree/list correctly.
- Raw model tool-call markup leaked into the conversation after the tree:
  `<tool_call><function=shell_command>...`

3. Shell/build prompt latency
- Prompt: `Run npm run build in the current folder and report whether it passed or failed with the key reason. Do not edit files.`
- In `full_access`, the turn stayed in `plain_chat` and timed out after 120s before any shell action ran.
- JSONL recorded `provider_error` and `request_mode: plain_chat`.

Acceptance:
- Verified read-file turns must not allow unsupported model prose to contradict the verified file contents.
- If a final model summary is shown after a verified read, it must be grounded in the verified content or omitted in favor of the verified code panel.
- Raw `<tool_call>` / XML-ish provider markup must never appear in visible conversation text.
- Tool-intent prompts like run/build should not waste a long plain-chat request before tool execution.
- Raw command/cwd/stdout/stderr remain available through details/raw copy.
- Add regression tests for read-file grounding/filtering, tool-call markup filtering, and build/run intent routing.
```

Update/comment:

```text
Dogfood evidence:

Plain chat:
- Prompt: `hello!`
- Result: provider-authored response, one provider request, `plain_chat`, zero tools.
- Timing: 12.3s, 233 prompt tokens, 471 completion tokens, 704 total.

Read file:
- Prompt: `read page.tsx`
- Result: verified code panel was correct, but final model prose hallucinated content not present in the file.
- Perf: 3 provider requests, 1 shell action, 101268ms provider time, 7971 total tokens.
- JSONL: action `cat app/page.tsx` had correct `stdout_tail`; final provider text made unsupported claims.

Project tree/review:
- `show me the project tree` rendered the compact tree but leaked raw `<tool_call>` markup after the summary.
- `review this project` behaved better: inspected package/page/layout files and produced grounded concise findings in one `project_review_synthesis` request.

Build shell prompt:
- Prompt timed out at 120s in the initial `plain_chat` request before any shell action ran.
- This is a routing/latency failure, not an npm/build failure.

Linear connector status:
- Linear tool calls still return `401: Reauthentication required`, so this entry needs syncing after auth is restored.
```

Additional update/comment:

```text
Additional live dogfood evidence from 2026-06-02:

1. Advanced natural review prompt
Prompt: Review this Next.js project for production readiness. Inspect package.json, app/page.tsx, app/layout.tsx, tailwind.config.ts, next.config.mjs, and postcss.config.mjs. Give 3 concrete findings max. Do not edit files.
Result: failed. The turn stayed in plain_chat, used zero tools, and hit the 120s provider read timeout before any file inspection.

2. Short project-review fast path
Prompt: review this project
Result: passed behaviorally. Deterministic inspection read package.json, app/page.tsx, and app/layout.tsx, then one project_review_synthesis provider request produced a grounded summary.
Timing: 26.4s total provider time, 1 provider request, 4 actions, 1135 prompt tokens, 671 completion tokens, 1806 total.

3. Explicit tool build proposal
Prompt: /tool run npm run build
Result: proposed the correct shell command and waited for approval.
Timing: 12.8s in one tool_enabled request in first run; 3.1s in a repeated run.

4. Explicit tool build with approval
Prompt: /tool run npm run build, then /approve
Result: build executed successfully. npm run build exit 0 in about 4.6s. No model-authored final pass/fail summary after approval.

5. Natural build/report prompt in full_access
Prompt: Run npm run build and report the result. Do not edit files.
Result: build succeeded, but the model repeated shell commands, wrote/read /tmp/build-output.txt, then incorrectly asked the user to share output despite verified output showing EXIT_CODE: 0.
Timing: 81.0s visible turn, 11 provider requests, 8 actions, 9 tool calls, 69.4s provider time, 21,886 prompt tokens, 1,814 completion tokens, 23,700 total tokens.

Interpretation:
- Short deterministic fast paths are working but too narrow.
- Natural tool-intent prompts can either time out in plain_chat or loop through repeated tools.
- The model receives verified shell output in trace/session state, but final behavior is not reliably grounded in it.
- This reinforces ELG-361 acceptance: route tool intents earlier, prevent repeated redundant shell loops, and ground visible summaries in verified tool results.

Linear note:
- ELG-361 was created through local Linear MCP.
- Attempted to add this evidence as a Linear comment through local MCP, but the nested MCP save_comment call was cancelled before saving.
```

Implementation update/comment:

```text
First-step implementation completed: earlier routing for obvious shell tool-intent prompts.

What changed:
- Added a deterministic shell-execution fast path before the plain route classifier.
- The fast path handles command-shaped requests like `Run npm run build and report the result`.
- It routes directly into the existing shell-execution tool loop with only ask_guidance + shell_command exposed.
- It does not hardcode visible assistant prose.
- It does not cap output tokens.
- It keeps question-shaped command explanations, such as `What does cargo test do?`, on the plain-chat-first path.
- It keeps the existing project-review fast path behavior.

Files changed:
- crates/elgar-core/src/agent_loop.rs

Tests:
- cargo fmt --check
- cargo check -p elgar-core
- cargo test -p elgar-core shell_execution_fast_path_skips_plain_route_classifier
- cargo test -p elgar-core command_question_stays_plain_chat_first
- cargo test -p elgar-core completed_plan_execution_intent_does_not_skip_local_shell_work
- cargo test -p elgar-core project_review_fast_path_skips_plain_route_classifier
- cargo test -p elgar-core --test no_natural_language_trigger_tables
- cargo test -p elgar-core --lib

Live dogfood:
- Prompt: `/permissions full_access`, then `Run npm run build and report the result. Do not edit files.`
- Result: first provider request is now `tool_enabled`; the initial `plain_chat` classifier request is gone.
- Before: 81.0s visible turn, 11 provider requests, 8 actions, 9 tool calls, 69.4s provider time, 23.7k tokens.
- After: 57.9s visible turn, 8 provider requests, 6 actions, 7 tool calls, 50.1s provider time, 17.7k tokens.
- The build still succeeded, but the model still looped and asked the user to paste output despite verified shell output.

Known limitation:
- This fixes only the first step: earlier routing.
- Duplicate shell-loop control and verified-result grounding remain open under ELG-361.
```

Planning update/comment:

```text
Created/updated the detailed harness/tool-loop bottleneck analysis doc.

File:
- docs/harness-tool-loop-bottleneck-analysis.md

What it captures:
- How TUI input, AgentRuntime routing, provider requests, model tool calls, runtime validation, policy, executors, UI rendering, trace logs, and model feedback interact.
- Why the current verification layer is strong but the model feedback loop is weak.
- Current live timing evidence:
  - Before shell fast path: 81.0s, 11 provider requests, 8 actions, 9 tool calls, 69.4s provider time, 23.7k tokens.
  - After shell fast path: 57.9s, 8 provider requests, 6 actions, 7 tool calls, 50.1s provider time, 17.7k tokens.
- The main bottleneck: after shell execution, the UI and trace have verified stdout/stderr/exit-code truth, but the model often receives only generic feedback such as `Executed approved shell command and recorded the verified result.`
- Proposed next method: transactional shell execution.

Recommended next implementation:
- Add a core verified shell result digest.
- Feed that digest to the model after shell execution.
- For report-only shell commands, close the tool phase after one conclusive verified result.
- Run a no-tool `tool_result_synthesis` provider request so the final pass/fail response is still model-authored.
- Add semantic command classes only after validated shell commands exist, not from broad natural-language trigger tables.

Success target:
- `Run npm run build and report the result. Do not edit files.`
- Expected provider shape: 1 tool-enabled request, 1 local shell action, 1 no-tool synthesis request.
- Target: 2 to 3 provider requests, 1 shell action, no repeated build/probe commands, under 5k tokens for simple build/report.
```

Files changed:

```text
docs/harness-tool-loop-bottleneck-analysis.md
zz_elgar_agent_docs/LINEAR_SYNC_QUEUE.md
```

Verification:

```text
printf '%s\n' 'hello!' '/exit' | elgar tui
elgar perf-trace
printf '%s\n' 'read page.tsx' '/exit' | elgar tui
elgar perf-trace
printf '%s\n' 'show me the project tree' 'review this project' '/exit' | elgar tui
printf '%s\n' '/permissions full_access' 'Run npm run build in the current folder and report whether it passed or failed with the key reason. Do not edit files.' '/exit' | elgar tui
tail latest .elgar/traces/*.jsonl entries for verified stdout and provider_error evidence
```

Known limitations:

```text
No runtime fix implemented in this verification pass. Linear live sync failed with 401, so this is queued locally.
```

Implementation update/comment synced to Linear 2026-06-03:

```text
Transactional shell synthesis landed and verified.

What changed:
- Added turn-scoped shell transaction state for natural shell_execution report-only turns.
- Added a core verified shell result digest for model-facing context.
- For conclusive report-only shell results, Elgar stops exposing tools and switches to no-tool tool_result_synthesis.
- Final pass/fail/result prose remains model-authored.
- Explicit /tool shell behavior keeps its existing explicit completion feedback path.
- Dev-server/debug/fix/keep-going style shell turns are excluded from one-command terminal behavior.
- Raw stdout/stderr remain in verified shell results and details/raw-copy paths.
- Updated stale CLI smoke expectations for shell-shaped text under stub provider.

Primary files changed in this implementation pass:
- crates/elgar-core/src/agent_loop.rs
- crates/elgar-cli/tests/smoke.rs

Verification:
- cargo fmt --check
- cargo check -p elgar-core
- cargo test -p elgar-core --lib => 422 passed
- cargo test -p elgar-cli --test smoke tui_command_plain_shell_text -- --nocapture => 2 passed
- ./bin/check-local => passed

Live dogfood:
- Cwd: playground/Nextjs-1
- Prompt: /permissions full_access, then Run npm run build and report the result. Do not edit files.
- Result: build executed once, exit 0, then model-authored final summary.
- provider_requests: 2
- actions: 1
- tool_calls: 1
- provider_time_ms: 10063
- tokens: prompt 2347, completion 227, total 2574
- request 1: tool_enabled, tools 2, duration 3780ms, tokens 1787
- request 2: tool_result_synthesis, tools 0, duration 6283ms, tokens 787

Known limitations:
- This completes the transactional shell synthesis slice, not the entire broader ELG-361 scope.
- Read-file grounding and any remaining raw tool-markup filtering should still be reviewed under ELG-361 before moving the whole issue to Done.
```

Additional verification/comment synced to Linear 2026-06-03:

```text
I re-audited the transactional shell synthesis implementation for hardcoded visible replies and brittle report/fix phrase predicates, then ran three live TUI dogfood cases against LM Studio.

Fixes added during verification:
- Removed natural-language report/fix helper predicates from the shell transaction path.
- Changed nonzero shell exits to remain verified shell results instead of failed harness actions.
- Normalized the common EXIT_CODE=$? wrapper so masked shell wrappers preserve the underlying exit code.
- Suppressed route-control JSON if a provider returns it during tool-result synthesis.
- Tightened fake local shell success guards so route/chat repair does not surface unverified shell completion claims.

Live dogfood results from playground/Nextjs-1:
- Run npm run build and report the result. Do not edit files. => exit 0, 2 provider requests, 1 tool call, 11.3s visible response, 2.5k provider tokens.
- Run npm run lint and report the result. Do not edit files. => verified exit 1, 2 provider requests, 1 tool call, 9.6s visible response, 2.6k provider tokens.
- Run pwd and report the result. Do not edit files. => exit 0, 2 provider requests, 1 tool call, 8.1s visible response, 2.5k provider tokens.

Regression verification:
- cargo fmt --check passed.
- cargo check -p elgar-core passed.
- cargo test -p elgar-core --lib passed: 423 passed.
- cargo test -p elgar-core --test no_natural_language_trigger_tables passed: 3 passed.
- Static audit found no production hardcoded greeting/build-success strings or removed report/fix predicate helpers in the checked core/CLI paths.

Verdict: the speedup holds across success, failure, and generic stdout shell-report cases. No visible assistant response was hardcoded.
```

Correction/follow-up synced to Linear 2026-06-03:

```text
Correction after a stricter hardcoding audit:

I removed the unsafe natural-language deterministic route fast path entirely. That path was too close to a phrase table for prompts like project review/report/fix, and it is not acceptable for Elgar's model-owned behavior boundary.

Current safe behavior:
- Normal user text goes through model route again.
- The model can still choose shell_execution.
- Elgar executes the verified shell command.
- For conclusive non-dev-server shell results, Elgar switches to no-tool tool_result_synthesis.
- The final visible pass/fail/result response is still model-authored, not harness-authored.

Corrected live dogfood numbers from playground/Nextjs-1 after removing the deterministic phrase route:
- Run npm run build and report the result. Do not edit files. => exit 0, 3 provider requests, 1 tool call, 17.2s, about 2.9k provider tokens.
- Run npm run lint and report the result. Do not edit files. => exit 1, 3 provider requests, 1 tool call, 14.8s, about 3.0k provider tokens.
- Run pwd and report the result. Do not edit files. => exit 0, 3 provider requests, 1 tool call, 17.6s, about 2.9k provider tokens.

This is materially better than the old 81.0s / 11 provider requests / 9 tool calls / 23.7k token behavior, but not as extreme as the earlier unsafe 2-request numbers.

Additional fixes kept:
- Nonzero shell exits remain verified shell results instead of failed harness actions.
- Shell wrappers preserve the underlying command exit status.
- Route-control JSON is suppressed if a provider emits it during synthesis.
- Command-shaped state route without answer_kind retries routing instead of invoking the slower state classifier.
- Fake local shell-success claims are blocked unless backed by verified action truth.

Verification:
- cargo fmt --check passed.
- cargo check -p elgar-core passed.
- cargo test -p elgar-core --lib passed: 424 passed.
- cargo test -p elgar-core --test no_natural_language_trigger_tables passed: 3 passed.
- cargo test -p elgar-cli --test smoke -- --nocapture passed: 16 passed.
- ./bin/check-local passed, including fmt, workspace check, workspace tests, and clippy.
- Static hardcoding audit found the forbidden deterministic route names only inside the guard test, not production code.
```

LM Studio smart integration update synced to Linear 2026-06-03:

```text
Linear issue: ELG-361
Linear comment: 846be3f3-171a-4c9b-8694-b44e9f865005

Implemented and verified the next plan step on codex/elg-361-transactional-shell-synthesis.

What changed:
- Added request-mode provider profiles so plain_chat, tool_enabled, tool_result_synthesis, and project_review_synthesis can choose backend behavior independently.
- Added LM Studio native /api/v1/chat path for no-tool plain chat only.
- Kept tool-enabled and synthesis calls on OpenAI-compatible chat completions for correctness.
- Added provider stats plumbing into traces/perf output: backend, TTFT, tok/s, reasoning token count, request mode.
- Added config pass-through from elgar-provider.json request_modes.
- Added shell-result exact-output retry for short verified stdout.
- Fixed TUI markdown rendering that stripped double underscores from paths like /Users/yuval/__git/....
- Fixed a live synthesis regression by separating post-tool shell synthesis from the general tool-use system prompt. The final answer remains model-authored; no canned success/failure prose was added.

Live checks:
- hello!: 1 provider request, no tools, backend lm_studio_native_chat, 4.2s, provider-authored response.
- Run pwd and report the result. Do not edit files. => 3 provider requests, 1 tool call, visible model answer included exact /Users/yuval/__git/elgar/playground/Nextjs-1; no shell_command(...) leak after fix.
- Run npm run build and report the result. Do not edit files. => 3 provider requests, 1 tool call, exit 0, model-authored build summary; 24.6s total including a 3.4s build.

Verification:
- ./bin/check-local passed.
- Focused regressions passed for request-mode split, shell synthesis exact stdout retry, CLI config loading, LM Studio native formatting/parsing, TUI double-underscore path rendering, and perf trace rendering.

Known limitation:
- Qwen can still spend large reasoning tokens on the first no-tool routing/plain call. Native LM Studio improves observability and avoids tool overhead, but does not eliminate model-side reasoning latency. Reasoning controls are supported in config shape, but this loaded LM Studio model rejects explicit reasoning configuration, so it is not enabled locally.
```

Project-review synthesis loop fix pending Linear sync 2026-06-03:

```text
Linear issue: ELG-361
Linear sync: Synced 2026-06-03
Linear comment: 1f317753-98ee-4208-8c6b-e90ad5334837

Implemented and verified the JSONL-backed fix for the live project-review regression.

Problem confirmed from traces:
- Screenshot symptom was real, but JSONL showed the exact cause.
- Older trace for "what do you think about my project?" routed to execute, applied a verified project listing, then ended with only the tool result. There was no project_review_synthesis request.
- First attempted fix fired synthesis, but live trace showed 18 provider requests, 9 actions, 35 tool calls, and the final no-tool request still produced tool-call protocol text because the synthesis context included the full tool transcript.
- Next live trace showed a safer but still blocked result: provider_requests 4, actions 2, with `cat package.json 2>/dev/null || echo "NOT FOUND"` proposed for review because the shell allowlist correctly rejected redirection/control syntax.

What changed:
- Project review now stops after a bounded verified inspection pass and switches to no-tool project_review_synthesis instead of continuing the tool loop.
- The project_review_synthesis request now uses clean verified evidence only: original user request plus verified action digests. It no longer carries tool-role messages or assistant tool-call transcript messages.
- Added regression coverage that final project-review synthesis has zero tools, no tool-role messages, no assistant tool_calls, and includes VERIFIED_SHELL_RESULT evidence.
- Added regression coverage for the live loop shape: listing plus batched file reads must synthesize immediately and must not consume later repeated tool outputs.
- Extended the existing read-only cat fallback rewriter to normalize `cat package.json 2>/dev/null || echo "NOT FOUND"` to safe plain `cat package.json`, preserving strict shell allowlist behavior while avoiding a pending approval for a common read-only fallback.

Live verification:
- Prompt: `what do you think about my project?`
- Result: clean project tree, read package.json, then model-authored project review prose. No pending action, no raw tool XML.
- Latest trace: `.elgar/traces/cli-tui-29905-1780481004756-1.jsonl`
- perf-trace latest: provider_requests 5, actions 2, tool_calls 6, final request mode `project_review_synthesis`, final synthesis tools 0.
- The final synthesis request took 33.0s because Qwen spent large reasoning tokens; the harness no longer loops.

Verification:
- cargo fmt --check passed.
- cargo test -p elgar-core project_review -- --nocapture passed: 5 passed.
- cargo test -p elgar-core read_only_cat_missing_fallback_is_rewritten_before_policy -- --nocapture passed.
- cargo test -p elgar-core --test no_natural_language_trigger_tables -- --nocapture passed: 3 passed.
- ./bin/check-local passed, including fmt, workspace check, workspace tests, and clippy.

Known limitation:
- Qwen still may spend tens of seconds in the final no-tool synthesis request. This pass fixes incorrect loop/synthesis behavior and protocol leakage; it does not solve model-side reasoning latency.
```

### Harness short-term memory phase 1

Status: `In Progress`
Linear sync: `Needs Linear`
Target team: `Elgar`
Linear issue: `ELG-324`

Issue title:

Harness short-term memory: normalize duplicate primitive keys and stop duplicate loops

Issue description:

The read-only primitive harness currently depends too much on the model remembering which tools have already been used in the same turn. Live `review the project` runs showed repeated `ls .` and low-value duplicate/no-op requests. Short-term harness memory now logs same-turn inspected paths and duplicate requests, but the next phase should make duplicate handling stricter and normalized.

Update/comment:

Planned Phase 1 for short-term harness memory:

- Keep `HarnessWorkingMemory` as one-turn runtime memory owned by Elgar, not the provider.
- Normalize duplicate keys before storing/checking:
  - `./package.json` equals `package.json`
  - `app/../package.json` equals `package.json`
  - repeated slashes collapse
  - absolute paths are preserved safely after existing runtime validation
- Add duplicate stop guard:
  - after 2 duplicate/no-op requests in one turn, stop with `duplicate_loop_detected`
  - synthesize from verified evidence
- Add tests for normalized duplicates and duplicate-loop stop.
- Keep memory prompt compact:
  - already listed paths
  - already read files
  - already used find patterns
  - already used grep queries
  - duplicate/no-op requests
- Do not inject full JSONL or full evidence into decision calls.
- Keep full evidence only for synthesis/logs/details.

Acceptance criteria:

- `ls .` repeated is blocked as duplicate.
- `ls ./` and `ls .` are treated as the same.
- `read ./package.json` and `read package.json` are treated as the same.
- duplicates do not become evidence.
- duplicates do not consume evidence budget.
- after repeated duplicates, Elgar synthesizes with `duplicate_loop_detected`.
- no natural-language trigger tables.
- no macro tools.
- model still chooses primitive tools.

Pre-mortem / mitigation:

- Risk: path normalization escapes project scope.
  Mitigation: normalization is for duplicate keys only; actual execution still uses existing path validation.
- Risk: valid repeated checks are blocked later.
  Mitigation: same-turn only; memory resets between user turns.
- Risk: stopping after duplicates reduces reliability.
  Mitigation: synthesize from verified evidence and log `duplicate_loop_detected`.
- Risk: prompt grows.
  Mitigation: compact memory only; no full JSONL in decision calls.

Files likely changed:

- `crates/elgar-core/src/harness/harness_loop/state/types.rs`
- `crates/elgar-core/src/harness/harness_loop/evidence/execution.rs`
- `crates/elgar-core/src/harness/harness_loop/state/budget.rs`
- `crates/elgar-core/src/harness/harness_loop/state/memory.rs`
- `crates/elgar-core/src/harness/harness_loop/control/request_handling.rs`
- `crates/elgar-core/src/harness/harness_loop/state/logging.rs`
- `crates/elgar-core/src/harness/tests/loop_flow/primitive_loop_test.rs`
- `docs/HARNESS_SHORT_TERM_MEMORY.md`
- `crates/elgar-core/src/harness/harness_loop/state/README.md`

Verification planned:

- `cargo fmt --check`
- `cargo test -p elgar-core harness::tests::loop_flow -- --nocapture`
- `cargo test -p elgar-core harness`
- live repeated prompt test from `playground/Nextjs-1`:
  - `elgar "review the project"`
  - verify `harness_memory_snapshot`, `duplicate_loop_detected`, and no duplicate evidence items

Known limitations:

- This does not implement long-term memory.
- This does not solve model-side reasoning latency.
- This does not add macro review tools or hardcoded project-review routing.

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
