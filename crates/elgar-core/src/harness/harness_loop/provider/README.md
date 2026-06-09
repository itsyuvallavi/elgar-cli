# Harness Loop Provider

Owns model-provider calls made during the primitive harness loop.

## Files

- `mod.rs` exposes provider-call modules inside `harness_loop`.
- `context.rs` builds the native loop system prompt and fallback repair prompts.
- `decision.rs` sends the growing native provider conversation with tool
  schemas attached.
- `repair.rs` asks the model to repair one invalid decision response.
- `synthesis.rs` asks for a final answer with no tools exposed.

This folder should not execute primitive tools or mutate loop state directly.
Normal decision calls use native provider tool messages. Repair calls receive
compact evidence summaries only when JSON/text fallback output is malformed.
Synthesis may receive full verified evidence when the loop needs a fallback
no-tool answer.

Repair calls are format-only. Native final text is accepted as the normal
successful loop ending.
