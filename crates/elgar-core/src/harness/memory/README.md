# harness/memory

## Purpose

Build durable, compact memory from Elgar-owned session JSONL events.

## Files

- `mod.rs` exposes the memory reader and index builder.
- `types.rs` defines compact verified memory facts.
- `session_reader.rs` reads `.elgar/log/sessions/<session>.jsonl`.
- `index.rs` converts trusted harness/session events into memory facts.

## Current Scope

This is read-only infrastructure. It does not inject memory into model prompts
yet.

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
