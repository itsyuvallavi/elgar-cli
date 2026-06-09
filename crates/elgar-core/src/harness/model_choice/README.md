# Model Choice

This folder owns fallback model-choice parsing for provider text.

The primary primitive harness protocol is native provider tool calls. This
folder is used only when a provider/model returns text instead of native
`tool_calls`.

Files:

- `types.rs` defines model-choice result and validation types.
- `parsing.rs` owns the top-level parse flow.
- `json_extract.rs` extracts JSON from provider text and fenced blocks.
- `prose_guard.rs` rejects prose mixed with primitive-shaped protocol JSON.
- `validation.rs` validates parsed JSON against the primitive registry.
- `contracts.rs` renders model-facing contracts from the primitive tool registry.

The model owns the choice. The harness validates the choice. Primitive tool
metadata lives in `harness/primitive_tools.rs`.

## Choice Types

- `message` is allowed for one-shot model-choice diagnostics.
- `structured_request` asks Elgar for one known primitive tool.
- `structured_requests` asks Elgar for a small batch of known primitive tools.
- `answer_now` is a legacy/fallback control shape that can trigger synthesis
  when JSON fallback is still in use.

Native primitive loops accept natural prose as the normal final answer. JSON
fallback still repairs malformed structured text once when needed.
