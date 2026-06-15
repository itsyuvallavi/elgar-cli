# MCP

This folder owns Model Context Protocol types and runtime helpers.

Current slice:

- config types for `http` and `stdio` servers
- JSON-RPC protocol types
- initialize, initialized, tools/list, and resources/list request builders
- HTTP discovery for configured remote MCP servers
- read-only HTTP tool calls through the harness `mcp_call` capability
- read-only internal Project Index tool calls through `mcp_call`
- system JSONL diagnostics for MCP config, HTTP, discovery, and tool-call events

Out of scope for this slice:

- stdio subprocesses
- side-effect MCP approval and execution
- durable MCP result memory beyond verified evidence labels

MCP must remain typed, logged, bounded, and evidence-based before it is exposed
to the model.
