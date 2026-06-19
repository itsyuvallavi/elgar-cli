# harness/memory

## Purpose

Build durable, compact memory from Elgar-owned session JSONL events.

## Files

- `mod.rs` exposes the memory reader, index builder, prompt budget, and prompt
  renderer.
- `types.rs` defines compact verified memory facts.
- `session_reader.rs` reads `.elgar/log/sessions/<session>.jsonl`.
- `index.rs` converts trusted harness/session events into memory facts.
- `budget.rs` selects prompt-useful facts under per-kind and total budgets.
- `render.rs` renders bounded verified facts for advisory prompt injection.

## Current Scope

Slice 1 builds indexes from session JSONL. Slice 2 injects compact verified
facts and bounded chat history into harness provider prompts at turn start.

Indexed facts are limited to verified Elgar events:

- files read by `read`
- directories listed by `ls`
- `find` and `grep` queries
- permission decisions
- approved executions
- harness stop reasons

The full index is audit memory. Provider prompts receive only a bounded prompt
view:

- newest useful facts by kind
- `read`, `ls`, `find`, `grep`, and approved execution facts
- no permission or stop facts
- a total rendered character budget
- an omission line when useful facts are pruned

Prompt selection is currently deterministic `recent_by_kind`: newest verified
facts are selected under per-kind caps, then pruned to the rendered character
budget. Logs include the strategy plus per-kind rendered/omitted counts so
future relevance-based selection can be compared without guessing.

## Safety

Do not index provider prose as truth. Durable memory is advisory context only;
all future tool execution must still pass runtime validation and permissions.
