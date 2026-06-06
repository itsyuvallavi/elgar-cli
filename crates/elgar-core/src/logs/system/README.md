# System Logs

This folder owns Elgar's local system log.

The system log is a local JSONL file that explains how the runtime moved
through code during a turn. It is meant for debugging and learning the system,
not for model behavior.

The important rule:

```text
Other files decide what happened.
This folder only records the facts they pass in.
```

## Files

- `mod.rs`: small public API used by the rest of core, CLI, and TUI.
- `event.rs`: typed event data written to JSONL.
- `writer.rs`: local file path and append logic.
- `redact.rs`: safe/full detail mode helpers.

## Output

By default, logs are written under:

```text
.elgar/log/system/{session_id}.jsonl
```

The default detail mode is safe. Safe mode records counts, timings, request
modes, providers, and event names, but not full prompts or model text.
