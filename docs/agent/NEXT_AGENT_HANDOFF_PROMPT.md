# Next Agent Handoff Prompt

Use this prompt when handing Elgar to another agent.

```text
You are taking over work on the Elgar repo.

Repo:
/Users/yuval/__git/elgar

Read first:
- AGENTS.md
- docs/agent/AGENTS.md
- docs/PROJECT_PLAN.md
- docs/ARCHITECTURE.md
- docs/FILE_MAP.md
- docs/HARNESS_REFACTOR_PLAN.md

Do not treat docs/archive/ as current truth. It is historical reference only.

User profile:
- Yuval is hands-on and wants to learn Rust while rebuilding Elgar.
- Explain file responsibilities and important functions in plain language.
- Do not assume Rust fluency.
- Keep answers short and direct, but do not hide important details.
- Before creating, deleting, moving, or splitting files, explain exactly what
  files will change and why.
- Always use: Inspect -> Map -> Plan -> Pre-mortem + mitigation -> Implement in
  small slices -> Test -> Update docs + Linear.

Current product direction:
- Elgar is rebuilding from a simplified baseline into a reliable local coding
  harness.
- The harness is the single route to the model.
- No direct raw-chat bypass.
- No macro tools like review_project.
- No hardcoded natural-language trigger tables.
- The model chooses primitive tools; Rust validates, checks permissions, and
  executes.

Current harness behavior:
- Normal route:
  user prompt -> harness loop -> native provider tool calls -> tool results ->
  final model text.
- Native provider tool calls are the happy path.
- JSON/model-choice parsing exists only as fallback.
- Synthesis exists only for fallback/stop paths, not normal successful turns.
- `elgar logs latest` is local and must not call the provider.
- `/raw` is intentionally removed and should remain an unknown local command.

Primitive tools:
- Read-only tools: read, ls, find, grep.
- Risky tools: bash, write, edit.
- bash/write/edit require approval.
- `/approve` executes the pending approval through core.
- `/deny` and `/reject` clear the pending approval without execution.
- Approval warnings must surface outside-folder targets.
- Approved write/edit reject symlink targets.

Memory state:
- Same-turn memory exists in `crates/elgar-core/src/harness/harness_loop/state/`.
- Durable memory slice 1 exists in `crates/elgar-core/src/harness/memory/`.
- Durable memory currently reads session JSONL and builds compact verified facts.
- Durable memory injects a bounded, verified prompt-memory view into provider
  calls.
- Prompt memory logs indexed/rendered counts, rendered chars, omitted facts, and
  budget-hit status.
- Memory must never trust provider prose.
- Raw JSONL should not be dumped into prompts.

Recent important commits:
- Split CLI diagnostics and TUI render modules.
- Remove stale pending approval restore path.
- Add durable harness memory index.
- Split provider stream event logging.
- Clean TUI live rendering and compact approval/result displays.

Testing baseline:
Run these before and after meaningful changes:
- ./bin/check-local
- cargo test -p elgar-core harness
- cargo test -p elgar-cli
- cargo test -p elgar-tui

For live dogfood, use `playground/Nextjs-1`:
- elgar "hello"
- elgar "read package.json"
- elgar "list app"
- elgar "grep for tailwind"
- elgar "review this project"
- elgar logs latest

For approval dogfood:
- Start `elgar tui`
- Ask to create a file.
- Confirm pending approval block appears.
- `/approve` should execute.
- `/deny` should not execute.

Known priorities:
1. Keep active implementation files small and split when they approach 300
   lines.
2. Keep TUI rendering clean, scrollback-safe, and evidence-backed.
3. Add repo-level MCP startup visibility and keep MCP docs aligned with the
   active tool surface.
4. Keep cleanup focused on active files; treat `_legacy/`, `docs/archive/`, and
   `docs/agent/history/` as historical references.

Rules:
- Inspect logs before guessing.
- Verify Cursor/other-agent findings before editing.
- Keep files small and add headers.
- Update Linear after meaningful work. If Linear is unavailable, write a local
  sync note.
- Do not revert unrelated changes.
- Commit stable checkpoints before risky changes.
```
