# Harness Baselines

## Purpose

Manual benchmark snapshots for the active harness path.

Use these results to compare whether future batching, routing, progressive
detail, memory, or compression changes improve behavior without reducing
answer quality.

## Baseline: 2026-06-07 CLI Non-TUI Batch

Historical pre-native-loop benchmark. It is useful for comparison, but its
stop reasons and call shape do not describe the current native tool loop.

Project:

```text
/Users/yuval/__git/elgar/playground/Nextjs-1
```

Run method:

```text
elgar "<prompt>"
```

Output capture:

```text
/tmp/elgar-cli-batch-baseline.txt
```

This measures core harness/provider behavior without TUI rendering.

| Prompt | Provider Calls | Total Tokens | Duration | Stop Reason | Notes |
|---|---:|---:|---:|---|---|
| `hello` | 1 | 1.4k | 1.5s | `model_message` | ok |
| `read package.json` | 2 | 3.5k | 14s | `model_message_after_evidence` | ok |
| `read app/page.tsx` | 2 | 3.3k | 10s | `model_message_after_evidence` | ok |
| `list app` | 3 | 7.8k | 18s | `model_message_after_evidence` | inefficient: used `ls .` then `ls app` |
| `read app` | 5 | 14.7k | 47s | `evidence_item_budget_exhausted` | inefficient: too many rounds |
| `grep for tailwind` | 2 | 3.5k | 19s | `model_message_after_evidence` | ok |
| `grep for export default` | 2 | 3.4k | 12s | `model_message_after_evidence` | ok |
| `find files named config` | 2 | 3.1k | 8s | `model_message_after_evidence` | ok |
| `find README files` | 5 | 13.2k | 67s | `find_budget_exhausted` | inefficient: repeated find/list |
| `review app` | 4 | 11.6k | 62s | `evidence_item_budget_exhausted` | inefficient: too broad for app-only prompt |
| `review this project` | 3 | 7.6k | 59s | `invalid_choice_after_evidence` | shallow: only `ls .` evidence |
| `tell me what files matter most in this project` | 3 | 8.0k | 62s | `ls_budget_exhausted` | acceptable answer, inefficient |

## Observations

- Simple direct file/search tasks are mostly acceptable.
- Directory-focused prompts are inefficient.
- The model often starts with `ls .` even when the user names a specific path.
- Batching works, but not consistently enough.
- Some broad review prompts stop for budget reasons before collecting ideal
  evidence.

## Current Optimization Target

Improve native tool selection and path choice before adding compression:

- Prefer the user-named path first.
- Batch clearly independent reads after a directory listing.
- Avoid repeated find/list loops when one result is enough.
- Keep all tools primitive; do not add macro tools.

## Experiment: 2026-06-07 Path-First Contract

Change:

- Added contract guidance to prefer user-named paths before broader project
  inspection.
- Added contract guidance to batch obvious file reads after listing a named
  directory.
- Kept the tool model primitive-only: `ls`, `find`, `grep`, `read`.

Output capture:

```text
/tmp/elgar-cli-batch-after-path-first.txt
```

| Prompt | Before Calls | After Calls | Before Tokens | After Tokens | Before Duration | After Duration | Result |
|---|---:|---:|---:|---:|---:|---:|---|
| `hello` | 1 | 1 | 1.4k | 1.5k | 1.5s | 6s | unchanged behavior |
| `read package.json` | 2 | 2 | 3.5k | 3.8k | 14s | 15s | unchanged |
| `read app/page.tsx` | 2 | 2 | 3.3k | 3.4k | 10s | 7s | slightly faster |
| `list app` | 3 | 2 | 7.8k | 3.4k | 18s | 10s | improved |
| `read app` | 5 | 3 | 14.7k | 5.7k | 47s | 18s | improved |
| `grep for tailwind` | 2 | 2 | 3.5k | 3.7k | 19s | 14s | slightly faster |
| `grep for export default` | 2 | 2 | 3.4k | 3.6k | 12s | 12s | unchanged |
| `find files named config` | 2 | 2 | 3.1k | 3.3k | 8s | 7s | unchanged |
| `find README files` | 5 | 2 | 13.2k | 3.3k | 67s | 8s | improved |
| `review app` | 4 | 3 | 11.6k | 9.2k | 62s | 65s | fewer tokens, still slow |
| `review this project` | 3 | 3 | 7.6k | 9.1k | 59s | 65s | worse tokens |
| `tell me what files matter most in this project` | 3 | 3 | 8.0k | 9.3k | 62s | 48s | faster, more tokens |

Conclusion:

- The path-first contract helped concrete directory/file/search prompts.
- Broad review prompts still need a different efficiency layer.
- Next likely target is progressive detail: concise evidence first, with exact
  file contents retrieved only when the model clearly needs them.

## Rejected Experiment: 2026-06-07 Compact `ls` Decision Evidence

Change tested:

- Kept full verified evidence internally.
- Sent compact `ls` evidence only to follow-up decision calls.
- Kept final synthesis on full verified evidence.

Output capture:

```text
/tmp/elgar-cli-batch-after-compact-ls.txt
```

Result:

| Prompt | Path-First Calls | Compact-Ls Calls | Path-First Tokens | Compact-Ls Tokens | Result |
|---|---:|---:|---:|---:|---|
| `list app` | 2 | 3 | 3.4k | 6.7k | regressed |
| `read app` | 3 | 3 | 5.7k | 6.7k | worse answer; missed `app/page.tsx` |
| `find README files` | 2 | 4 | 3.3k | 9.9k | regressed |
| `review app` | 3 | 2 | 9.2k | 4.1k | fewer tokens but unreliable; answered from listing only |
| `review this project` | 3 | 2 | 9.1k | 4.0k | fewer tokens but unreliable; asked user for files |

Conclusion:

- Compacting all `ls` evidence during decision calls reduced some token totals
  by making the model stop early, but it damaged reliability.
- The experiment was reverted.
- Do not reintroduce generic compact `ls` decision evidence without a stronger
  retrieval contract or a different evidence design.

## Rejected Experiment: 2026-06-07 Generic Stopping Rules

Change tested:

- Added contract guidance for when evidence should be considered enough.
- Kept primitive tools only.
- Did not change execution, evidence, logs, or synthesis.

Output capture:

```text
/tmp/elgar-cli-batch-after-stopping-rules.txt
```

Result:

| Prompt | Path-First Calls | Stopping-Rules Calls | Path-First Tokens | Stopping-Rules Tokens | Result |
|---|---:|---:|---:|---:|---|
| `list app` | 2 | 2 | 3.4k | 3.7k | ok |
| `read app` | 3 | 4 | 5.7k | 8.4k | reliable but less efficient |
| `find README files` | 2 | 3 | 3.3k | 7.3k | regressed |
| `review app` | 3 | 2 | 9.2k | 5.2k | unreliable; described next step instead of requesting files |
| `review this project` | 3 | 2 | 9.1k | 5.3k | unreliable; asked user for files |
| `tell me what files matter most in this project` | 3 | 3 | 9.3k | 9.6k | no improvement |

Conclusion:

- Generic stopping guidance made the model more likely to stop or speak about
  next steps instead of requesting primitive tools.
- The experiment was reverted.
- Future work should focus on a stronger model-action protocol, not more prose
  guidance about stopping.

## Rejected Experiment: 2026-06-07 Strict Decision JSON

Change tested:

- Decision mode accepted only JSON actions.
- Added `final_answer` as an explicit JSON final-answer shape.
- Rejected raw prose in loop decisions.
- Kept primitive tools and execution unchanged.

Output capture:

```text
/tmp/elgar-cli-batch-after-strict-protocol.txt
```

Result:

| Prompt | Path-First Calls | Strict Calls | Path-First Tokens | Strict Tokens | Result |
|---|---:|---:|---:|---:|---|
| `list app` | 2 | 2 | 3.4k | 3.4k | ok |
| `read app` | 3 | 3 | 5.7k | 9.2k | reliable but more expensive |
| `find files named config` | 2 | 2 | 3.3k | 3.5k | valid but stopped through invalid-choice synthesis |
| `find README files` | 2 | 4 | 3.3k | 11.4k | regressed |
| `review app` | 3 | 3 | 9.2k | 9.4k | reliable, no speed gain |
| `review this project` | 3 | 2 | 9.1k | 5.0k | unreliable; answered from `ls:.` only |
| `tell me what files matter most in this project` | 3 | 3 | 9.3k | 9.7k | no improvement |

Conclusion:

- Strict JSON reduced one class of visible raw JSON/prose ambiguity, but did not
  improve broad review behavior.
- It made some simple discovery prompts more expensive and still allowed
  unreliable early answers through invalid-choice synthesis.
- The experiment was reverted.
- A stricter protocol likely needs a repair path and/or a typed action schema
  before it is worth re-testing.
