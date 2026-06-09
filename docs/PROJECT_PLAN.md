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
- The harness can batch multiple primitive read-only requests in one provider
  response through native tool calls, with JSON fallback still available.
- Native tool results return to the provider as `role:"tool"` messages, and
  normal final text ends the turn.
- Fallback synthesis remains available for safe-stop paths.
- `NATIVE_TOOL_LOOP.md` documents the active native provider tool loop and
  remaining transition gaps.

Paused or archived:

- permissioned tool execution
- permission policy
- shell execution
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
2. Permission gate.
3. `bash` primitive.
4. `write` and `edit` primitives.
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

1. Move the read-only harness toward the native provider tool-result loop
   described in `NATIVE_TOOL_LOOP.md`.
2. Stabilize the TUI on that harness route.
3. Review loop token/speed logs from `Nextjs-1` manual tests only after the
   native loop shape is stable.
4. Add permission boundaries before enabling `bash`, `write`, or `edit`.
5. Keep primitives small: `read`, `ls`, `find`, `grep`, then permissioned
   `bash`, `write`, and `edit`.
