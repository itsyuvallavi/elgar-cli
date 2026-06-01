# Elgar v0.10

Elgar is a local-first Rust agent harness for coding work with local or
OpenAI-compatible models. The current line is focused on a simple contract:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

The goal is a Pi-like terminal experience with Codex-like coding capability,
while keeping Elgar-owned truth about files, commands, plans, approvals, and
state.

## Current Capabilities

- Terminal TUI for normal chat, planning, execution, approvals, memory, status,
  and trace inspection.
- LM Studio / OpenAI-compatible provider support with deterministic stub mode
  for local tests.
- Plain chat stays plain: no tools, no project memory, and no folder anchoring
  on simple messages.
- Route-first runtime with typed tool validation, policy decisions, verified
  filesystem and shell execution, and local state answers.
- Verified project plans with expected file/folder tracking and readable tree
  rendering.
- Follow-up memory from verified action records, including created files,
  folders, plans, structured plans, and imported session artifacts.
- Append-only local observability logs under `.elgar/` for traces and session
  events.
- Shell commands are first-class actions: visible in the TUI, logged in JSONL,
  bounded by timeout, and policy-gated unless allowlisted.
- Conservative read-only shell allowlist for inspection commands such as
  `pwd`, `ls`, `cat`, `head`, `tail`, `wc`, `rg`, safe `sed`, safe `find`, and
  read-only `git` subcommands.

## Workspace

```text
crates/elgar-core   runtime, actions, policy, sessions, memory, provider, fs, shell
crates/elgar-cli    CLI entry point, TUI launch, provider smoke, install/runtime paths
crates/elgar-tui    terminal UI, rendering, input, panes, memory/status views
bin/                local checks, dogfood scripts, install helpers
docs/               architecture notes, planning references, local operating docs
zz_elgar_agent_docs agent instructions, roster, and repo working rules
```

## Quick Start

Run the full local verification path:

```sh
./bin/check-local
```

Run from source:

```sh
cargo run -p elgar-cli -- "hello"
```

Install the local `elgar` command:

```sh
./bin/install-local
elgar
```

When launched interactively with no arguments, `elgar` opens the terminal TUI.
In non-interactive shells it exits safely instead of hanging. The local install
records this repo path so `elgar` can find `elgar-provider.json` and
`AGENTS.md` even when launched from another directory. A provider config in the
current project still takes precedence.

## Provider Mode

Elgar uses LM Studio when `elgar-provider.json` enables live mode and LM Studio
is running locally. Without a live provider config, Elgar falls back to
deterministic no-network behavior.

Force no-network mode:

```sh
ELGAR_PROVIDER_CONFIG=off elgar
```

Optional live-provider smoke details:

```text
docs/live-provider-smoke.md
```

## Permission And Shell Model

Provider output is never treated as filesystem or shell truth. Elgar records a
typed action first, then policy decides whether it can apply immediately or
needs explicit user approval.

Current policy behavior:

- File creation can be policy-applied in permissive modes.
- Edits, deletes, moves, and non-allowlisted shell commands require approval
  unless `full_access` is enabled.
- Read-only allowlisted shell inspection commands can run without manual
  approval.
- Shell proposals show the exact command, cwd, timeout, and approve/reject
  affordance in the TUI.
- Approved shell commands write structured JSONL lifecycle events, including
  command, cwd, timeout, elapsed time, exit/timed-out status, expected paths,
  output byte counts, and safe stdout/stderr tails.

Use explicit slash commands for local control:

```text
/approve
/reject
/permissions
/pending
/status
/memory
/created
/plan
/reasoning
/tokens
/trace
/new
/exit
```

Normal text like “yes”, “approve”, or “create a file” still goes through the
model/runtime path. Local control is slash-only.

## Planning And Memory

Elgar stores verified plan state and verified action facts in the session. It
can answer questions such as:

- what files were created
- what the current plan expects
- which files are present or missing
- what the first/latest created artifact was
- whether facts came from the current session or imported session logs

Plain chat does not receive project memory. Tool, follow-up, and verified-state
turns receive compact verified context derived from `Session::actions()` and
local session JSONL when appropriate.

## Observability

Elgar has human-facing views:

```text
/reasoning
/tokens
/memory
/state
/status
/created
/plan
/trace
```

It also writes machine-readable local JSONL under `.elgar/`:

```text
.elgar/traces/
.elgar/sessions/
```

Logs are append-only and redacted by default: they store routes, request ids,
token counts, action metadata, tool names, paths, command metadata, status,
timings, and error categories. They do not log raw provider prompts or generated
file contents by default.

## Local Checks

Before handing off changes, run:

```sh
./bin/check-local
```

That covers:

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

CI also runs:

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

## Dogfood

Useful local dogfood scripts include:

```text
./bin/dogfood-plan-followup-execution
./bin/dogfood-adhoc-create-roundtrip
./bin/dogfood-session-reopen-memory
./bin/dogfood-plain-memory-regression
./bin/dogfood-plan-scope-unrelated-create
```

Live TUI dogfood remains important because local models vary. Prefer small,
repeatable prompts that exercise one behavior at a time.

## Documentation

Start here:

```text
docs/elgar-product-architecture-plan.md
docs/codex-style-agent-runtime-plan.md
docs/local-checks.md
docs/live-provider-smoke.md
docs/permissioned-shell-commands.md
zz_elgar_agent_docs/AGENTS.md
zz_elgar_agent_docs/AGENT_ROSTER.md
```

Linear is the execution map for active implementation work. The repo-local
architecture plan is the source of truth when external notes are stale.

## Deferred Work

- Project-scoped persistent command approvals: approve once vs always allow in
  this project.
- Intent-scoped tool refinements and shell repair loops after failed commands.
- LM Studio-specific context-window discovery/config sync.
- Session resume, compaction, and branch/fork support.
- Langfuse, Phoenix, or OpenTelemetry exporters.
- Continued split of large runtime/TUI modules once behavior is stable.
