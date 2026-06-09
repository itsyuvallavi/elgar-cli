# Primitive Tool Model

## Purpose

This document explains the tool architecture Elgar should move toward as it
adds agent features back.

The short version:

```text
Expose honest low-level primitive tools to the model.
Validate every request in Rust.
Gate risky actions through policy.
Execute through narrow executors.
Record verified results.
Render exactly what happened.
```

Elgar should not hide broad workflows behind special harness shortcuts such as
`review_project`. A project review should emerge from normal tool use: inspect
the tree, read relevant files, maybe run a safe command later, then synthesize
from verified evidence.

## The Target Contract

Elgar's contract stays:

```text
model owns intent
runtime validates
policy decides
executors verify
UI reports
tests protect
```

This means the model should be allowed to choose tools, but not allowed to
execute arbitrary effects directly. The harness sits between model requests and
real-world actions.

## What Pi Teaches

Pi's coding agent model is intentionally tool-forward. The model gets direct
low-level tools such as `read`, `write`, `edit`, and `bash`, with helper tools
like `grep`, `find`, and `ls` also present in the built-in tool set.

That design has an important property: the model is not forced through a
special `review_project` tool. For a review request, the model can list files,
read `package.json`, inspect source files, run commands if appropriate, then
answer. The workflow is composed from simple tools.

Pi also stores sessions as JSONL entries containing user messages, assistant
messages, tool calls, and tool results. That protects the agent from relying
only on the model's short-term memory. Compaction summarizes old context while
keeping structured session history available.

The tradeoff is that Pi does not provide Elgar's desired built-in permission
model by default. Its own docs point users toward sandboxing, containerization,
or extension-based controls when stronger boundaries are needed.

## What Elgar Should Borrow

Borrow the tool shape:

- Give the model clear low-level primitive tools.
- Keep primitive tool descriptions short and exact.
- Let the model combine tools for complex work.
- Persist verified tool calls and results as session truth.
- Compact old context from structured events, not from vague chat memory.

Do not borrow unguarded execution:

- The model may request a file write, but Rust validates the path and payload.
- The model may request shell, but policy decides if it is allowed.
- The model may claim success, but executors verify before UI reports success.
- The model may synthesize an answer, but only from verified evidence.

## Current Elgar Direction

The current harness direction is stricter now: no macro tools. The model should
see only primitive tools:

```text
read   -> read one bounded UTF-8 file
ls     -> list one bounded directory
find   -> find file/directory paths by name pattern
grep   -> search text inside bounded UTF-8 files
bash   -> run one shell command after policy approval
write  -> create or overwrite one file after policy approval
edit   -> patch one existing file after policy approval
```

There are no active macro tools in the current harness. A project review
should emerge from primitive tool use such as `ls .`, `read package.json`,
`find app`, `grep "export default"`, and then final provider text from verified
tool results.

## Why Not Macro Tools

A broad macro tool such as `review_project` is tempting because it can
reduce model rounds. It is also risky for Elgar's goals.

Problems:

- It hides work from the model-visible tool sequence.
- It changes semantics based on a task label instead of explicit tool calls.
- It makes traces less honest: one tool name can imply many internal actions.
- It can reintroduce hardcoded workflow routing.
- It makes permission boundaries fuzzier when writes or shell enter later.

Macro-like helpers are only acceptable if they remain explicit, traceable, and
honest. For example, a future "batch read" tool could be acceptable if the
model explicitly requests exact file paths and the runtime records each file
read. A hidden "review this project" workflow should not replace low-level
tools.

## Reliability Model

The model can be weak. The harness must compensate with structure:

- Validate tool names against a registry.
- Validate arguments with typed parsers.
- Reject unknown or disabled primitive tools.
- Enforce path, byte, line, and command limits.
- Detect repeated evidence requests.
- Stop bounded loops before they spiral.
- Use fallback synthesis only when the native loop cannot continue safely, such
  as duplicate-loop stops, invalid native tool calls, or explicit safe-stop
  paths.
- Store verified events so follow-up turns can retrieve exact facts.

The goal is not to make the model remember everything. The goal is to make the
harness remember what actually happened.

## Performance Model

Making the harness less strict is the wrong speed optimization. It may reduce a
call or two, but it weakens correctness exactly where Elgar needs strength.

Better performance levers:

- Let trivial prompts finish in one harness provider call with no tool
  execution.
- Scope the visible primitive tool list by request mode.
- Keep tool schemas compact and stable.
- Use bounded evidence budgets.
- Prefer provider final text after tool results; use fallback synthesis only on
  explicit safe-stop paths.
- Compress large verified outputs before sending them back to the model.
- Persist raw evidence separately so copy/details stay exact.

Strict validation and fast execution are compatible if the harness keeps each
tool small.

## Implementation Implications

Near term:

- Keep the current primitive tools granular.
- Do not add `review_project` or any other hidden workflow.
- Add tests that unknown primitive names are rejected.
- Add tests that `read`, `ls`, `find`, and `grep` return bounded evidence.
- Keep permission decisions explicit before enabling side-effect execution.
- Keep fallback synthesis no-tool and evidence-only.

Next primitive tool stages:

- Add write/edit tools as explicit low-level primitive tools.
- Add shell as an explicit low-level primitive tool with policy gating.
- Add a verified action/event ledger for created, edited, read, and executed
  artifacts.
- Add route-scoped retrieval from that ledger for follow-up questions.
- Add session persistence in an append-only format before compaction.

Longer term:

- Add compaction over verified session events.
- Add richer UI rendering for tool calls, results, and synthesis.
- Add provider/request-mode tuning without changing the tool contract.

## Acceptance Criteria

Elgar is following this model when:

- A review request is handled by visible low-level tool requests, not a hidden
  review macro.
- Every executed action has a typed request, a validation result, a policy
  decision where needed, and a verified result.
- Follow-up questions can be answered from verified events, not model memory.
- Trivial conversation can finish in one harness call without executing tools.
- Tool-loop performance is improved through bounded loops and synthesis, not by
  skipping validation.

## Sources

- [Pi repository](https://github.com/earendil-works/pi)
- [Pi extension docs: built-in tools, overrides, truncation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md)
- [Pi session format](https://pi.dev/docs/latest/session-format)
- [Pi compaction docs](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/compaction.md)
