# Harness Loop

This folder owns multi-round harness flows.

## Folders

- `mod.rs` exposes loop entry points.
- `control/` owns loop order, request handling, and finish paths.
- `provider/` owns provider decision and synthesis calls.
- `evidence/` converts validated primitive requests into verified evidence.
- `state/` owns budgets, logging, and shared result/evidence types.

## Rules

- Keep loop files small.
- Do not run shell commands here.
- Do not write files here.
- Do not add permissions here.
- Let the model choose primitive tools; Rust only validates and executes.

## Current Loop

`control/coordinator.rs` currently supports executable read-only primitives:

- `read`
- `ls`
- `find`
- `grep`
- final model answer

`bash`, `write`, and `edit` are visible risky primitives. The loop itself still
does not run side effects. Risky requests create verified permission evidence
and a pending approval record. Approved `bash`, `write`, and `edit` execution
happens through the core approval command path after the loop, not directly from
coordinator code.

It logs every round and provider call so we can inspect what happened later.

## Decision vs Synthesis

Tool decision mode uses provider request mode `harness_tool_decision`. It
attaches provider schemas for enabled primitive tools and sends the growing
native provider conversation. When the model returns native `tool_calls`, Elgar
validates and executes them, then appends matching `role:"tool"` result messages
before asking the provider again.

Normal final text is the successful loop ending, even after verified evidence
exists. This matches the Codex/Pi/Claude-style loop: tool calls continue the
turn, text ends it.

Synthesis mode uses provider request mode `harness_synthesis`. It does not
expose tools. It receives only the original user request, selected verified
evidence blocks, and a stop reason, then asks the model to answer now.

Synthesis is now a fallback path, not the default successful finish. It remains
available for duplicate-loop stops, legacy `answer_now` JSON fallback, and other
explicit safe-stop cases.

Native tool results carry bounded verified evidence back to the provider as
tool messages. Full evidence is retained locally and remains available for
synthesis, logs, and later retrieval flows.

When a turn includes side effects or command executions, tool messages also
carry a compact verified action timeline. The timeline is derived from evidence
only and keeps failures, fixes, and successful reruns visible for later
decisions and final answers without adding framework-specific behavior.

Same-turn memory also keeps capped visible dirs/files from verified `ls`
results. If the model repeats the same listing, Elgar feeds back the known child
paths so the next decision can inspect a more specific path, read a visible
file, search, or answer from existing evidence.

If the model sends natural prose, Elgar accepts it as the final answer. If the
provider sends text that is valid fallback control JSON, Elgar can still execute
that fallback request. If the text looks like malformed control JSON after
evidence exists, Elgar treats it as final text instead of spending a repair call.

If the model sends malformed JSON before evidence exists, Elgar gives it one
bounded repair call. If the repair succeeds, the loop continues with the
repaired choice. If repair also fails, Elgar returns a validation error.

Repair prompts are strict protocol prompts for malformed JSON/text fallback.
Native tool-call output does not need the JSON repair path.

The loop does not cap useful read-only evidence by item count, byte count, or
primitive type. It still rejects duplicate evidence inside one turn, and the
second consecutive duplicate stops with `duplicate_loop_detected` so the
provider cannot spin on the same no-op request forever. Useful evidence resets
that duplicate streak.

Side-effect duplicate keys are state-aware. `write` and `edit` include a stable
argument fingerprint, while `bash` includes the current file-mutation epoch so
the same verification command can run again after a verified file change.

For MCP, duplicate detection includes a stable fingerprint of the MCP argument
object. This blocks exact repeated calls while allowing refined searches through
the same server/tool pair.
