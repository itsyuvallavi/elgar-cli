# Orchestrator Situation - 2026-05-29

## Purpose

Handoff for continuing Elgar after the first workable product prototype.

Elgar can now plan and create small runnable projects through the model/tool
runtime, including a React Vite prototype that ran with `npm install` and
`npm run dev`. The next work should focus on making natural-language action
routing, plan follow-up, and performance reliable without reintroducing
hardcoded phrase routing.

## Current Contract

Keep the active v0.2 contract:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

Do not add hardcoded natural-language trigger tables. Slash commands remain
local for explicit controls, but normal text should go through the model path.

## Current Branch And Repo State

Active branch:

```text
core-functionality-review-checkpoint
```

Recent relevant commits:

- `9859583` Avoid tool-enabled calls for plain chat turns
- `7e791f8` Shrink plain chat route prompt
- `dbc580a` Support plan execution intent in route decisions

Known unrelated dirty files to ignore unless explicitly asked:

- `.DS_Store`
- `crates/**/.DS_Store`
- `docs/.DS_Store`
- `playground/`

Do not commit `.DS_Store` or playground dogfood artifacts.

## What Works Now

- Plain chat no longer pays the heavy tool-enabled call when route says plain.
- The plain route prompt is much smaller; live `hello!` dropped to roughly
  `↑165 ↓24` and about 1.8-2s in one smoke.
- Token/timing footer and `/tokens` are present enough to inspect current
  context, last turn, and session totals.
- Natural-language plan creation and execution can work.
- A React Vite dogfood reached a runnable state: after `npm install`,
  `npm run dev` launched Vite successfully.
- Structured plan memory records verified plan files, roots, expected files,
  completion status, and prompt memory selection.

## Current Problems

### Linear auth in old chat

The current old Codex thread had a stale Linear token:

```text
401 token_expired
```

A fresh Codex chat can read Linear again. Continue Linear updates from a fresh
thread.

### Plan follow-up reliability

Observed CalculatorUI flow:

- User asked to create a Markdown plan in `CalculatorUI`; model answered with
  prose instead of creating the file.
- Follow-up `execute the plan` created the folder and plan late, then stopped.
- Follow-up `the plan you just created` did not bind to latest verified plan.
- Forcing `execute the plan for me! create the files in the plan` eventually
  created `README.md`, `calculator.py`, and `ui.py`.
- Final verified memory was correct, but route/follow-up behavior was weak.

`dbc580a` added model-selected `plan_execution` intent, but live model behavior
still needs dogfood and tuning.

### Performance

Action turns can still be expensive because they may involve:

1. compact route call
2. tool-enabled call
3. finalization/post-tool call

Some plan/action turns used about 14k-16k tokens and took 70s+. The next work
should measure each stage separately before optimizing deeper.

### Plan extractor quality

Plan extraction can over-collect paths from prose or embedded code. Recent
examples included JSON fragments and markdown/code snippets being interpreted
as directories/files. Keep improving typed extraction conservatively.

## Linear Map

Important current issues:

- `ELG-326` Roadmap efficient natural-language action routing and agent mode
- `ELG-324` Add durable usage and performance monitoring
- `ELG-323` Add honest token/context usage observability
- `ELG-320` Add first-class PlanContract and runtime-enforced planning
- `ELG-318` Workspace-root execution reliability and TUI trust pass

Retry adding this update to `ELG-326` from the fresh chat:

```text
CalculatorUI regression added as implementation focus.

Failures to fix:
- Plan file request was answered as prose instead of creating the requested markdown file.
- `execute the plan` created the plan late and stopped instead of continuing execution intent.
- `the plan you just created` did not bind to latest verified plan even though memory later recorded it correctly.
- Execution eventually worked, but with high token/latency cost.

Constraints: no hardcoded phrase routing; preserve verified memory and plan preflight.
```

## Recommended Next Sequence

1. Update `ELG-326` with the CalculatorUI regression and current commits.
2. Add live route evals for:
   - plain chat
   - plan-only artifact creation
   - execute latest verified plan
   - "the plan you just created"
   - create a small project with complete files
3. Split token/timing metrics by route call, tool-enabled call, and final call.
4. Improve verified-plan binding so follow-ups can resolve the latest verified
   plan from runtime state before asking clarification.
5. Add a dogfood case for creating a plan then executing the same plan without
   fighting the harness.
6. Keep improving plan extraction so prose/code snippets do not become phantom
   files or directories.
7. After routing and follow-up are stable, experiment with an `agent` mode that
   uses a Codex/Claude-style single tool-enabled loop behind a flag.
8. Compare `auto` router mode against `agent` mode using token/time metrics and
   dogfood transcripts before changing defaults.

## Prompt For Fresh Chat

```text
You are my main Elgar agent.

We are continuing from the Elgar v0.2 workable prototype checkpoint. Start by
reading:
- AGENTS.md
- zz_elgar_agent_docs/AGENTS.md
- zz_elgar_agent_docs/ORCHESTRATOR_SITUATION_2026-05-29.md
- docs/elgar-product-architecture-plan.md
- docs/codex-style-agent-runtime-plan.md

Current branch: core-functionality-review-checkpoint.

Important rules:
- Do not hardcode natural-language phrases or sentence triggers.
- Normal text goes through the model path.
- Slash commands remain local controls.
- Runtime validates, policy decides, executors verify, UI reports.
- Do not commit .DS_Store or playground artifacts.
- Do not revert unrelated dirty work.

First, update Linear issue ELG-326 with the CalculatorUI regression and the
current routing/performance plan from ORCHESTRATOR_SITUATION_2026-05-29.

Then continue implementation on natural-language action routing and plan
follow-up reliability:
1. inspect current state and tests,
2. add/adjust focused tests,
3. implement the smallest safe fix,
4. run targeted tests and ./bin/check-local,
5. update Linear with files changed, tests run, and known limitations,
6. commit only relevant tracked changes.

Focus area:
- latest verified plan binding for follow-ups like "the plan you just created"
- route/tool/final timing and token split
- dogfood coverage for plan creation then execution
- preserve the recent plain-chat optimization

Before editing, tell me the exact first step and Linear issue you are using.
```
