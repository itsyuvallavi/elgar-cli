# Elgar Agent Instructions

## Purpose

Instructions for agents working on this repo.

Elgar is in a simplified rebuild phase. The current baseline is the new
harness route, not the full historical agent harness.

## Source Of Truth

Read these first:

```text
docs/PROJECT_PLAN.md
docs/ARCHITECTURE.md
docs/FILE_MAP.md
```

Historical docs live in:

```text
docs/archive/
```

Archive files are reference only. Do not use them as current instructions.

## Current Contract

Plain chat must stay harness-controlled:

```text
user prompt -> harness loop -> provider request(s) -> verified evidence -> visible response
```

For normal chat, Elgar must not:

- bypass the harness
- use macro tools
- send `tool_choice`
- inject unbounded or raw JSONL memory (use compact verified facts and bounded
  chat history only)
- run folder anchoring
- use hardcoded natural-language trigger tables
- write harness-authored assistant prose

Current harness capabilities:

- Native provider tool calls are the primary path.
- JSON/model-choice parsing is fallback only.
- The enabled primitive tools are `read`, `ls`, `find`, `grep`, `bash`,
  `write`, and `edit`.
- `read`, `ls`, `find`, and `grep` execute without approval.
- By default, `bash`, `write`, and `edit` require pending user approval.
- `/permissions workspace_write` allows safe relative `write` requests inside
  the launch folder to execute without approval; `bash`, `edit`, absolute
  paths, parent paths, symlink paths, and outside-folder writes still do not
  bypass approval/safety checks.
- `/permissions full_access` is explicit trusted local execution for dogfoods:
  launch-folder `write`, `edit`, and `bash` requests can execute without
  approval, while unsafe paths remain rejected by execution checks.
- `/approve` executes the current pending approval through core.
- `/approve continue` executes the current pending approval and starts one
  generic follow-up harness turn.
- `/deny` and `/reject` clear the current pending approval without execution.
- Raw chat bypass is intentionally removed; `/raw` should stay a local unknown
  command unless a new plan explicitly reintroduces it.
- `elgar logs latest` must stay a local diagnostic command and must not call the
  model.
- Durable memory indexes verified session JSONL facts and injects compact
  advisory facts plus bounded prior user/assistant turns into harness prompts.
  Assistant replay is display-only; verified facts override for file claims.

## Rebuild Direction

Keep the permanent skeleton small:

- provider request/response
- session/events
- visible message rendering
- CLI/TUI input loop
- provider/model config
- local logs

Re-add tools, permissions, shell, memory, planning, and synthesis one layer at a
time through explicit module boundaries.

## Required Execution Workflow

Every non-trivial implementation step must follow this order:

1. Inspect the current files, logs, docs, and behavior before changing code.
2. Map the current behavior against the intended target behavior.
3. Plan the change, including files to edit or add.
4. Run a pre-mortem with concrete mitigations.
5. Implement in small slices.
6. Test with focused checks and, when relevant, live CLI/TUI prompts.
7. Update docs and Linear. If Linear is unavailable, update the Linear sync
   queue or the relevant local planning doc.

Do not skip from idea directly to implementation unless the user explicitly
asks for a tiny mechanical change.

## Agent Rules

1. Work only in this repo unless the user explicitly says otherwise.
2. Do not use `docs/archive/` as source of truth.
3. Follow Linear as the execution map.
4. Do not revert unrelated dirty work.
5. Keep files small and responsibilities narrow.
6. Add headers to source files.
7. Prefer explicit types and tests over prompt-only behavior.
8. Keep CLI thin, TUI UI-focused, and core runtime-focused.
9. Explain briefly what changed after each implementation step.
10. Report tests run and known limitations.
11. Never create, move, split, or delete files without first telling the user
    what files will change, unless the user has already approved that exact
    plan.
12. Prefer primitive tools over macro tools. Do not add hidden shortcuts like
    `review_project`; let the model choose primitive tools and let Rust
    validate/execute them.
13. When comparing behavior to other coding CLIs, stay close to their native
    tool-loop pattern. Do not propose off-road architecture without a strong
    reason.
14. The user is learning Rust. Explain file responsibilities and important
    functions plainly, without assuming Rust fluency.
15. For large or risky changes, ask Cursor or another external agent to run
    dogfood tests when that would shorten feedback, but treat their output as
    evidence to verify, not truth to copy blindly.
16. If a model response looks wrong, inspect logs before guessing.
17. Avoid forcing model behavior with hardcoded natural-language trigger
    tables. Prefer protocol, typed tools, validation, and verified evidence.
18. Keep memory safe: index verified facts, keep raw logs as audit source, and
    never trust provider prose as memory truth.

## Communication

Keep messages short and direct without dropping important details.

For implementation reports, include:

- issue or task
- files changed
- what changed and why
- tests/commands
- known limitations
- next recommended step
