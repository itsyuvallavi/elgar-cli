# Harness Refactor Plan

## Status

**Selective cleanup only.** Do not run a broad file refactor. Cleanup happens when it
directly supports active product work. Use the **debt inventory** below to choose
slices — not as a checklist to burn down in one pass.

**Out of scope:** `_legacy/` trees (reference only; listed for awareness, not cleanup).

---

## Current state (2026-06-10)

- Native tool loop is the primary harness route.
- `bash`, `write`, and `edit` go through pending approval; `/approve`, `/deny`, and
  `/reject` work on scripted and interactive TUI entry points.
- Broad dogfood passed (~39 tests). Remaining gaps are product/policy, not loop shape:
  - approval UX (cards/buttons, less model-authored “done” prose)
  - bash approval clarity (bash is explicit shell execution, not a sandbox)
  - absolute-path clarity in approval preview
  - token efficiency on multi-round turns

---

## Direction

1. **Cleanup only when it ships with product work** — no standalone “long file” PRs.
2. **One slice per PR** where possible; run `bin/check-local` after each slice.
3. **No behavior change** unless the slice says so and has tests.
4. Update READMEs that name touched files before merge (tick only what changed).
5. **Tick items in the debt inventory** when a slice resolves them (or mark deferred with reason).

---

## Prioritize now

| Area | What | Why |
|------|------|-----|
| **Slash commands** | Single parse/help/handler path for scripted TUI and interactive TUI | Foundation for approval buttons and consistent `/approve` behavior |
| **Path safety helpers** | Shared helpers for write/edit/bash policy (jail, symlinks, ancestors) | Dogfood found bash bypasses write/edit path guards |
| **Stale naming** | Replace inaccurate `read_only` (and similar) in modes, logs, comments | Permissions and executors landed; names should match behavior |
| **Surgical splits** | Small extractions from `coordinator.rs`, `logging.rs`, or provider call sites **only while editing those files** | Keep files navigable without a mega-PR |

### Next product work (not refactor for its own sake)

1. TUI approval card/buttons on top of core `pending_approval`.
2. Bash safety policy (same path rules as write/edit where possible).
3. Clearer approval preview: exact write/edit payload, resolved path, absolute-path warning.

---

## Defer

Do not schedule until permissioned path and TUI approval UX are stable:

- Broad file moves (`runtime` → core, `provider_visible` → `provider/visible`)
- Test reorganization (splitting large test files)
- `harness/context` → `harness/collectors` rename
- TUI render/pane/markdown splits
- CLI logs diagnostic file split
- Evidence/provider openai splits unless required by a feature PR

The old numbered phase list (0–12) is retired; use the inventory below instead.

---

## Do not do now

- No large rename-only PRs.
- No macro tools.
- No cleanup that changes runtime behavior without tests.
- No moving or splitting files just because they are long.
- No refactor batch that competes with bash safety or approval UX.

---

## Known debt inventory

**Legend:** **Now** = allowed standalone slice if small. **Pair** = only with product
work named in parentheses. **Defer** = do not schedule yet.

### Long files inventory

Scanned active `crates/**/*.rs` excluding `_legacy/` (threshold **150+ lines**).
Re-run: `find crates -name '*.rs' ! -path '*_legacy*' -print0 | xargs -0 wc -l | sort -n`

#### Tier A — 400+ lines

| Lines | File | Issue | Action |
|------:|------|-------|--------|
| 914 | `elgar-core/.../harness/tests/loop_flow/primitive_loop_test.rs` | Harness loop tests in one file | **Defer** (test reorg) |
| 749 | `elgar-tui/.../terminal/ui/render.rs` | Frame layout + draw paths | **Defer** (pane refactor) |
| 656 | `elgar-core/.../provider/lm_studio/tests.rs` | Provider integration tests | **Defer** |
| 590 | `elgar-cli/tests/smoke.rs` | CLI/TUI smoke integration | **Defer** |
| 547 | `elgar-core/.../harness/tests/model_choice/parsing_test.rs` | Model-choice parsing tests | **Defer** |
| 96 | `elgar-core/.../harness/permissions/approval_flow.rs` | Approve/deny dispatch only after split | **Done** (`approval_logging.rs` + `approval_flow_tests.rs`) |
| 498 | `elgar-core/.../harness/harness_loop/state/logging.rs` | Harness system-log helpers | **Pair** (logging touch) |
| 461 | `elgar-cli/.../diagnostics/logs.rs` | JSONL parse + human render | **Defer** (logs split) |
| 440 | `elgar-tui/.../terminal/ui/prompt.rs` | Input chrome + approval affordances | **Pair** (approval card) |
| 411 | `elgar-core/.../harness/harness_loop/control/coordinator.rs` | Native loop + synthesis entry | **Pair** (coordinator edit) |
| 400 | `elgar-tui/src/markdown.rs` | Markdown → terminal pipeline | **Defer** |

#### Tier B — 250–399 lines

| Lines | File | Issue | Action |
|------:|------|-------|--------|
| 362 | `elgar-tui/src/code_blocks.rs` | Fence detection + highlighting | **Defer** |
| 361 | `elgar-tui/.../panes/conversation.rs` | Event → visible transcript | **Pair** (approval card) |
| 351 | `elgar-core/.../provider/lm_studio/openai.rs` | Chat completions request/response | **Defer** |
| 319 | `elgar-core/.../harness/context/directory.rs` | `ls` collector + local path helpers | **Pair** (`path.rs`) |
| 316 | `elgar-core/.../harness/context/grep.rs` | `grep` collector + walk helpers | **Pair** (`path.rs`) |
| 294 | `elgar-cli/.../diagnostics/scripted_tui.rs` | Scripted TUI loop + handlers | **Done** (shared parse/help) |
| 288 | `elgar-tui/.../terminal/turn/submitted.rs` | Submit path + slash dispatch | **Pair** (approval card) |
| 273 | `elgar-core/src/event/mod.rs` | Session event types + helpers | **Watch** (split only when editing) |
| 269 | `elgar-core/.../harness_loop/evidence/execution.rs` | Read-only + pending tool execution | **Pair** (permission naming) |
| 254 | `elgar-core/.../provider/http/transport.rs` | HTTP client + timeouts | **Watch** |
| 250 | `elgar-core/.../harness/context/find.rs` | `find` collector | **Pair** (`path.rs`) |

#### Tier C — 150–249 lines (watchlist)

Split **only when editing** the file or its neighbor.

| Lines | File | Notes |
|------:|------|-------|
| 242 | `elgar-core/src/context/bundle.rs` | Prompt context bundle assembly |
| 239 | `elgar-core/src/session.rs` | Session state + `pending_approval` |
| 228 | `elgar-core/.../harness/primitive_tools.rs` | Tool registry + stage flags |
| 228 | `elgar-core/.../provider/lm_studio/mod.rs` | LM Studio provider surface |
| 218 | `elgar-core/tests/context_accounting.rs` | Integration test file |
| 217 | `elgar-tui/.../terminal/commands/clipboard.rs` | Copy/paste command path |
| 210 | `elgar-core/.../harness_loop/provider/synthesis.rs` | Final-answer synthesis call |
| 209 | `elgar-cli/src/tests/logs_test.rs` | Logs diagnostic tests |
| 204 | `elgar-tui/.../panes/event_rendering.rs` | Event → display strings (overlaps conversation pane) |
| 204 | `elgar-cli/.../startup/provider_config.rs` | Provider JSON load + validation |
| 194 | `elgar-core/.../provider/http/response.rs` | HTTP response parsing |
| 188 | `elgar-tui/.../panes/provider_reasoning.rs` | Provider reasoning pane |
| 180 | `elgar-core/.../harness/tests/context/collectors_test.rs` | Collector tests |
| 175 | `elgar-tui/.../terminal/turn/provider.rs` | Provider turn wiring |
| 174 | `elgar-core/.../provider/stub/mod.rs` | Stub provider for tests |
| 173 | `elgar-core/.../provider/config/tests.rs` | Config tests |
| 170 | `elgar-core/.../provider/lm_studio/parse.rs` | LM Studio response parse |
| 167 | `elgar-core/.../provider/config/mod.rs` | Provider config types |
| 164 | `elgar-core/.../harness/context/project_file.rs` | `read` collector | **Pair** (`path.rs`) |
| 161 | `elgar-tui/src/shell.rs` | TUI shell + harness submit |
| 159 | `elgar-tui/.../terminal/display_context/mod.rs` | Terminal display context |
| 157 | `elgar-tui/src/input.rs` | Shared input helpers |
| 155 | `elgar-core/.../harness_loop/state/memory.rs` | Loop memory / evidence caps |
| 154 | `elgar-tui/.../markdown/tests/markdown_test.rs` | Markdown tests |
| 153 | `elgar-cli/.../tests/provider_config_test.rs` | Provider config tests |
| 152 | `elgar-core/src/token_accounting.rs` | Token usage accounting |
| 151 | `elgar-tui/.../terminal/turn/provider_worker.rs` | Background provider worker |

#### `_legacy/` (awareness only — do not clean)

Largest reference files: `agent_loop/tests.rs` (~9.7k), `plan_contract.rs` (~1.9k),
`session.rs` (~1.9k), `controller_project_memory.rs` (~1.9k), `fs.rs` (~1.6k),
`memory.rs` (~1.5k). These inflate repo size but are out of scope for active cleanup.

### Duplication and overlap

| Location A | Location B | Overlap | Action |
|------------|------------|---------|--------|
| `elgar-cli/.../scripted_tui.rs` (`is_tui_*` predicates, handlers) | `elgar-tui/.../terminal/commands/parse.rs` + `turn/submitted.rs` | Slash command parse/help/approve path | **Done** for parse/help/unknown-command contract; handler shape remains CLI-local |
| `harness/context/{directory,find,grep,project_file}.rs` | Each other | `canonicalize`, `root.join`, walk/noise patterns | **Pair** (`harness/context/path.rs`) |
| `harness/permissions/approved_paths.rs` | Collector path logic above | Resolve/jail/symlink rules | **Pair** (write/edit or collector path work) |
| `approved_bash.rs` cwd visibility | `approval_flow.rs` logs | Bash runs exact `sh -c` in resolved cwd after approval | **Done** for cwd validation/visibility; no shell path jail by design |
| `harness_loop/provider/decision.rs` | `harness_loop/provider/repair.rs` | Provider call + tool schema boilerplate | **Defer** (`call_with_tools.rs`) |
| `elgar-cli/src/startup/paths.rs` | Future `elgar-core` runtime | Project root + provider config discovery | **Defer** (runtime → core) |
| Core `approve_pending_approval` / `deny_pending_approval` | TUI approval presentation | State in core; UI in panes/shell | **Pair** (approval card) |
| `panes/event_rendering.rs` | `panes/conversation.rs` | Event → string rendering | **Pair** (approval card / pane work) |
| `startup/provider_config.rs` | `provider/config/mod.rs` | Config load vs types | **Defer** (runtime → core) |
| `elgar logs latest` (`diagnostics/logs.rs`) | Session JSONL + system JSONL | Two views of turn truth | **Defer**; document contract in `LOGGING.md` when touching logs |

### Stale naming and comments

| Symbol / text | Where | Accurate replacement | Action |
|---------------|-------|----------------------|--------|
| `read_only_primitive_loop` | `harness/mod.rs` log metadata | `primitive_harness_loop` or `permissioned_primitive_loop` | **Pair** (any harness log touch) |
| `"mode": "read_only"` | `harness_loop/control/start.rs` | Reflect permissioned executors | **Pair** |
| `execute_read_only_request` | `evidence/execution.rs` | `execute_safe_primitive` or split read vs pending | **Pair** |
| “executable read-only primitives” | `harness/context/README.md`, `harness_loop/README.md`, `state/README.md` | Read-only **tools** + approval-gated risky tools | **Now** (doc pass with next harness PR) |
| “Stage 3 read-only” | `permissions/policy.rs` comment | Stage / permission model as implemented | **Pair** |
| `executable_in_stage` | `primitive_tools.rs`, tool definitions | Rename when schema stable | **Defer** |

### Naming conflicts (not bugs, but confusion)

| Name | Collision | Action |
|------|-----------|--------|
| `harness/context/` | `elgar_core::context/` (prompt bundle) | **Defer** → `harness/collectors/` |
| `harness_loop/provider/context.rs` | “context” means prompt builder | **Defer** → `prompt_builder.rs` |
| `read` tool vs “read-only stage” | Overloaded “read-only” | Fix via stale-naming pass above |

### Doc and index drift

| Doc | Issue | Action |
|-----|-------|--------|
| `docs/README.md`, `docs/FILE_MAP.md` | Said plan was “paused” | **Now** (keep in sync with this file) |
| `docs/PROJECT_PLAN.md` | May lag interactive `/approve` and dogfood | **Pair** (feature PRs) |
| `harness/**/README.md` | Read-only-era wording | **Now** on next harness slice |
| `docs/TUI.md` | Approval buttons not documented until built | **Pair** (approval card) |

### `_legacy/` (do not clean up)

Duplicated concepts live here (`shell_allowlist`, `agent_loop`, old `/tool` gate,
approval copy in `_legacy/tests`). **Do not merge or delete** as part of this plan;
use only as historical reference.

---

## Allowed slices (when paired with product work)

**Slash commands**

- Export `parse_terminal_command`, `render_terminal_help`, `render_unknown_command` from `elgar-tui`.
- Delegate `elgar-cli/.../scripted_tui.rs` to TUI command parsing and command
  message text.
- Resolves: scripted_tui parser/help duplication. Compatibility `is_tui_*`
  wrappers remain, but they no longer own a separate command table.

**Path helpers**

- New `harness/context/path.rs` (optional `noise.rs` / `walk.rs`).
- Wire `approved_paths.rs` and collectors to the same helpers when touching
  write/edit/read path behavior.
- Resolves: collector duplication and write/edit path overlap. Bash is not a
  path-based primitive and should not pretend to share a path jail.

**Surgical splits**

- Extract only what the feature needs (e.g. `native_tool_execution.rs`, `bash_policy.rs`).
- Resolves: coordinator/logging rows only for the touched module.

**Stale naming doc pass**

- Update symbols and READMEs in the same PR as behavior work; never rename-only.

---

## Per-slice verification

```sh
cargo fmt --check
cargo test -p elgar-core harness
cargo test -p elgar-cli
cargo test -p elgar-tui
./bin/check-local
# optional live:
elgar "read package.json"
elgar logs latest
```

After policy or approval changes, re-run the dogfood checklist under `/private/tmp` or
`playground/dogfood-*`.

When closing a slice, note which inventory rows it cleared in the PR description.

---

## Related docs

- `docs/PROJECT_PLAN.md` — feature order and current next work
- `docs/TUI.md` — TUI surfaces and commands
- `docs/NATIVE_TOOL_LOOP.md` — harness loop contract
- `docs/LOGGING.md` — log layers and diagnostics
- `docs/FILE_MAP.md` — file locations
