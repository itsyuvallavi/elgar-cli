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
3. The model chooses a primitive tool or `answer_now`.
4. Rust validates the request against short-term memory.
5. If it is an exact duplicate, Elgar rejects it as a no-op and asks for a
   different tool or `answer_now`.
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
- final answer still comes from synthesis over verified evidence

This should improve reliability without introducing macro tools or hardcoded
natural-language task routing.
