# Elgar Architecture

## Mental Model

```text
CLI = front door
TUI = terminal interface
Core = runtime engine
Provider = model connection
Logs = local audit trail
```

## Startup Flow

```text
elgar
-> crates/elgar-cli/src/main.rs
-> elgar-cli/src/startup/
-> elgar-tui
-> elgar-core harness/provider/session
```

`elgar-core` does not start the app. It is the reusable engine used by CLI,
TUI, tests, and future surfaces.

## Harness Flow

```text
terminal input
-> TUI input handling
-> core harness turn
-> model either answers or requests native primitive tool calls
-> runtime validates and executes read-only primitives
-> verified results return as provider tool messages
-> model final text
-> session events
-> TUI renders visible text
```

Current plain chat must not bypass the harness, use macro tools, inject durable
memory, or run folder anchoring.

The target harness direction is the native provider tool loop documented in
`NATIVE_TOOL_LOOP.md`: native `tool_calls` first, Rust validation/execution,
provider `tool` result messages, then final text as the normal loop end.

## Crate Responsibilities

`elgar-cli`:

- parse process arguments
- find runtime paths
- load provider config
- launch TUI
- expose diagnostic commands

`elgar-tui`:

- read terminal input
- handle local slash commands
- render conversation state
- keep terminal-only UI state

`elgar-core`:

- provider types and requests
- harness turn execution
- sessions and events
- token/context accounting
- logs
- future runtime capabilities

## Current Slash Commands

Current local commands include:

```text
/cancel
/clear
/new
/details last
/copy
/copy raw
/help
/commands
/exit
/quit
/q
```

Unknown slash commands are local errors. Plain text without `/` goes to the
model.

## Future Agent Path

Future tools should plug into core as typed capability layers. The model may
request typed tools, but the runtime must validate, policy must decide, and
executors must verify before the UI reports success.
