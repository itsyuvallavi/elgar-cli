# Elgar v0.2

Elgar is a local-first Rust agent harness. The v0.2 line is a clean restart
focused on a small, inspectable core:

```text
Model reasons.
Runtime routes.
Action gate enforces.
Filesystem confirms.
UI reports.
Tests protect.
Extensions wait.
```

The current build includes a model-first agent runtime, a narrow action gate
for explicit approvals, permissioned file and shell actions, a terminal TUI,
LM Studio provider support, context accounting, and no-network regression
checks.

## Workspace

```text
crates/elgar-core   runtime, action gate, routing, session, actions, filesystem, provider, shell
crates/elgar-cli    command-line entry point and smoke/performance commands
crates/elgar-tui    terminal UI rendering, input, panes, theme, and TUI shell
docs/               current implementation notes and local operating docs
zz_elgar_agent_docs agent-facing instructions and roster
```

## Quick Start

Run the no-network local verification path:

```sh
./bin/check-local
```

Run the CLI from source:

```sh
cargo run -p elgar-cli -- "hello"
```

Install the local `elgar` command:

```sh
./bin/install-local
elgar
```

`elgar` with no arguments launches the terminal TUI when run from an
interactive terminal. In non-interactive shells it exits safely with a short
message instead of hanging. The local install records this repo path so
`elgar` can still find `elgar-provider.json` and `AGENTS.md` when launched from
another directory. A configured project in the current directory or one of its
parents still takes precedence.

## Provider Mode

Normal CLI/TUI runs can use LM Studio when `elgar-provider.json` enables live
mode and LM Studio is running locally. Without a live provider config, Elgar
uses deterministic stub/no-network behavior.

To force a no-network run:

```sh
ELGAR_PROVIDER_CONFIG=off elgar
```

Live provider smoke commands are manual and optional. They are documented in:

```text
docs/live-provider-smoke.md
```

Default local checks and CI do not require LM Studio.

## Permission Model

Provider/model text is never treated as verified truth. For file or shell work,
Elgar records a typed action first. The action gate handles explicit
`/approve` or `/reject` commands before any gated operation mutates or executes.
The filesystem or shell executor then records the verified result.

Examples:

- project-relative folder creation uses a typed `CreateDirectory` action
- external folder creation uses an approved `ShellCommand` and verifies the
  directory exists after execution
- rejected actions are terminal and do not mutate files or run commands

## Local Checks

Use this before handing off changes:

```sh
./bin/check-local
```

It runs:

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

CI also runs:

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

## Documentation

Start with:

```text
docs/local-checks.md
docs/live-provider-smoke.md
docs/permissioned-shell-commands.md
docs/v0.2-forward-plan.md
zz_elgar_agent_docs/AGENTS.md
zz_elgar_agent_docs/AGENT_ROSTER.md
```

Planning exports are tracked under `docs/planning/`. Linear is the execution
map for active implementation work.
