# bin

## Purpose

Local developer entrypoints for installing, checking, and measuring Elgar.

## Important Files

- `check-local` runs the main no-network Rust checks used by CI.
- `dogfood-plan-contract` runs the live scripted plan-contract loop against the installed `elgar`.
- `dogfood-tui-flow` runs the live scripted TUI/runtime flow against the installed `elgar`; full-screen renderer behavior is covered by TUI renderer tests.
- `install-local` installs the local CLI for dogfooding.
- `perf-baseline` captures the current performance baseline.

## Ownership

Keep scripts thin and deterministic. They should call workspace tools, not hide product logic.

## Checks

- `./bin/check-local`
- `./bin/dogfood-plan-contract`
- `./bin/dogfood-tui-flow`
- `./bin/perf-baseline`
