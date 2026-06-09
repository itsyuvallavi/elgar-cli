# Session Logs

Session logs record the conversation event truth.

They answer: what happened in this session?

Examples:

- user message
- provider started
- provider finished
- assistant message
- error
- harness model decision
- verified harness tool result
- harness duplicate rejection
- harness memory snapshot
- harness synthesis result
- harness turn finished

Harness entries are compact durable facts. They do not store full prompts or
large evidence bodies; those stay in verified evidence/session events or system
diagnostics as appropriate.

## Output

```text
.elgar/log/sessions/{session_id}.jsonl
```

## Files

- `mod.rs` - public session-log API.
- `writer.rs` - JSONL event shape, path selection, and append behavior.
