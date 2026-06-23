# Provider Types

This folder holds the shared provider vocabulary.

## Files

- `mod.rs` - re-exports the public provider types.
- `chat.rs` - chat roles and chat messages.
- `request.rs` - OpenAI-compatible request/response JSON shapes, including
  optional reasoning-control fields.
- `profile.rs` - backend selection, per-request profile options, and reasoning
  level/profile data.
- `metadata.rs` - provider/model/request id metadata.
- `stream.rs` - streaming chunk enum.
- `error.rs` - provider error categories and formatting.
- `tools.rs` - tool-call JSON shapes kept for future tool rebuilds.
- `controller.rs` - trait implemented by provider backends.
