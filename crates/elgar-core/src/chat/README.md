# Chat

This folder owns Elgar's model conversation turns.

Right now it only contains the raw no-tool chat path:

- one user message goes to the provider
- no tools are attached
- no permission system runs
- no planning/memory/workflow routing runs
- the final provider output is recorded into the session

## Future Shape

This folder should grow into the full chat layer, one explicit capability at a
time.

Expected direction:

```text
chat/
  mod.rs              # public chat API
  blocking.rs         # current raw blocking turn
  streaming.rs        # current raw streaming turn
  context.rs          # future context attachment
  tools.rs            # future typed tool-call chat
  memory.rs           # future memory-aware chat
  permissions.rs      # future policy-aware tool flow
  tests.rs            # chat behavior tests
```

The important rule: plain chat stays plain unless a future module explicitly
adds context, tools, memory, or permissions. We should not recreate one giant
agent loop here.

Files:

- `blocking.rs` waits for the complete provider response.
- `streaming.rs` forwards provider chunks while the response is being built.
- `mod.rs` exports the current public chat functions and result type.
- `tests.rs` verifies the current raw path does not attach tools.
