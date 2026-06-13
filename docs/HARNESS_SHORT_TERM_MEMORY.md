# Harness Short-Term Memory

## Purpose

Design notes for adding reliable short-term memory to the Elgar harness.

The goal is not to make the model "remember" more. The goal is for Elgar to
track verified work inside one harness turn and prevent the model from wasting
calls on repeated or no-op primitive tools.

## Problem Observed

Live `review the project` runs showed unstable behavior.

Good run:

- `ls .`
- `read package.json`
- `find README*`
- `find app`
- attempted to continue with useful app-file inspection

Bad run:

- `ls .`
- repeated `ls .`
- `find package.json`
- `read package.json`
- repeated `ls .`
- searched common but wrong filename `next.config.js`
- stopped before reading app files

Both runs had the same user intent. The difference was the model's tool path.

## Root Cause

The model does not reliably treat previous tool evidence as operational memory.
It may see that `ls .` already happened, but still ask for `ls .` again because:

- the root listing was large or truncated
- broad project review makes the model fall back to generic exploration
- prompt reminders are advisory, not enforced
- the model does not know that the exact same read-only primitive cannot produce
  new information inside the same turn

Elgar knows this. The model does not always act on it.

## Current First Step

Elgar now has short-term harness memory for one turn.

It tracks:

- paths already listed
- files already read
- find patterns already used
- grep queries already used
- exact duplicate requests

It logs `harness_memory_snapshot` events to the existing system log.
It also writes compact durable harness events to the existing session JSONL log
under `.elgar/log/sessions`.

Current behavior:

- exact duplicate primitive requests are detected
- duplicates are logged as memory
- duplicates do not become verified evidence
- duplicates do not consume useful evidence budget
- the next model call receives compact short-term memory
- the second duplicate/no-op request in one turn stops the loop with
  `duplicate_loop_detected`
- simple path variants such as `./package.json` and `package.json` share the
  same memory key
- duplicate rejections are rendered back to the model as explicit runtime
  feedback
- over-budget batch requests are rendered back as skipped runtime feedback,
  without becoming verified evidence

The durable session events are intentionally compact. They record decisions,
verified tool-result labels, duplicate rejections, synthesis status, and stop
reasons; they do not store full prompts or large evidence bodies.

## Why This Is Not Long-Term Memory

Short-term memory is scoped to one harness turn.

It should reset between user turns because files may change later after:

- write/edit tools
- shell commands
- permissioned actions
- external user edits

Long-term memory is a separate future layer. It should be built from verified
session logs and evidence handles, not provider prose.

## Design Direction

Preferred next behavior:

1. Before choosing a primitive tool, the model receives short-term memory.
2. The contract tells the model to check memory before acting.
3. The model chooses a primitive tool or returns final text.
4. Rust validates the request against short-term memory.
5. If it is an exact duplicate, Elgar rejects it as a no-op and asks for a
   different tool or final text.
6. If duplicates continue, Elgar stops with `duplicate_loop_detected` and
   synthesizes from verified evidence.

This keeps the model in control of tool choice while making Elgar responsible
for loop safety.

## Safe Rule

The safe duplicate rule is narrow:

```text
same harness turn
+ same primitive tool
+ same normalized arguments
= duplicate/no-op
```

Examples:

- `ls .` repeated in the same turn is a duplicate.
- `read package.json` repeated in the same turn is a duplicate.
- `find . README*` repeated in the same turn is a duplicate.

This should not apply across turns.

## What To Avoid

Avoid broad or hardcoded rules:

- Do not hardcode `review project`.
- Do not add macro tools like `review_project`.
- Do not globally ban repeated tools across sessions.
- Do not block similar-but-not-identical requests.
- Do not treat provider prose as verified memory.
- Do not inject full JSONL logs into the model.

## Open Questions To Research

- What is the best agent pattern for short-term tool memory?
- Should duplicate rejection be a repair call, a normal decision call, or a
  direct runtime stop condition?
- How many duplicate requests should trigger `duplicate_loop_detected`?
- Should repeated requests after file mutations become valid again?
- Should short-term memory include compact directory facts, or only tool keys?
- How do other agents represent "already inspected" state without reducing
  model autonomy?

## Current Recommendation

Implement memory as a harness-owned validation layer:

- memory is visible to the model
- memory is logged
- duplicate/no-op requests are rejected by Rust
- normal final answers come directly from the native provider loop
- synthesis remains a fallback for duplicate-loop and other safe-stop paths

This should improve reliability without introducing macro tools or hardcoded
natural-language task routing.

## Completed Hardening Slice

The bounded cross-turn prompt memory slice is complete.

Archived execution details:

```text
docs/archive/HARNESS_MEMORY_HARDENING_PLAN.md
docs/archive/CURSOR_MEMORY_REVIEW_REQUEST.md
```

The implementation keeps full JSONL as audit truth but injects only a capped,
recent, verified prompt view into provider calls.

Current bounded prompt memory:

- keeps the full session JSONL and compact index as audit truth
- renders only prompt-useful facts into provider context
- excludes permission decisions and stop reasons from prompt memory
- prefers newer facts within each kind
- logs indexed count, rendered count, rendered chars, omitted count, and whether
  the prompt-memory budget was hit

This is cross-turn prompt hardening. Same-turn harness working memory remains
the runtime layer that rejects duplicate/no-op tool requests inside one loop.

## Recall Quality Follow-Up

Stress dogfood showed a separate reliability issue: some direct file-inspection
prompts returned provider prose like "file read successfully" without a
verified tool result. Those turns should not become memory truth.

The native tool-loop prompt now states that direct local inspection requests
such as read, list, find, grep, create, write, edit, or run should use the
matching tool or permission path instead of answering from prior messages.
Verified memory rendering also groups facts by kind so inventory-style answers
can copy the rendered sections directly.

## Provider Claim Guard Follow-Up

The remaining stress gap was not a memory-rendering bug. One turn ended as
plain provider prose about `app/page.tsx` without a verified tool result, so
there was no fact for memory to recall.

Elgar now guards final provider prose before it becomes visible truth. If a
turn has no verified evidence and the provider claims local file actions or
local project facts, the loop stops with `unverified_provider_action_claim`
instead of storing or displaying the claim as normal final text.

This guard validates provider output only. It does not inspect user prompts,
route intent, add macro tools, or hardcode user-command triggers.

If the first blocked claim happens before any evidence exists, Elgar now gives
the provider one corrective retry inside the same turn. The retry is generic
runtime feedback: request a primitive tool if local evidence is needed, or
answer without local file/project claims. A second blocked claim still stops
with `unverified_provider_action_claim`.

The same retry path also catches incorrect approval requests for read-only
inspection. If provider prose asks for approval to read, list, find, grep, or
inspect local project state, Elgar returns generic runtime feedback that those
primitives are read-only and do not need approval. A second bad approval claim
stops with `read_only_approval_claim`.

Explicit primitive target fidelity is guarded before tool execution. For narrow
requests such as opening a specific file or searching for text in a specific
file, Elgar rejects provider-selected tools whose arguments target a different
file or query. The provider gets corrective tool-result notice; repeated
mismatches stop with `tool_target_mismatch`.

## Live Dogfood Model Baseline

Use Qwen as the preferred live dogfood model for memory and tool-fidelity
checks. Keep Gemma as a comparison model, not the main regression signal.

Latest repeated stress dogfood, three runs per model:

| Metric | Gemma `google/gemma-4-26b-a4b-qat` | Qwen `qwen3.6-35b-a3b-mlx` |
| --- | ---: | ---: |
| Round-0 tool affinity | 87% | 95% |
| Final recall | 90% | 100% |
| Guard retries | 0 | 0 |
| Target mismatches | 0 | 0 |
| Node_modules trap | 3/3 pass | 3/3 pass |
| Dogfood artifact hallucination | 3/3 pass | 3/3 pass |
| Total tokens | 169,600 | 219,831 |
| Approx throughput | 799 tokens/sec | 1,010 tokens/sec |

Interpretation:

- Qwen is more reliable for strict tool execution and recall.
- Qwen costs about 30% more tokens in this stress test, but was not materially
  slower on local LM Studio/MLX.
- Gemma repeatedly missed late-session reads for `app/page.tsx` and
  `postcss.config.mjs`.
- Qwen removed those read misses; its remaining variance was isolated to
  listing `components`.
- Both models had clean target-fidelity and guard metrics, so the observed gap
  is model tool-affinity variance rather than a harness failure.

Dogfood reports use `LATER_ROUND` when expected evidence appears after round 0.
This is different from a guard retry; guard retries are counted separately.

## Session Isolation Rule

Each launched runtime surface should create a unique session id. Scripted TUI,
single-turn CLI, and interactive terminal runs use ids with a surface prefix,
process id, timestamp, and local counter.

`/clear` keeps the same launch lineage but rotates the conversation scope with
`-clear-N`. Because verified memory reads only the current session JSONL file,
this prevents memory from crossing independent launches while still preserving
old JSONL logs for audit.
