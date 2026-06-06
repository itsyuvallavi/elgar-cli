# bin

## Purpose

Small local scripts for the current raw-chat baseline.

## Active Scripts

- `install-local` builds and installs the local `elgar` binary into Cargo's bin
  directory.
- `check-local` runs the current local verification set.

## Archived Scripts

`_archive/` contains old dogfood and performance scripts that target archived
tool, permission, planning, memory, shell, or trace behavior.

They are kept as historical references only. Do not use them as current checks.

## Checks

Run:

```sh
./bin/check-local
```

Current coverage:

```text
cargo fmt --check
cargo check -p elgar-core
cargo check -p elgar-tui
cargo check -p elgar-cli
cargo test -p elgar-tui
cargo test -p elgar-cli
```

Core tests are intentionally not part of this script yet because the core suite
still needs stale-test cleanup.
