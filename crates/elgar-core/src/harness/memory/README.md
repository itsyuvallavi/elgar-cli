# harness/memory

## Purpose

Build durable, compact memory from Elgar-owned session JSONL events.

## Files

- `mod.rs` exposes the memory reader, index builder, and prompt renderer.
- `types.rs` defines compact verified memory facts.
- `session_reader.rs` reads `.elgar/log/sessions/<session>.jsonl`.
- `index.rs` converts trusted harness/session events into memory facts.
- `render.rs` renders compact verified facts for advisory prompt injection.

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

## Safety

Do not index provider prose as truth. Durable memory is advisory context only;
all future tool execution must still pass runtime validation and permissions.
