# Logs

This folder owns Elgar's local JSONL logs.

There are two log types:

- `sessions/` - conversation/session events.
- `system/` - runtime flow, timing, and diagnostic events.

## Output Folders

```text
.elgar/log/sessions
.elgar/log/system
```

## Shared Helpers

`common.rs` holds duplicated low-level helpers:

- environment flag parsing
- timestamp creation
- safe filename component creation
- JSONL append behavior

The session and system logs decide what data to write. `common.rs` only handles
shared mechanics.
