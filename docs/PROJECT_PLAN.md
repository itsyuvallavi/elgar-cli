# Elgar Project Plan

## Goal

Rebuild Elgar from the smallest useful system into a reliable coding agent.

Current baseline:

```text
user prompt -> provider request -> model answer -> visible response
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

Elgar is currently simplified around raw chat.

Active:

- CLI starts the app.
- TUI receives input and renders conversation state.
- Core sends no-tool provider requests.
- Sessions/events record turns.
- Provider config supports LM Studio.
- Local logs exist under `.elgar/log/`.

Paused or archived:

- tool execution
- permission policy
- shell execution
- project planning
- memory/context injection
- synthesis/project review
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

1. Structured logging and trace visibility.
2. Tool-call parsing with no execution.
3. Permission gate.
4. One read-only inspection tool.
5. Write tools.
6. Shell execution.
7. Bounded memory/context.
8. Planning.
9. Synthesis and project review.

Each feature needs:

- one clear entry point
- typed inputs and outputs
- tests at the boundary
- no hardcoded natural-language trigger table
- no harness-authored assistant prose for normal chat

## Pre-Mortem

Likely failures:

- simple chat gets tools or memory again
- CLI/TUI starts owning runtime behavior
- provider code mixes LM Studio quirks with policy
- logs multiply into competing truth sources
- archived docs get treated as current instructions
- feature tests pass while live TUI behavior regresses

Mitigations:

- test provider payloads for plain chat
- keep startup, UI, provider, and runtime responsibilities separate
- keep active docs small
- run checks after each structural move
- archive stale docs instead of deleting useful history

## Current Next Work

1. Finish root/docs/bin cleanup.
2. Review `bin/` scripts and archive stale dogfood scripts.
3. Clean remaining dead code warnings.
4. Review the raw-chat provider path before adding any new capability.
