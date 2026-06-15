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
- Core stores one pending approval record when risky primitive execution needs
  approval. The record may hold one exact action or a small serial batch of
  exact risky actions from one provider response.
- `/permissions workspace_write` is an explicit mode for project generation:
  safe relative `write` requests inside the launch folder can execute without
  approval, while `bash`, `edit`, absolute paths, parent paths, symlink paths,
  and outside-folder writes remain approval-gated or rejected by execution
  checks.
- `/permissions full_access` is an explicit trusted local mode for dogfoods:
  launch-folder `write`, `edit`, and `bash` requests can execute without
  approval, while unsafe paths remain rejected by execution checks.
- Pending `write` and `edit` approvals include a target preview that shows
  relative/absolute path status and warns when the target appears outside the
  launch folder.
- Line-based CLI mode supports `/approve`, `/approve continue`, `/deny`, and
  `/reject`.
- Interactive terminal TUI renders the current pending approval record after
  provider turns and exposes keyboard-first `[Approve]` / `[Deny]` controls.
  `/approve`, `/deny`, and `/reject` remain command fallbacks.
- Approved `bash`, `write`, and `edit` requests execute from the launch folder
  boundary and return verified execution output. Approved batches execute the
  stored steps serially after one approval and log each step.
- Approved `bash` is explicit shell execution, not a sandbox. It reports the
  requested and resolved cwd before/after execution.
- The harness can batch multiple primitive read-only requests in one provider
  response through native tool calls, with JSON fallback still available.
  Multiple risky native tool calls in one provider response become one pending
  batch approval instead of duplicate rejection. The current batch policy limit
  is 8 primitive tool calls per provider response.
- Native tool results return to the provider as `role:"tool"` messages, and
  normal final text ends the turn.
- Fallback synthesis remains available for safe-stop paths.
- Cross-turn harness memory keeps full compact JSONL facts as audit truth, but
  injects only a bounded prompt view plus bounded prior user/assistant turns.
  `/clear` and `/new` reset core conversation state and rotate the session id.
- Provider final prose is guarded so local project action claims cannot become
  visible truth without verified same-turn evidence; the first blocked claim
  gets one generic corrective retry before the turn stops.
- Provider prose that asks for approval to use read-only inspection primitives
  is also guarded and retried once, because `read`, `ls`, `find`, and `grep`
  execute without approval.
- Provider prose that asks the user to approve a risky action is guarded unless
  core has a real pending approval record. Prose alone must not make `/approve`
  appear actionable.
- Explicit primitive target fidelity is validated before execution for narrow
  direct file-open requests and user-language text search requests such as
  `search for <query> in <file>`; obvious argument mismatches are rejected and
  retried.
- `NATIVE_TOOL_LOOP.md` documents the active native provider tool loop and
  remaining transition gaps.

Paused or archived:

- project planning
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

1. Execute the clean terminal UI redesign plan in
   `docs/TUI_CLEAN_REDESIGN_PLAN.md`.
2. Keep native scrollback and text selection intact while improving response,
   reasoning, command, file-tree, approval, and footer rendering.
3. Verify each TUI slice with focused tests, then run Cursor dogfood for
   project generation, approval, `/cancel`, and `/details last`.
