# Elgar

Elgar is a local-first Rust terminal chat harness.

Current baseline:

```text
user prompt -> harness -> native tool calls if needed -> model answer -> visible response
```

The project is being rebuilt into a fuller coding agent one capability at a
time. The current stable route is a read-only primitive harness.

## Run

Install the local binary:

```sh
./bin/install-local
```

Run the TUI:

```sh
elgar
```

Run one CLI prompt:

```sh
cargo run -p elgar-cli -- "hello"
```

Disable live provider config:

```sh
ELGAR_PROVIDER_CONFIG=off elgar
```

## Current Architecture

```text
elgar-cli  starts the app
elgar-tui  owns terminal input/rendering
elgar-core owns provider/session/runtime logic
```

All plain text goes through the harness. The active executable primitives are
read-only: `read`, `ls`, `find`, and `grep`. Shell, write/edit, permissions,
planning, and durable memory are future layers.

## Docs

Start here:

```text
docs/PROJECT_PLAN.md
docs/ARCHITECTURE.md
docs/FILE_MAP.md
docs/LOCAL_CHECKS.md
docs/PROVIDER.md
docs/LOGGING.md
docs/TUI.md
```

Agent instructions:

```text
AGENTS.md
docs/agent/AGENTS.md
```

Historical docs live in:

```text
docs/archive/
```

Generated maps live in:

```text
docs/maps/
```

## Checks

```sh
cargo fmt
cargo test -p elgar-cli
cargo test -p elgar-tui
cargo test -p elgar-core
```
