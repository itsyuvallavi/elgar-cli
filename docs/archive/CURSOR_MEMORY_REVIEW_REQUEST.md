# Cursor Review Request: Harness Memory Gaps

Status: completed and archived. The reviewed risks were addressed by bounded
verified-memory rendering, session isolation, dogfood cleanup, and JSONL
diagnostics.

## Purpose

Review the parts missed in the latest harness memory work. Do not implement
changes unless explicitly asked. Inspect, verify, and report findings with file
references, evidence, and suggested fixes.

## Context

Elgar now has durable harness memory:

- verified session JSONL facts are indexed
- bounded prior user/assistant turns are replayed
- verified memory facts are injected into harness provider prompts
- `/clear` rotates the session id

This is useful, but the current review found likely reliability and token-cost
risks that were not fully addressed.

## Current Concern

The memory feature may be durable but not bounded enough.

Main suspected issue:

```text
TUI starts with fixed session id:
terminal-tui-session

memory loader reads all memory events for that session id
renderer renders all facts
provider prompt injects all rendered facts
```

That can cause prompt growth across long sessions or repeated TUI launches.

## Files To Inspect

Core memory:

```text
crates/elgar-core/src/harness/memory/
crates/elgar-core/src/harness/harness_loop/provider/session_context.rs
crates/elgar-core/src/harness/harness_loop/state/logging/
crates/elgar-core/src/session.rs
```

TUI session lifecycle:

```text
crates/elgar-tui/src/terminal.rs
crates/elgar-tui/src/terminal/turn/submitted.rs
```

Tests:

```text
crates/elgar-core/src/harness/tests/memory/
crates/elgar-core/src/harness/tests/loop_flow/
```

Dogfood scripts:

```text
bin/dogfood-memory-recall
bin/dogfood-memory-stress
bin/README.md
```

Docs:

```text
docs/HARNESS_SHORT_TERM_MEMORY.md
docs/PROJECT_PLAN.md
docs/agent/AGENTS.md
```

## Questions To Answer

1. Is verified memory injected into provider prompts with a hard size budget?
2. Is there a per-kind cap, such as max reads, max lists, max greps, max writes?
3. Are facts ordered by recency, importance, or insertion order?
4. Does the fixed TUI session id cause old memory to appear in new launches?
5. Does `/clear` fully isolate memory after rotation?
6. Do logs show enough data to debug memory prompt growth?
7. Do tests cover long sessions with many facts?
8. Do dogfood scripts leave artifacts or require tools that may not exist?

## Expected Safe Direction

Do not dump full JSONL into prompts.

Keep JSONL as the audit source. Build a compact prompt view from it.

Recommended memory prompt rules:

```text
- include only verified facts
- cap rendered memory by total chars or estimated tokens
- cap each fact kind separately
- prefer recent facts when over budget
- log fact count and rendered memory chars
- preserve full JSONL for audit/details, not prompt injection
```

Example caps to evaluate, not blindly implement:

```text
max rendered memory chars: 2,000-4,000
max read facts: 12
max listed directories: 8
max grep/find facts: 8
max approved executions: 8
max stop reasons: 3
```

## Pre-Mortem

Potential failure modes:

- Memory grows until every prompt becomes expensive.
- Old TUI runs pollute new conversations because `terminal-tui-session` is
  reused.
- The model over-trusts stale facts from previous launches.
- A future prompt becomes slower even when the current user asks a simple
  question.
- Tests pass because they only cover tiny memory sets.
- Dogfood scripts pass locally but fail elsewhere because `rg` is unavailable.
- Dogfood scripts dirty `playground/Nextjs-1` with generated artifacts.

## Mitigations To Verify

- Add a hard rendered-memory budget before prompt injection.
- Add per-kind fact caps.
- Prefer newest facts when pruning.
- Log:
  - total indexed facts
  - rendered fact count
  - rendered memory chars
  - memory budget hit true/false
- Add tests for:
  - 100+ memory facts does not exceed prompt budget
  - `/clear` resets memory context
  - duplicated facts do not inflate memory
  - old session memory does not leak if session id changes
- Make dogfood scripts check dependencies and clean generated artifacts, or
  explicitly document retained artifacts.

## Acceptance Criteria

Cursor should return:

1. Findings with file and line references.
2. Whether the unbounded-memory concern is real.
3. Whether fixed `terminal-tui-session` is acceptable or should be changed.
4. A minimal implementation plan if fixes are needed.
5. Tests to add or update.
6. Any dogfood script cleanup needed.

Do not claim the memory system is safe only because `./bin/check-local` passes.
The review must include long-session and prompt-growth reasoning.
