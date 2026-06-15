# MCP

## Purpose

Elgar should use MCP to bring external context into the harness without
bypassing the normal contract:

```text
model chooses -> runtime validates -> executor verifies -> logs record -> UI reports
```

MCP is not a macro workflow layer. It is a typed capability source. MCP tool
and resource results must become verified evidence before the model relies on
them.

## Transport Plan

Elgar will support both standard MCP transports:

- `http` for remote servers such as Context7.
- `stdio` for local servers such as Obsidian.

Elgar also supports `internal` for built-in local MCP tools. The current
implementation supports HTTP discovery, read-only HTTP MCP tool calls, and the
internal read-only Project Index MCP. Stdio servers are config-only until the
stdio transport slice lands.

## Config Shape

MCP config lives in `elgar-mcp.json`:

```json
{
  "servers": {
    "context7": {
      "transport": "http",
      "url": "https://mcp.context7.com/mcp",
      "headers": {
        "CONTEXT7_API_KEY": { "env": "CONTEXT7_API_KEY" }
      }
    },
    "obsidian": {
      "transport": "stdio",
      "command": "obsidian-mcp-server",
      "args": [],
      "env": {}
    },
    "project-index": {
      "transport": "internal",
      "kind": "project_index"
    }
  }
}
```

Secrets must be referenced by environment variable name. They must not be
stored directly in config or logs.

## Phases

1. Config and protocol types for `http` and `stdio`.
2. HTTP transport and `elgar mcp list --server context7`.
3. Read-only MCP calls as verified evidence.
4. Bounded MCP result memory.
5. Internal Project Index MCP.
6. Stdio transport and Obsidian dogfood.
7. Approval-gated MCP side effects.

## Safety Rules

- Do not hardcode natural-language triggers for MCP servers.
- Do not trust MCP tool descriptions from untrusted servers as policy truth.
- Do not expose side-effect MCP tools without approval.
- Do not log secret header values or environment values.
- Keep all MCP results bounded before adding them to prompt context.

## First Connectivity Target

Context7 is the first HTTP target because it exposes documentation context over
remote MCP. Its documented endpoint is:

```text
https://mcp.context7.com/mcp
```

Expected read-only tools include:

- `resolve-library-id`
- `query-docs`

Obsidian comes after HTTP because local stdio adds process lifecycle and vault
semantics on top of the base protocol.

## Diagnostic Command

HTTP connectivity is introduced through:

```text
elgar mcp list --server context7
```

The command is diagnostic-only. It loads `elgar-mcp.json`, initializes the
server, sends `notifications/initialized`, lists declared tools/resources, and
prints a compact summary. It does not call the model.

## Local Logs

MCP diagnostics write to the existing system JSONL log:

```text
.elgar/log/system/mcp-diagnostic-*.jsonl
```

Current MCP summaries:

- `mcp_config_loaded`
- `mcp_http_request_started`
- `mcp_http_request_finished`
- `mcp_http_request_failed`
- `mcp_initialize_finished`
- `mcp_tools_listed`
- `mcp_resources_listed`
- `mcp_tool_call_started`
- `mcp_tool_call_finished`
- `mcp_tool_call_failed`

The logs include server id, transport, method, duration, status, and counts.
They must not include header values, environment values, or full response
bodies.

## Model Access

The first model-facing MCP capability is generic:

```text
mcp_call
```

The model calls it with an exact configured server id, tool name, and argument
object:

```json
{
  "server": "context7",
  "tool": "query-docs",
  "arguments": {
    "libraryId": "/vercel/next.js",
    "query": "middleware auth"
  }
}
```

`mcp_call` is not Context7-specific. It works for any configured MCP server
once the transport is supported. In this slice, HTTP MCP servers and Elgar's
built-in `internal` Project Index server are executable from the harness; stdio
servers remain config-only.

Before the model chooses tools, the harness renders a bounded live catalog of
configured MCP servers and their advertised `tools/list` names, descriptions,
and input schemas into the system prompt. This is generic discovery output, not
a hardcoded server trigger table.

Runtime validation still applies:

- the MCP config must exist
- the server id must exist
- the server must use a supported transport
- the requested tool must appear in `tools/list`
- `arguments` must be a JSON object
- returned content is bounded before it is sent back to the model

The harness returns successful calls as verified evidence with labels like:

```text
mcp:context7:query-docs:<argument-fingerprint>
```

MCP duplicate detection is argument-aware. An exact repeated `mcp_call` is
blocked as a duplicate, but the same server/tool with a different query or
argument object is allowed so the model can refine a search.

Malformed `mcp_call` requests are returned as verified feedback with labels
like:

```text
invalid_mcp_call:<request-fingerprint>
```

This means the provider emitted an invalid `mcp_call` shape, not that the MCP
server or remote tool failed. Valid `mcp_call` requests require top-level
`server`, top-level `tool`, and a top-level `arguments` object.

## Internal Project Index

The internal Project Index server is configured as:

```json
{
  "servers": {
    "project-index": {
      "transport": "internal",
      "kind": "project_index"
    }
  }
}
```

It exposes read-only project inspection tools through the same generic
`mcp_call` tool:

- `project_tree` returns a bounded directory summary.
- `project_find` finds bounded paths by name pattern.
- `project_read_summary` reads one bounded UTF-8 file.
- `project_status` summarizes current session counts and pending approval
  state.

Project Index paths must be relative to the launch folder. Absolute paths and
parent-directory segments are rejected as verified MCP tool errors.
