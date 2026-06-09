# Harness

This folder owns Elgar's single model route.

## Current Behavior

The normal path is:

```text
caller -> harness::run_harness_turn -> harness_loop -> primitive evidence -> final answer
```

Every prompt enters the primitive harness loop. Provider calls expose the active
primitive tool schemas through `harness_tool_decision`. Native provider
`tool_calls` continue the loop; normal provider text ends the loop.

The loop does not cap useful read-only evidence by a fixed decision-call count.
It still rejects duplicate/no-op requests so the model cannot spin on the same
tool action forever.
The active executable primitives are `read`, `ls`, `find`, and `grep`.
Primitive `bash`, `write`, and `edit` are declared for the future but are not
executable yet. Permission policy decisions are logged now, but approval prompts
and side-effect execution are still future stages.
The model can request one primitive or a small batch of primitive requests in a
single provider call. Verified tool results are returned as `role:"tool"`
messages. If a text fallback response is invalid before any evidence exists,
the harness gives the model one bounded repair attempt before failing safely.

The native-loop architecture is documented in `docs/NATIVE_TOOL_LOOP.md`.
Provider-native tool calls are the primary protocol, JSON model-choice parsing
is fallback, and normal final text ends successful tool loops without default
synthesis.

## Files

- `mod.rs` exposes `run_harness_turn`, records visible session events, and runs
  the bounded primitive loop.
- `primitive_tools.rs` describes model-requestable primitive tools. It is a table
  of contents, not a router or executor.
- `provider_route.rs` names harness provider request modes so loop files do not
  hardcode backend-routing labels.
- `tool_definitions.rs` converts executable primitive tools into
  OpenAI-compatible provider tool schemas.
- `permissions/` decides whether a validated primitive may execute, needs
  approval, or must be denied.
- `context/` owns the currently executable read-only evidence collectors for
  `read`, `ls`, `find`, and `grep`.
- `harness_loop/` owns bounded multi-round harness flows.
- `model_choice.rs` exposes model-choice protocol helpers.
- `model_choice/` owns model-choice protocol details: contract rendering,
  parsing, and shared types.
- `tests/` checks harness behavior.

## Future Shape

Future harness stages should be added one primitive at a time:

- approval prompts
- write/shell execution
- bounded context
- evidence compression
- TUI rendering for richer tool progress

The harness must not become another giant agent loop. Keep each primitive tool
in a small module once it exists.
