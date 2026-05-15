# Elgar v0.2 Agent Roster

## Purpose

Use a small, stable set of specialized agents for Elgar v0.2 work.

The orchestrator chooses one agent per handoff by default. Do not create a swarm or add new standing agent roles unless the project scope changes.

## Default Agents

### Core Harness Agent

Owns controller/core glue and broad Core Harness implementation tasks.

Use for:

- workspace/core integration tasks
- controller turn flow
- cross-module core behavior that does not clearly belong to a narrower agent

### Router/Session Agent

Owns deterministic routing and core-owned session state.

Use for:

- route enum and route classification
- session data shape
- event/session wiring that remains side-effect free

### Action Lifecycle Agent

Owns permissioned action types and lifecycle semantics.

Use for:

- action model
- Proposed/Approved/Applied/Rejected/Failed state rules
- rejected-action terminal behavior
- in-memory lifecycle tests

### Filesystem Safety Agent

Owns verified filesystem mutation boundaries.

Use for:

- approved WriteFile apply path
- filesystem verification
- file mutation safety tests
- preventing provider text, proposed actions, or rejected actions from writing files

### Harness/Test Agent

Owns regression tests and no-model harness coverage.

Use for:

- Core Harness regression gates
- no-model router/controller/action tests
- CLI/TUI boundary smoke tests
- test coverage before expanding behavior

### Simple TUI Agent

Owns minimal TUI work after the Core Harness is proven.

Use for:

- TUI shell layout
- rendering Core Harness events in TUI
- approval panel UI
- TUI smoke tests

### Provider / LM Studio Agent

Owns live provider work after the Core Harness and Simple TUI are stable enough.

Use for:

- LM Studio integration
- OpenAI-compatible local provider path
- provider configuration
- provider boundary tests

### Code Review Agent

Owns focused risk-gate reviews.

Use for:

- after filesystem mutation work such as ELG-122
- before completing the Core Harness slice
- before provider integration
- before TUI integration if event/rendering contracts are unclear
- whenever controller/action/session behavior becomes difficult to reason about

## Operating Rules

1. Reuse these roles instead of inventing new standing agents.
2. Prefer one implementation agent at a time.
3. Use Code Review Agent as a risk gate, not as a blocker for every small change.
4. Every implementation agent must update Linear or provide exact Linear update text.
5. Every agent report must include files changed, commands run, test results, known limitations, and the next recommended issue.
6. Every agent report must include a short plain-English explanation of what changed and why.
7. Keep reports short and direct without dropping important details.
8. Keep files small and responsibilities narrow; recommend a follow-up split when a file starts mixing concerns.
9. The orchestrator reviews each result before creating the next handoff.

## Current Recommended Flow

```text
ELG-116 Core Harness Agent
ELG-117 Core Harness Agent
ELG-118 Router/Session Agent
ELG-119 Router/Session Agent
ELG-120 Core Harness Agent
ELG-121 Action Lifecycle Agent
ELG-122 Filesystem Safety Agent
ELG-122 review Code Review Agent
ELG-123 Harness/Test Agent
Core Harness review Code Review Agent
ELG-124 to ELG-127 Simple TUI Agent
Provider / LM Studio work Provider / LM Studio Agent
```
