# Elgar v0.2 New Agent Handoff - 2026-05-23

This handoff is for a new agent continuing Elgar v0.2 work in the active repo:

```text
/Users/yuval/__git/elgar
```

Do not use old/archive folders as source of truth unless the user explicitly asks.

## Read First

Read these before implementing:

- `AGENTS.md`
- `zz_elgar_agent_docs/AGENTS.md`
- `zz_elgar_agent_docs/AGENT_ROSTER.md`
- `docs/v0.2-forward-plan.md`
- `docs/local-checks.md`
- `docs/live-provider-smoke.md`
- `docs/pi-like-tui-direction.md`
- `docs/pi-like-terminal-tui-visual-spec.md`
- `docs/read-only-memory-context.md`

Use Linear as the execution map. Before implementation, find or create the relevant Linear issue, move it to In Progress, comment completion details, and move it to Done only after verification passes.

## Core Philosophy

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
Tests protect.
Extensions wait.
```

Hard boundaries:

- Provider/model text must never mutate files, approve actions, execute shell commands, or verify filesystem truth.
- UI/TUI must report controller truth, not create truth.
- All filesystem and shell mutations require typed controller-owned proposals and explicit `/approve`.
- Rejected/applied/failed actions are terminal.
- Default checks stay no-network.
- MCP, Skills, Obsidian, broad extension systems, and parallel-agent frameworks remain deferred.

## Current Branch And Worktree

Current branch:

```text
elgar-v0.2
```

As of this handoff, the worktree is dirty. Do not revert unrelated changes. `.DS_Store` may be modified and must not be committed.

Known modified/untracked paths include recent v0.2 work across:

- `README.md`
- `bin/install-local`
- `crates/elgar-cli/src/lib.rs`
- `crates/elgar-cli/src/main.rs`
- `crates/elgar-cli/tests/smoke.rs`
- `crates/elgar-core/src/action.rs`
- `crates/elgar-core/src/controller.rs`
- `crates/elgar-core/src/provider/lm_studio.rs`
- `crates/elgar-core/src/router.rs`
- `crates/elgar-core/src/session.rs`
- `crates/elgar-core/tests/core_harness_regression.rs`
- `crates/elgar-tui/src/lib.rs`
- `crates/elgar-tui/src/markdown.rs`
- `crates/elgar-tui/src/memory.rs`
- `crates/elgar-tui/src/panes.rs`
- `crates/elgar-tui/src/reasoning.rs`
- `crates/elgar-tui/src/startup.rs`
- `crates/elgar-tui/src/terminal.rs`
- `crates/elgar-tui/src/terminal/commands.rs`
- `crates/elgar-tui/src/terminal/prompt.rs`
- `crates/elgar-tui/src/terminal/tests.rs`
- `crates/elgar-tui/tests/smoke.rs`
- `docs/live-provider-smoke.md`

Before changing files, run:

```sh
git status --short
git diff --stat
```

If asked to commit, stage only intended files and never stage `.DS_Store`.

## Current Product State

Elgar v0.2 is now a usable local-first Rust agent harness with:

- controller-owned route/session/action/event truth
- typed permissioned actions for create/edit/overwrite/delete/move/directory and shell execution
- approval-gated mutation through `/approve` and `/reject`
- filesystem verification after mutation
- LM Studio/OpenAI-compatible local provider support through `elgar-provider.json`
- terminal TUI launched by installed `elgar`
- no-network default verification through `./bin/check-local`
- local performance baseline through `./bin/perf-baseline`
- read-only local memory notes from `.elgar/memory/*.md`
- controller-owned project memory for verified folders, verified plans, and structured plan execution
- `/memory` command for low-latency inspection of current-session verified memory

The user is currently prioritizing:

1. flawless memory
2. low latency
3. low token use
4. core functionality that can build bigger projects through controller-owned plans

Do not jump to MCP/skills. The user explicitly said those can wait.

## Recent Linear Progress

The latest Linear state was checked on 2026-05-23.

Important Done issues:

- `ELG-116` Create clean v0.2 workspace and core skeleton.
- `ELG-117` Define core event types.
- `ELG-118` Define router enum and basic routes.
- `ELG-119` Define minimal session state.
- `ELG-120` Implement controller turn flow with provider stub.
- `ELG-121` Define action model and lifecycle.
- `ELG-122` Implement approved WriteFile apply path.
- `ELG-123` Add no-model Core Harness regression tests.
- `ELG-133` Harden WriteFile path policy against symlink escapes.
- `ELG-134` Encapsulate session mutation before broader UI/provider expansion.
- `ELG-141` Document live provider smoke commands and no-network guardrail.
- `ELG-195` Support model-proposed Markdown WriteFile plans.
- `ELG-196` Add dogfood regressions for live TUI provider flows.
- `ELG-197` Add controller-backed context accounting.
- `ELG-198` Add bounded context selection and token budget trimming.
- `ELG-199` Add no-network local performance baselines.
- `ELG-205` Define WriteFile overwrite and parent-directory policy.
- `ELG-214` Define expanded permissioned action capability model.
- `ELG-215` Implement approved file edit and overwrite actions.
- `ELG-216` Implement approved delete, move, and directory actions.
- `ELG-217` Define permissioned shell command action model.
- `ELG-218` Implement approved shell command execution.
- `ELG-225` Add read-only local memory context source.
- `ELG-228` Include recent conversation turns in provider prompts.
- `ELG-229` Route natural folder creation requests to `CreateDirectory`.
- `ELG-237` Route user filesystem tasks to approved shell commands with verification.
- `ELG-238` Add CI and README for local development.
- `ELG-246` Implement controller-owned batch project execution from approved plans.
- `ELG-247` Add controller memory for project targets and structured plan execution.
- `ELG-248` Harden controller project memory before larger builds.
- `ELG-249` Add memory inspection command with low-latency token discipline.

Known Backlog issues:

- `ELG-234` Add Pi-like structured task blocks and expandable plan previews.
- `ELG-172` Add live terminal TUI smoke checklist for thinking then response.
- `ELG-166` Quiet terminal provider progress into Pi-like thinking/result display.

Future provider issue:

- `ELG-106` Evaluate oMLX as optional local provider backend. Do not prioritize this now.

Recommended next Linear issue to create or work on:

```text
Add selective verified memory retrieval for provider prompts
```

Suggested scope:

- Feed provider turns only the smallest relevant verified memory facts.
- Keep `/memory` inspection separate from prompt injection.
- Prefer deterministic controller retrieval over model guessing.
- Include current verified folder/plan/project facts only when the user prompt needs them.
- Mark stale/missing paths before including them.
- Add no-network tests for reference resolution, stale memory exclusion, and bounded prompt size.
- Do not add vector search, MCP, skills, or long-term semantic memory.

## Live Dogfood Lessons

User dogfooding exposed these product requirements:

- The model must remember recent conversation and references like `that folder`.
- When the user asks to create folders/files, Elgar must propose real controller actions, not just print shell snippets.
- `/approve` must work after proposed actions. If there is no pending action, that is a bug in routing/proposal creation for that flow.
- The terminal TUI must not reformat completed responses with huge gaps after streaming.
- Live preview and final transcript should share the same compact Markdown rendering path.
- Thinking display should be concise and should not include filler like `Need brief`, `Responding shortly`, or hidden chain-of-thought labels.
- The footer should stay stable and Pi-like: repo/path/branch on the left, model on the right. Misleading `context: ~178 tokens` was removed.
- Native terminal scrolling and text selection are non-negotiable.
- Esc must not exit the app.
- UI/input/footer must remain visible during provider work.

## Important Files

Core:

- `crates/elgar-core/src/controller.rs`
- `crates/elgar-core/src/router.rs`
- `crates/elgar-core/src/session.rs`
- `crates/elgar-core/src/action.rs`
- `crates/elgar-core/src/fs.rs`
- `crates/elgar-core/src/context.rs`
- `crates/elgar-core/src/provider/lm_studio.rs`
- `crates/elgar-core/tests/core_harness_regression.rs`
- `crates/elgar-core/tests/context_accounting.rs`

CLI:

- `crates/elgar-cli/src/lib.rs`
- `crates/elgar-cli/src/main.rs`
- `crates/elgar-cli/tests/smoke.rs`

TUI:

- `crates/elgar-tui/src/terminal.rs`
- `crates/elgar-tui/src/terminal/commands.rs`
- `crates/elgar-tui/src/terminal/prompt.rs`
- `crates/elgar-tui/src/terminal/tests.rs`
- `crates/elgar-tui/src/markdown.rs`
- `crates/elgar-tui/src/panes.rs`
- `crates/elgar-tui/src/reasoning.rs`
- `crates/elgar-tui/src/memory.rs`
- `crates/elgar-tui/src/startup.rs`

Scripts and docs:

- `bin/install-local`
- `bin/check-local`
- `bin/perf-baseline`
- `docs/live-provider-smoke.md`
- `docs/local-checks.md`
- `docs/performance-baselines.md`
- `docs/read-only-memory-context.md`
- `docs/pi-like-terminal-tui-visual-spec.md`

## `/memory` Current Behavior

`/memory` is local. It must not call LM Studio, mutate files, or alter controller truth.

Implemented behavior:

- Empty session memory renders:

```text
Memory
(empty)
```

- Verified folders show path and `ok`/`missing`.
- Verified plans show path, project root, source action id, and `ok`/`missing`.
- Structured plans show proposed/executed state, source action id, plan path freshness, root freshness, and present/expected directory/file counts.

Key implementation:

- `crates/elgar-tui/src/memory.rs`
- `crates/elgar-tui/src/terminal/commands.rs`
- `crates/elgar-tui/src/terminal.rs`
- `crates/elgar-cli/src/lib.rs`

Known limitation:

- This is current-session memory inspection only. It is not long-term semantic memory and it does not automatically inject all memory into provider prompts.

## Verification Baseline

Before handing off implementation, run:

```sh
cargo fmt --check
cargo check --workspace
cargo test --workspace
./bin/check-local
```

For local install and manual dogfood:

```sh
./bin/install-local
elgar
```

Optional live LM Studio smoke requires LM Studio running with the configured model:

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
cargo run -p elgar-cli -- controller-smoke "Say hello in one sentence."
```

Do not make live LM Studio required for default verification.

Last known verification for `ELG-249`:

- `cargo fmt --check` passed.
- `cargo check --workspace` passed.
- `cargo test --workspace` passed.
- `./bin/check-local` passed.
- `./bin/install-local` passed.
- Installed `elgar tui` smoke with `/memory` then `/exit` passed.

## Recommended Next Work

Focus on memory/core hardening, not new surface area.

Suggested next slice:

1. Create or update a Linear issue for selective verified memory retrieval.
2. Add a small controller-owned retrieval function that picks only relevant verified memory facts for a prompt.
3. Keep retrieval deterministic and bounded.
4. Exclude stale/missing references unless the user is explicitly asking about missing/stale state.
5. Feed the selected facts into provider prompt construction with tests proving token discipline.
6. Add TUI/CLI smoke coverage showing a follow-up like `execute it inside that folder` works after longer conversation.

Acceptance criteria for the next slice:

- No provider call needed for retrieval tests.
- Prompt context grows only by a small bounded memory block.
- The model can resolve recent verified project targets without re-reading full transcript.
- Missing paths are not silently treated as valid.
- Existing tests and `./bin/check-local` pass.

## New Agent Operating Prompt

Copy this into the next agent:

```text
You are the Elgar v0.2 Continuation Agent.

Work only in the active repo:

/Users/yuval/__git/elgar

Do not use old/archive folders as source of truth unless explicitly asked.

Read first:
- AGENTS.md
- zz_elgar_agent_docs/AGENTS.md
- zz_elgar_agent_docs/AGENT_ROSTER.md
- zz_elgar_agent_docs/HANDOFF_2026-05-23_NEW_AGENT.md
- docs/v0.2-forward-plan.md
- docs/local-checks.md
- docs/live-provider-smoke.md
- docs/pi-like-tui-direction.md
- docs/pi-like-terminal-tui-visual-spec.md
- docs/read-only-memory-context.md

Core philosophy:
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
Tests protect.
Extensions wait.

Current branch: elgar-v0.2.

The working tree may be dirty from recent v0.2 work. Do not revert unrelated changes and do not commit .DS_Store.

Use Linear as the execution map:
- Find or create the relevant issue before implementation.
- Move it to In Progress before code changes.
- Add a completion comment with files changed, tests run, and known limitations.
- Move it to Done only after verification passes.

Current priority:
Harden memory/core so Elgar can reliably build larger projects with low latency and low token cost.

Recommended next issue:
Add selective verified memory retrieval for provider prompts.

Scope:
- Feed provider turns only the smallest relevant verified memory facts.
- Keep /memory as local inspection; do not dump all memory into prompts.
- Use deterministic controller-owned retrieval, not model guessing.
- Include current verified folder/plan/project facts only when the user prompt needs them.
- Mark or exclude stale/missing paths.
- Add no-network tests for reference resolution, stale memory exclusion, and bounded prompt size.
- Do not add MCP, skills, vector search, Obsidian, or long-term semantic memory in this slice.

Non-negotiables:
- /approve and /reject remain slash-only.
- Provider prose cannot mutate files or actions.
- Filesystem confirms before UI claims success.
- Native terminal scrolling and text selection must remain intact.
- Live LM Studio checks are optional/manual; default checks stay no-network.

Verification to run:
- cargo fmt --check
- cargo check --workspace
- cargo test --workspace
- ./bin/check-local

Report back:
- Linear issue updated.
- Files changed.
- What changed and why.
- Verification commands and results.
- Known limitations.
- Recommended next issue.
```
