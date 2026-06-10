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
  - bash safety (symlink/traversal bypass via shell)
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

### Long files (active code, ≈400+ lines)

| Lines | File | Issue | Action |
|------:|------|-------|--------|
| 914 | `harness/tests/loop_flow/primitive_loop_test.rs` | Hard to navigate | **Defer** (test reorg) |
| 749 | `elgar-tui/src/terminal/ui/render.rs` | Large render surface | **Defer** (pane refactor) |
| 656 | `provider/lm_studio/tests.rs` | Large provider tests | **Defer** |
| 590 | `elgar-cli/tests/smoke.rs` | Broad integration tests | **Defer** |
| 547 | `harness/tests/model_choice/parsing_test.rs` | Large unit tests | **Defer** |
| 498 | `harness/harness_loop/state/logging.rs` | Many log helpers in one file | **Pair** (logging touch) |
| 461 | `elgar-cli/src/diagnostics/logs.rs` | Parse + render combined | **Defer** (logs split) |
| 445 | `harness/permissions/approval_flow.rs` | Approval + tests + logging | **Pair** (approval UX / bash policy) |
| 440 | `elgar-tui/src/terminal/ui/prompt.rs` | Large prompt UI | **Pair** (approval card) |
| 411 | `harness/harness_loop/control/coordinator.rs` | Loop + native + synthesis paths | **Pair** (coordinator edit) |
| 400 | `elgar-tui/src/markdown.rs` | Markdown pipeline | **Defer** |
| 362 | `elgar-tui/src/code_blocks.rs` | Syntax/block rendering | **Defer** |
| 361 | `elgar-tui/src/panes/conversation.rs` | Pane + event rendering | **Pair** (approval card) |
| 352 | `elgar-cli/src/diagnostics/scripted_tui.rs` | Duplicated slash logic | **Done** (delegates to shared TUI parser/help text) |
| 351 | `provider/lm_studio/openai.rs` | Provider HTTP + parse | **Defer** |
| 319 | `harness/context/directory.rs` | Collector + local path helpers | **Pair** (`path.rs` extract) |
| 316 | `harness/context/grep.rs` | Same | **Pair** (`path.rs` extract) |
| 250 | `harness/context/find.rs` | Same | **Pair** (`path.rs` extract) |

Files in the 230–290 line range (`session.rs`, `event/mod.rs`, `evidence/execution.rs`,
`submitted.rs`, `transport.rs`, etc.) are watchlist only — split **only when editing**.

### Duplication and overlap

| Location A | Location B | Overlap | Action |
|------------|------------|---------|--------|
| `elgar-cli/.../scripted_tui.rs` (`is_tui_*` predicates, handlers) | `elgar-tui/.../terminal/commands/parse.rs` + `turn/submitted.rs` | Slash command parse/help/approve path | **Done** for parse/help/unknown-command contract; handler shape remains CLI-local |
| `harness/context/{directory,find,grep,project_file}.rs` | Each other | `canonicalize`, `root.join`, walk/noise patterns | **Pair** (`harness/context/path.rs`) |
| `harness/permissions/approved_paths.rs` | Collector path logic above | Resolve/jail/symlink rules | **Pair** (bash safety + path helpers) |
| *(missing)* bash cwd/path policy | `approved_paths.rs` | Bash runs any `sh -c` in `session.cwd` | **Pair** (bash safety — product, not dedupe) |
| `harness_loop/provider/decision.rs` | `harness_loop/provider/repair.rs` | Provider call + tool schema boilerplate | **Defer** (`call_with_tools.rs`) |
| `elgar-cli/src/startup/paths.rs` | Future `elgar-core` runtime | Project root + provider config discovery | **Defer** (runtime → core) |
| Core `approve_pending_approval` / `deny_pending_approval` | TUI approval presentation | State in core; UI in panes/shell | **Pair** (approval card) |
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
- Wire `approved_paths.rs` and bash policy to the same helpers.
- Resolves: collector duplication; bash/write/edit path overlap.

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
