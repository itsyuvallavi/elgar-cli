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
- inject project memory
- run folder anchoring
- use hardcoded natural-language trigger tables
- write harness-authored assistant prose

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

## Communication

Keep messages short and direct without dropping important details.

For implementation reports, include:

- issue or task
- files changed
- what changed and why
- tests/commands
- known limitations
- next recommended step
