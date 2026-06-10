# Elgar Project Plan

## Goal

Rebuild Elgar from the smallest useful system into a reliable coding agent.

Current baseline:

```text
user prompt -> harness loop -> provider request(s) -> verified evidence -> visible response
```

Target system:

```text
model owns intent
runtime validates
policy decides
executors verify
UI reports
tests protect
```

## Current State

Elgar is currently simplified around one harness route.

Active:

- CLI starts the app.
- TUI receives input and renders conversation state.
- Core sends harness-controlled provider requests.
- Sessions/events record turns.
- Provider config supports LM Studio.
- Local logs exist under `.elgar/log/`.
- The active harness exposes primitive read-only tools: `read`, `ls`, `find`,
  and `grep`.
- Permission policy decisions exist for risky primitives.
- Core stores one pending approval record when a risky primitive needs approval.
- Pending `write` and `edit` approvals include a target preview that shows
  relative/absolute path status and warns when the target appears outside the
  launch folder.
- Line-based CLI mode supports `/approve`, `/deny`, and `/reject`.
- Interactive terminal TUI supports `/approve`, `/deny`, and `/reject` and
  renders the current pending approval record after provider turns. Richer
  approval buttons/cards are still pending.
- Approved `bash`, `write`, and `edit` requests execute from the launch folder
  boundary and return verified execution output.
- Approved `bash` is explicit shell execution, not a sandbox. It reports the
  requested and resolved cwd before/after execution.
- The harness can batch multiple primitive read-only requests in one provider
  response through native tool calls, with JSON fallback still available.
- Native tool results return to the provider as `role:"tool"` messages, and
  normal final text ends the turn.
- Fallback synthesis remains available for safe-stop paths.
- `NATIVE_TOOL_LOOP.md` documents the active native provider tool loop and
  remaining transition gaps.

Paused or archived:

- project planning
- memory/context injection
- project-review macro routing
- old controller-style routing

## Permanent Skeleton

These pieces should stay small and stable:

- provider request/response
- session/events
- visible message rendering
- CLI/TUI input loop
- provider/model config
- local logs

The skeleton must not decide ordinary-language intent.

## Feature Re-Add Order

Add features back slowly in this order:

1. Stabilize the read-only primitive harness route.
2. Permission approval flow.
3. `bash` primitive execution.
4. `write` and `edit` primitive execution.
5. Bounded memory/context.
6. Evidence compression.
7. Planning.

Each feature needs:

- one clear entry point
- typed inputs and outputs
- tests at the boundary
- no hardcoded natural-language trigger table
- no harness-authored assistant prose for normal chat

## Execution Workflow

Use this sequence for every meaningful implementation step:

```text
inspect -> map -> plan -> pre-mortem + mitigation -> implement small slices
-> test -> update docs + Linear
```

This workflow is part of the rebuild safety model. It keeps Elgar aligned with
the target architecture before code changes land.

## Pre-Mortem

Likely failures:

- simple chat bypasses the harness
- macro tools return under a different name
- CLI/TUI starts owning runtime behavior
- provider code mixes LM Studio quirks with policy
- logs multiply into competing truth sources
- archived docs get treated as current instructions
- feature tests pass while live TUI behavior regresses

Mitigations:

- test provider payloads for the harness route
- keep startup, UI, provider, and runtime responsibilities separate
- keep active docs small
- run checks after each structural move
- archive stale docs instead of deleting useful history

## Current Next Work

1. Dogfood interactive TUI approval prompts and commands with write/edit/bash.
2. Design and add richer TUI approval buttons/cards on top of core approval
   state.
3. Review loop token/speed logs from `Nextjs-1` manual tests after the
   permissioned primitive path is stable.
