# bin

## Purpose

Local developer entrypoints for installing, checking, and measuring Elgar.

## Important Files

- `check-local` runs the main no-network Rust checks used by CI.
- `dogfood-plan-contract` runs the live scripted plan-contract loop against the installed `elgar`.
- `dogfood-plan-followup-execution` runs a live plan-creation, latest-plan execution, memory, token, reasoning, and shell-command flow against the installed `elgar`.
- `dogfood-plain-memory-regression` verifies a live plain chat turn stays cheap after verified plan memory exists.
- `dogfood-latest-plan-selection` verifies that latest verified plan memory wins when more than one plan exists.
- `dogfood-review-approval-policy` verifies review policy, pending action approval, memory, reasoning, and shell execution.
- `dogfood-file-tool-lifecycle` exercises create, patch, overwrite, move, delete, shell, memory, and reasoning through live TUI input.
- `dogfood-guidance-ambiguity` verifies model-owned clarification via `ask_guidance` before writing into an ambiguous target.
- `dogfood-complex-python-execution` creates and executes a larger Python package plan, edits it, and runs shell verification.
- `dogfood-tui-flow` runs the live scripted TUI/runtime flow against the installed `elgar`; full-screen renderer behavior is covered by TUI renderer tests.
- `install-local` installs the local CLI for dogfooding.
- `perf-baseline` captures the current performance baseline.

## Ownership

Keep scripts thin and deterministic. They should call workspace tools, not hide product logic.

## Checks

- `./bin/check-local`
- `./bin/dogfood-plan-contract`
- `./bin/dogfood-plan-followup-execution`
- `./bin/dogfood-plain-memory-regression`
- `./bin/dogfood-latest-plan-selection`
- `./bin/dogfood-review-approval-policy`
- `./bin/dogfood-file-tool-lifecycle`
- `./bin/dogfood-guidance-ambiguity`
- `./bin/dogfood-complex-python-execution`
- `./bin/dogfood-tui-flow`
- `./bin/perf-baseline`
