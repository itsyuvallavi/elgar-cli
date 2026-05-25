# Orchestrator Situation - 2026-05-25

## Purpose

Explain the current Elgar direction to the orchestrator and ask it to align the
next work around a clear architecture:

```text
Pi-like UX
+ Codex-like coding capability
+ Elgar-owned verified trust
```

This is a planning/handoff note. It does not request implementation by itself.

## Current Read

Elgar has moved beyond the original conservative Core Harness slice.

The original planning docs still matter as the constitution:

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
Tests protect.
Extensions wait.
```

The active implementation and Linear map now point toward a newer runtime shape:

```text
Model owns intent.
Runtime validates tool calls.
Policy decides.
Filesystem/shell execute.
Executors verify.
UI reports verified events.
Tests protect the loop.
```

This is the right direction, but it creates a transition risk: Elgar is partly
between old controller-first routing and the newer model/tool AgentRuntime path.

## What Exists

Strong foundations:

- typed file and shell actions
- permission policy modes
- action gate for explicit approvals
- filesystem and shell verification
- LM Studio/OpenAI-compatible provider path
- terminal TUI
- Pi-style TUI rendering work
- selective verified memory retrieval
- no-network local checks
- many controller/action regression tests

Important current files:

- `crates/elgar-core/src/agent_loop.rs`
- `crates/elgar-core/src/policy.rs`
- `crates/elgar-core/src/action_gate.rs`
- `crates/elgar-core/src/fs.rs`
- `crates/elgar-core/src/shell.rs`
- `crates/elgar-tui/src/terminal.rs`
- `crates/elgar-tui/src/terminal/provider_task.rs`
- `docs/codex-style-agent-runtime-plan.md`
- `docs/elgar-product-architecture-plan.md`
- `docs/model-first-routing-plan.md`
- `docs/read-only-memory-context.md`

Important Linear direction:

- `ELG-304` Codex-style agent runtime migration parent
- `ELG-291` Codex-like permission policy roadmap
- `ELG-293` Move live TUI to Pi-style permissive model tool loop
- `ELG-302` Add Pi-style tool rendering layer for live TUI actions
- `ELG-303` Fix Pi-style TUI rendering when model-first project creation has interleaved failures
- `ELG-311` Codex-style golden harness and live e2e coverage
- `ELG-313` Fix Desktop project targeting and missing tool argument recovery in model runtime
- `ELG-314` Document target product architecture and implementation sequence
- `ELG-315` Harden AgentRuntime permission policy enforcement

## Main Risk

The current live runtime can become unclear if "permissive" means bypassing the
trust model.

Permissive is acceptable only when:

- the active permission mode is explicit
- policy decisions are recorded
- applied work is verified
- auto-applied work is rendered differently from user-approved work
- risky edits/deletes/shell commands are gated by the selected policy
- normal chat does not fall back into legacy natural-language controller routing

`agent_loop.rs` should be reviewed carefully because it is the normal-model/tool
execution center and has historically contained full-access/permissive behavior.

## Target Component Flow

The desired normal chat flow is:

```text
CLI/TUI
-> AgentRuntime / agent loop
-> Provider
-> Model
-> Tool calls
-> Validation
-> Permission policy
-> Filesystem/shell executor
-> Verified events
-> TUI rendering
-> Memory/checks/run summary
```

The TUI should submit text and render events. It should not infer filesystem
intent or own action routing.

The model should reason and draft tool calls. It should not own permission,
execution, or verified truth.

The runtime/core should validate tools, apply policy, execute permitted work,
and record verified results.

## Missing Layer

The Ralph-style lesson is not implemented yet as a first-class layer.

Useful future layer:

```text
Run Harness / Issue Runner
```

Responsibilities:

- select one Linear issue or run spec
- build bounded context
- run one implementation iteration
- collect actions, events, checks, and verification
- write a run ledger
- produce Linear update text
- stop

Suggested ledger shape:

```text
.elgar/runs/<run-id>/
  run.json
  summary.md
  checks.json
  actions.jsonl
```

This should not precede the AgentRuntime/harness hardening. It is a later layer
above the runtime, not a replacement for core safety.

## Recommended Orchestrator Decision

Prioritize stabilization over new features.

Recommended next sequence:

1. Harden AgentRuntime permission policy enforcement so normal chat does not behave like implicit full access.
2. Prove normal chat uses AgentRuntime/model-tool flow, not legacy Controller routing.
3. Finish or verify `ELG-311` with golden transcript tests for natural chat, folder creation, Desktop/home path handling, plan-then-implement follow-up, and clarification on ambiguity.
4. Clean UI reporting boundaries so project creation renders as one concise summary by default.
5. Only after that, create a small design issue for the Run Harness / run ledger.
6. Update Google planning docs or repo docs so the new architecture is explicit:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

## Prompt For The Orchestrator

```text
You are the Elgar orchestrator.

Goal:
Review the current Elgar v0.2 direction and align the next work around this target:

Pi-like UX + Codex-like coding capability + Elgar-owned verified trust.

Read first:
- AGENTS.md
- zz_elgar_agent_docs/AGENTS.md
- zz_elgar_agent_docs/AGENT_ROSTER.md
- zz_elgar_agent_docs/ORCHESTRATOR_SITUATION_2026-05-25.md
- docs/codex-style-agent-runtime-plan.md
- docs/elgar-product-architecture-plan.md
- docs/model-first-routing-plan.md
- docs/read-only-memory-context.md
- docs/permissioned-actions-review.md

Also check Linear:
- ELG-291
- ELG-293
- ELG-302
- ELG-303
- ELG-311
- ELG-313
- ELG-314
- ELG-315
- ELG-274
- ELG-277

Task:
Do a planning review only. Do not edit files yet.

Please answer:
1. Is the current architecture actually moving toward AgentRuntime/model-tool normal chat, or are legacy Controller paths still in the normal chat path?
2. Does the current permissive agent loop enforce permission policy clearly, or does it still behave like implicit full access?
3. What exact issue should be next after the current update: ELG-311, an AgentRuntime cleanup issue, a permission-policy hardening issue, or a Run Harness design issue?
4. What is the smallest safe sequence that gets Elgar to:
   - Pi-like natural terminal UX,
   - Codex-like tool execution,
   - verified Elgar truth and policy clarity?
5. Should the Google planning docs be updated now, or after the AgentRuntime migration stabilizes?

Constraints:
- Keep the worktree read-only for this review.
- Do not create new standing agent roles.
- Do not start MCP, Skills, Obsidian integration, or parallel agents.
- Treat Ralph as inspiration for a later Run Harness, not as a direct script to copy.
- Keep the recommendation short, concrete, and mapped to Linear.
```
