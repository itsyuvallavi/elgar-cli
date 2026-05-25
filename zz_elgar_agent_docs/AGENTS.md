# Elgar v0.2 Agent Instructions

## Purpose

This document defines how coding agents should work on Elgar v0.2.

Elgar v0.2 is a clean restart. Do not preserve v0.1 architecture unless
explicitly instructed.

## Current Contract

Use the repo-local architecture plan as the current source of truth:

```text
docs/elgar-product-architecture-plan.md
```

The active operating contract is:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
Extensions wait.
```

The controller is no longer the conversational brain for normal CLI/TUI chat.
Normal user text should enter the AgentRuntime/model-tool path. Legacy
controller paths may remain only when explicitly named as smoke, review, or
compatibility surfaces.

## Agent Rules

1. Work only in the active repo unless the user explicitly says otherwise.
2. Do not use old/archive folders as source of truth.
3. Follow Linear as the execution map.
4. Find or create the relevant Linear issue before implementation.
5. Move the issue to In Progress before code changes.
6. Add a completion comment with files changed, tests run, and known
   limitations.
7. Move the issue to Done only after verification passes.
8. Do not commit `.DS_Store`.
9. Do not revert unrelated dirty work.
10. Keep files small and responsibilities narrow.
11. Keep normal chat model-first; slash commands remain local and explicit.
12. Keep permission, execution, and verification in runtime/core layers, not in
    UI text or provider prose.
13. Prefer explicit types, structured events, and tests over prompt-only
    behavior.
14. Add or update tests when behavior changes.
15. Report what changed, what was tested, and what is intentionally deferred.

## Communication Style

Be concise. Do not omit important facts, but avoid long narration.

For implementation reports, prefer this shape:

- issue
- files changed
- what changed and why
- tests/commands
- known limitations
- next recommended step

If a change is risky, call out the risk directly and briefly.

## File Size And Simplicity

Keep files small enough to audit quickly.

Guidelines:

- Keep CLI files thin: parse command, call runtime/core, print result.
- Keep TUI files focused on input, rendering, and slash commands.
- Keep provider HTTP/client details out of runtime policy code.
- Keep filesystem and shell execution behind typed executors.
- Split a module when it starts mixing unrelated responsibilities.
- Prefer a small follow-up issue for module splitting over broad refactors
  inside feature work.

## Agent Roster

Use the stable agent roster in:

```text
AGENT_ROSTER.md
```

Reuse standing roles rather than creating new ones. Use review agents at risk
gates, especially after filesystem/shell changes, permission-policy changes,
or live TUI path changes.

## Source Of Truth

Repo-local source of truth:

```text
docs/elgar-product-architecture-plan.md
```

Supplemental current planning reference:

```text
docs/codex-style-agent-runtime-plan.md
```

Google Drive planning docs remain useful background when available:

```text
GOOGLE_DRIVE_PLANNING_SOURCES.md
```

If Google Drive docs conflict with the repo-local architecture plan, follow the
repo-local plan and update Linear/docs with the discrepancy.

## What To Report Back

Every agent run should report:

- Linear issue worked on
- files created, modified, or deleted
- architecture decisions made
- tests added
- commands run
- what passes
- what is intentionally deferred
- any blockers or ambiguity
