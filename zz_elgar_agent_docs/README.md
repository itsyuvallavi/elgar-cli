# zz_elgar_agent_docs

## Purpose

Active agent instructions, roster, and planning-source guidance for Elgar v0.10
work.

## Start Here

- `AGENTS.md` is the active agent entry point.
- `AGENT_ROSTER.md` defines stable implementation and review roles.
- `ORCHESTRATOR_SITUATION_2026-05-29.md` summarizes the current checkpoint.
- `ORCHESTRATOR_SITUATION_2026-05-25.md` is older transition background.
- `GOOGLE_DRIVE_PLANNING_SOURCES.md` points to planning docs that may still be
  useful as background.

## Current Contract

Use `docs/elgar-product-architecture-plan.md` as the repo-local architecture
contract:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

Older controller-first handoffs and first-slice prompts were removed because
they conflicted with the current AgentRuntime direction.

## Ownership

Update these files only when agent operating rules or planning-source rules
change. Keep instructions short, current, and enforceable.

## Checks

- Confirm links and referenced paths still exist.
- Include agent-doc changes in the implementation report.
