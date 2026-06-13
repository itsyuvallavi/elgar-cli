# Logging

## Purpose

Logs should answer:

```text
what happened during this turn?
```

They should not create multiple competing sources of truth.

## Current Local Folders

```text
.elgar/log/sessions/
.elgar/log/system/
```

Session logs are model/user/provider event history.

System logs are runtime flow/timing/error diagnostics.

MCP diagnostics also write to system logs. Example summaries:

```text
mcp_config_loaded
mcp_http_request_started
mcp_http_request_finished
mcp_http_request_failed
mcp_initialize_finished
mcp_tools_listed
mcp_resources_listed
mcp_tool_call_started
mcp_tool_call_finished
mcp_tool_call_failed
```

MCP logs should include method names, status codes, durations, and counts, but
not auth headers, environment values, or full response bodies.

## Rules

- Keep logs local by default.
- Prefer JSONL for machine-readable history.
- Do not log raw secrets.
- Do not log full generated file contents by default.
- Keep session history and system diagnostics separate.

## Future

Sentry or another hosted diagnostic sink may be useful later for:

- crashes
- panics
- provider failures
- HTTP errors
- unexpected runtime states

Do not add hosted logging until the local log shape is stable.
