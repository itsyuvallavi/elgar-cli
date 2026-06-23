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
-> provider stream chunks are recorded and may update active TUI preview
-> runtime validates and executes read-only primitives
-> risky primitive calls create pending approval records
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
- MCP config/protocol foundations
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

`/cancel` is a runtime cancellation command: it signals the active provider
request through the harness/provider path and drops any canceled turn output.

## Future Agent Path

Future tools should plug into core as typed capability layers. The model may
request typed tools, but the runtime must validate, policy must decide, and
executors must verify before the UI reports success.

MCP follows the same rule. MCP servers provide typed tools and resources, but
Elgar must validate, bound, log, and treat MCP results as verified evidence
before adding them to model context.

The first model-facing MCP path is the generic `mcp_call` capability. It takes
an exact server id, MCP tool name, and JSON argument object. Elgar validates the
configured server and tool before calling MCP, then returns bounded verified
evidence to the model.

When MCP is configured, the native loop system prompt includes a bounded live
catalog of advertised server tools and input schemas. The catalog comes from
`tools/list`; it is not a hardcoded Context7 or Obsidian trigger table.

## Approval Boundary

Core owns approval truth. A pending approval records the exact typed risky
action, or a small serial batch of exact typed risky actions from one provider
response. The TUI renders that record but does not interpret model prose.

The default permission mode requires approval for `bash`, `write`, and `edit`.
The explicit `workspace_write` mode can auto-execute safe relative `write`
requests inside the launch folder. This mode does not auto-run `bash`, `edit`,
absolute paths, parent paths, symlink paths, or outside-folder writes.
The explicit `full_access` mode is trusted local execution: launch-folder
`write`, `edit`, and `bash` requests can run without approval, while unsafe
paths remain rejected by execution checks.

One user approval can approve a batch, but core still executes the stored steps
one at a time and logs verified output for each step. If the provider only asks
for approval in prose, no approval exists until a real typed tool call is
created.
`/approve continue` is an opt-in approval command that executes the pending
approval and then starts one generic follow-up harness turn.
