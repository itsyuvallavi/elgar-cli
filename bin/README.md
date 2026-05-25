# bin

## Purpose

Local developer entrypoints for installing, checking, and measuring Elgar.

## Important Files

- `check-local` runs the main no-network Rust checks used by CI.
- `install-local` installs the local CLI for dogfooding.
- `perf-baseline` captures the current performance baseline.

## Ownership

Keep scripts thin and deterministic. They should call workspace tools, not hide product logic.

## Checks

- `./bin/check-local`
- `./bin/perf-baseline`
