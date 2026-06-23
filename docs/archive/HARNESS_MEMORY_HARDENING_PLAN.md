# Harness Memory Hardening Plan

Status: completed and archived. This document records the Slice 3a execution
plan; current memory behavior is summarized in
`docs/HARNESS_SHORT_TERM_MEMORY.md`.

## Purpose

Make harness memory useful without letting it become another source of token
growth, stale context, or model confusion.

This plan is for **Slice 3a only**:

```text
bounded verified-memory prompt view
```

It does not add new tools, macro routing, planning behavior, or TUI approval UI.
It does not change JSONL audit storage. It only controls what compact verified
memory is injected into provider prompts.

## Current Behavior

Elgar has two memory/context channels at the start of a harness turn:

```text
system prompt
  -> native harness instructions
  -> history disclaimer
  -> verified session facts rendered from JSONL

chat messages
  -> bounded prior user/assistant turns
  -> current user input
```

What is already bounded:

- prior user turns: capped at 8
- assistant replay: capped at 800 chars per assistant message
- history token estimate: capped around 2048

What is not bounded:

- rendered verified memory facts
- per-kind memory facts
- rendered memory chars/tokens

Relevant current files:

```text
crates/elgar-core/src/harness/harness_loop/provider/session_context.rs
crates/elgar-core/src/harness/harness_loop/provider/context.rs
crates/elgar-core/src/harness/harness_loop/state/logging/provider_events.rs
crates/elgar-core/src/harness/memory/index.rs
crates/elgar-core/src/harness/memory/render.rs
crates/elgar-core/src/harness/memory/types.rs
```

## Evidence From Dogfood

Basic recall passed:

- `read package.json`
- `list app`
- approved write
- no-tools recall
- `/clear`
- post-clear recall

Stress test exposed the real gap:

- verified facts grew to 21
- history stayed bounded
- prompt tokens grew from about 1.8k to about 4.5k
- model hallucinated `.dogfood-*` artifact paths from transcript/history prose
- `app/page.tsx` was repeatedly omitted despite being read
- final recall reached 92%, but not 100%

Interpretation:

- JSONL indexing works.
- `/clear` works.
- The prompt view of memory is too loose.
- Assistant replay can compete with verified facts unless the prompt makes
  verified facts clearly authoritative.

## Target Behavior

Keep full memory in JSONL as audit truth.

Inject only a bounded prompt view:

```text
full session JSONL
  -> build full verified memory index
  -> select newest useful facts under per-kind and total budgets
  -> render compact prompt memory
  -> log indexed count, rendered count, chars, and budget hit
```

The model should see enough verified memory to avoid rework, but not enough to
inflate every turn or confuse current context.

## Slice 3a Scope

Implement bounded verified-memory rendering.

In scope:

- add prompt-memory budget type
- select rendered facts from the full index
- prefer recent facts
- cap facts by kind
- cap total rendered chars
- exclude permission and stop facts from prompt memory
- add an overflow line when facts are omitted
- log rendered memory stats
- update tests and dogfood checks

Out of scope:

- changing JSONL event storage
- changing same-turn working memory
- changing TUI session ids
- lowering `MAX_ASSISTANT_CHARS`
- adding macro tools
- adding natural-language trigger tables
- changing approval, bash, write, or edit behavior

## Proposed Budget

Initial default budget:

```text
max rendered chars:        3000
read file facts:             12
listed directory facts:       8
find facts:                   4
grep facts:                   4
approved execution facts:     8
permission facts:             0
stop reason facts:            0
```

Reasoning:

- The observed stress test had about 13 useful action facts.
- These caps should preserve that case with some headroom.
- Permission and stop facts matter for audit/logging, but they are not useful
  prompt memory for most follow-up user requests.

If the rendered output still exceeds the char budget, prune oldest selected
facts until it fits.

Add one compact omission line:

```text
+ N older verified facts omitted from prompt memory; full audit remains in JSONL
```

## Ordering Rule

Prompt memory should be grouped by kind for readability:

```text
read
listed
find
grep
executed
```

Within each group, use newest first.

This keeps the prompt structured while biasing toward recent verified work.

## Verified Memory Rule

When verified facts are present, add a stable rule to the system prompt:

```text
When stating which files were read, listed, searched, or written, use only
paths listed under "Verified session facts". Do not infer file actions from
prior assistant messages.
```

This is not a natural-language trigger table. It does not force tool choices.
It only clarifies evidence precedence.

## Files To Add

```text
crates/elgar-core/src/harness/memory/budget.rs
crates/elgar-core/src/harness/tests/memory/render_budget_test.rs
```

## Files To Edit

```text
crates/elgar-core/src/harness/memory/mod.rs
crates/elgar-core/src/harness/memory/render.rs
crates/elgar-core/src/harness/memory/README.md
crates/elgar-core/src/harness/harness_loop/provider/session_context.rs
crates/elgar-core/src/harness/harness_loop/provider/context.rs
crates/elgar-core/src/harness/harness_loop/state/logging/provider_events.rs
crates/elgar-core/src/harness/tests/memory/mod.rs
bin/dogfood-memory-stress
bin/dogfood-memory-recall
bin/README.md
docs/HARNESS_SHORT_TERM_MEMORY.md
docs/PROJECT_PLAN.md
```

## Implementation Steps

1. Add `budget.rs`.
   - Define `HarnessMemoryPromptBudget`.
   - Define `RenderedMemoryStats`.
   - Add `select_facts_for_prompt`.
   - Exclude prompt-useless facts.
   - Prefer newest facts within each kind.

2. Update renderer.
   - Render selected facts, not the full index.
   - Return rendered text plus stats.
   - Include the omission line when facts are pruned.

3. Wire normal provider context.
   - Use bounded rendered memory in `native_tool_loop_turn_context`.
   - Add verified memory precedence rule only when facts are present.
   - Extend `TurnPromptContextStats`.

4. Wire repair provider context.
   - Use the same bounded memory helper.
   - Avoid a second unbounded render path.

5. Update logging.
   - Replace or extend `verified_fact_count` with:
     - `indexed_fact_count`
     - `rendered_fact_count`
     - `rendered_memory_chars`
     - `memory_budget_hit`
   - Keep compatibility if needed by retaining `verified_fact_count` as the
     indexed count for now.

6. Add focused tests.
   - Caps total rendered chars.
   - Caps per-kind facts.
   - Prefers newer facts.
   - Excludes permission and stop facts.
   - Adds omission line.
   - Keeps empty memory blank.
   - Keeps basic recall facts present when under budget.

7. Update dogfood scripts.
   - Assert rendered memory chars are logged.
   - Assert final recall does not hallucinate `.dogfood-*` artifacts.
   - Check for required dependencies such as `rg`, or use a fallback.
   - Document artifact behavior.

8. Update docs.
   - Document the audit-vs-prompt split.

## Implementation Notes

`docs/agent/AGENTS.md` is intentionally not part of this slice. Bounded memory
changes harness behavior and operational docs, not standing agent rules.

The prompt-context log keeps `verified_fact_count` as the indexed fact count
for compatibility and adds:

```text
indexed_fact_count
rendered_fact_count
rendered_memory_chars
omitted_fact_count
memory_budget_hit
```
   - Mark bounded memory as the current next memory slice.

## Pre-Mortem

### Failure: Caps hide facts the model needs.

Mitigation:

- Use newest-first selection.
- Keep enough per-kind headroom for the observed stress test.
- Log omitted counts so we can see when the budget is active.
- Keep JSONL full audit intact.

### Failure: Token usage still grows.

Mitigation:

- Enforce a hard rendered char budget.
- Log rendered chars on every turn.
- Add a stress dogfood assertion for peak rendered memory size.

### Failure: Model still uses assistant history as fact memory.

Mitigation:

- Add the verified memory precedence rule.
- Do not lower assistant history yet.
- If hallucinations continue after Slice 3a, revisit assistant replay size as a
  separate slice.

### Failure: Tests pass but live behavior regresses.

Mitigation:

- Run unit tests.
- Run full harness tests.
- Run memory recall dogfood.
- Run memory stress dogfood.
- Compare recall score, hallucinated paths, prompt tokens, and rendered chars.

### Failure: Logging changes break `elgar logs latest`.

Mitigation:

- Keep old field compatibility where practical.
- Run CLI diagnostics tests.
- Run `elgar logs latest` manually after dogfood.

### Failure: Dogfood scripts dirty the playground.

Mitigation:

- Move generated transcripts and manifests outside the playground when possible.
- If retained audit files are intentional, document them clearly.
- Add cleanup instructions or a cleanup flag later.

## Required Tests

Automated:

```text
cargo fmt --check
cargo test -p elgar-core harness::tests::memory -- --nocapture
cargo test -p elgar-core harness
cargo test -p elgar-cli diagnostics
./bin/check-local
```

Live dogfood:

```text
./bin/install-local
./bin/dogfood-memory-recall
./bin/dogfood-memory-stress
```

Review logs:

```text
elgar logs latest
```

## Acceptance Criteria

Slice 3a is successful when:

- full JSONL remains unchanged as audit truth
- prompt memory is rendered from a bounded selected view
- rendered memory chars never exceed the configured budget
- permission and stop facts are not injected as prompt memory
- logs show indexed facts, rendered facts, rendered chars, and budget hit
- basic recall still passes
- stress dogfood has no `.dogfood-*` hallucinated file actions in final recall
- prompt tokens do not grow linearly with every verified fact
- no macro tools or hardcoded natural-language trigger tables are added

## Deferred Slice 3b

After Slice 3a passes, decide whether to isolate interactive TUI launches.

Current risk:

```text
crates/elgar-tui/src/terminal.rs
Session::new("terminal-tui-session", ...)
```

Potential fix:

- use a per-launch TUI session id
- keep `/clear` rotation behavior
- update docs and tests

Reason to defer:

- it changes user-visible memory continuity
- bounded memory is the urgent safety fix
- this should be reviewed as a separate behavior change
