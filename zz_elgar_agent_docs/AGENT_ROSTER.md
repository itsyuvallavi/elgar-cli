# Elgar v0.10 Agent Roster

## Purpose

Use a small, stable set of specialized agents for Elgar v0.10 work.

The orchestrator chooses one agent per handoff by default. Do not create a
swarm or add new standing agent roles unless the project scope changes.

## Default Agents

### AgentRuntime Agent

Owns the normal model/tool turn loop.

Use for:

- AgentRuntime and agent loop behavior
- model tool-call validation
- turn lifecycle events
- keeping normal CLI/TUI chat out of legacy controller routing

### Policy And Action Gate Agent

Owns permission modes and explicit review behavior.

Use for:

- `review_all`, `auto_create_review_modify`, `workspace_write`, and
  `full_access` behavior
- policy-approved versus user-approved action records
- `/approve` and `/reject` lifecycle semantics
- terminal state for rejected, failed, and applied actions

### Executor Safety Agent

Owns verified filesystem and shell execution boundaries.

Use for:

- file and directory mutation safety
- shell execution safety
- path resolution and allowed-root checks
- verification result shape
- preventing provider prose from proving filesystem or shell truth

### Provider Runtime Agent

Owns LM Studio/OpenAI-compatible provider behavior.

Use for:

- provider request/response parsing
- streaming and reasoning-field handling
- provider configuration
- provider boundary tests

### TUI Agent

Owns terminal UI behavior.

Use for:

- chat rendering
- tool progress summaries
- action panel rendering
- input behavior and slash commands
- TUI smoke tests

### Harness/Test Agent

Owns regression tests and no-network coverage.

Use for:

- golden transcript tests
- fake-provider runtime tests
- CLI/TUI boundary smoke tests
- local check coverage before risky changes land

### Memory/Context Agent

Owns bounded, verified context and memory work.

Use for:

- local context selection
- read-only memory behavior
- future `USER.md` / `MEMORY.md` design
- avoiding transcript dumps and prompt poisoning

### Documentation Agent

Owns agent docs, architecture docs, and cleanup passes.

Use for:

- keeping docs aligned with implemented behavior
- deleting stale duplicate docs
- updating handoffs and planning indexes
- summarizing architecture decisions

### Code Review Agent

Owns focused risk-gate reviews.

Use for:

- after filesystem or shell mutation work
- after permission-policy changes
- after live TUI path changes
- when runtime/action/session behavior becomes hard to reason about

## Operating Rules

1. Reuse these roles instead of inventing new standing agents.
2. Prefer one implementation agent at a time unless the user asks for
   parallel/subagent work.
3. Use Code Review Agent as a risk gate, not as a blocker for every small
   change.
4. Every implementation agent must update Linear or provide exact Linear update
   text.
5. Every agent report must include files changed, commands run, test results,
   known limitations, and the next recommended issue.
6. Every agent report must include a short plain-English explanation of what
   changed and why.
7. Keep reports short and direct without dropping important details.
8. Keep files small and responsibilities narrow; recommend a follow-up split
   when a file starts mixing concerns.
9. The orchestrator reviews each result before creating the next handoff.

## Current Recommended Flow

Use Linear as the execution map. For the current architecture migration, prefer:

```text
AgentRuntime Agent
Policy And Action Gate Agent
Executor Safety Agent
TUI Agent
Harness/Test Agent
Code Review Agent
```

Documentation and memory/context work should remain separate from core runtime
changes unless the issue explicitly combines them.
