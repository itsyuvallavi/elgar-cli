# LM Studio Legacy Test Notes

The old LM Studio tool/action tests were erased instead of kept as compiled or
archived Rust code.

Reason: those tests described the previous harness, not the raw-only system we
are rebuilding now. Keeping the old code would create noise and false failures.

The active raw-provider tests now live in:

```text
crates/elgar-core/src/provider/lm_studio/tests.rs
```

When tools are reintroduced, recreate fresh tests for these behaviors:

- tool-enabled OpenAI-compatible request formatting
- tool-definition serialization through `model_runtime`
- OpenAI tool-call parsing into raw model tool names
- malformed tool-call argument parsing
- native-backend rejection for tool-enabled requests

Those behaviors are intentionally outside the current raw-only harness.
