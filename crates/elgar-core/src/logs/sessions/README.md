# Session Logs

Session logs record the conversation event truth.

They answer: what happened in this session?

Examples:

- user message
- provider started
- provider finished
- assistant message
- error

## Output

```text
.elgar/log/sessions/{session_id}.jsonl
```

## Files

- `mod.rs` - public session-log API.
- `writer.rs` - JSONL event shape, path selection, and append behavior.
