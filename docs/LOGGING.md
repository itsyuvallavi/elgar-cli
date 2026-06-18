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

Provider streaming is logged before request completion:

```text
provider_stream_chunk                 # session log event
harness_loop_provider_stream_chunk     # tool-decision system log event
harness_synthesis_provider_stream_chunk # synthesis system log event
```

Session logs retain chunk events, including reasoning/text/tool-call-delta
kind and sequence. System logs keep bounded previews and counts for diagnosis.
If a provider request is canceled before `provider_finished`, streamed chunks
record what Elgar actually received before cancellation.

Completed provider requests also include stream timing fields on
`provider_finished` / provider-finished system events:

```text
first_reasoning_ms
first_text_ms
last_reasoning_ms
last_text_ms
last_chunk_ms
reasoning_to_text_ms
stream_done_ms
last_chunk_to_done_ms
done_to_finish_ms
last_chunk_to_finish_ms
total_stream_ms
```

These values distinguish slow first-token latency from a long reasoning phase
before visible answer text. `stream_done_ms` records when the provider sent the
OpenAI-compatible `data: [DONE]` sentinel. `last_chunk_to_done_ms` isolates the
gap between the last visible chunk and protocol completion.
`done_to_finish_ms` should stay near zero for the OpenAI-compatible path; if it
grows, Elgar is doing work after protocol completion. `last_chunk_to_finish_ms`
remains as the end-to-end gap from last streamed reasoning/text/tool chunk to
the finished provider event.

Provider/TUI handoff diagnostics also include request IDs and event counts:

```text
harness_loop_provider_call_finished   # core received full provider response
session_event_recorded                # session event persisted
provider_worker_finished              # background TUI worker finished harness turn
provider_worker_completion_received   # foreground TUI received worker result
tui_live_preview_rendered             # active inline streamed preview rendered
tui_live_preview_finalized            # live preview preserved or replaced
ui_render_finished                    # interactive TUI rendered new lines
scripted_tui_render_finished          # scripted TUI rendered transcript state
```

Use `latest_provider_request_id` plus timestamps to compare LM Studio's visible
request state with Elgar's internal handoff. The important gaps are:

```text
harness_loop_provider_call_finished -> session_event_recorded(assistant_message)
provider_worker_finished -> provider_worker_completion_received
provider_worker_completion_received -> ui_render_finished
```

Those gaps separate provider transport timing from worker delivery and terminal
rendering.

For interactive turns, `tui_live_preview_finalized` records whether the live
answer preview matched the final rendered provider message:

```text
preview_matched_final
preserved_preview
final_content_changed
live_preview_chars
final_chars
finalize_render_ms
```

If `preserved_preview=true`, the TUI kept the streamed answer already on screen
and avoided appending a late response-timing line under the answer. If
`final_content_changed=true`, the TUI used the full final render path.

For live terminal review while running dogfoods, use:

```text
elgar logs --follow
```

Run it in a second terminal. It tails `.elgar/log/system` and prints compact
request lifecycle lines without calling the model or writing new logs. On first
attach it starts at the end of the newest existing system log so reopening the
follower does not replay stale events. If a newer log file appears while the
follower is already running, it reads that new file from the beginning.

`elgar logs --follow` also prints compact state lines for the diagnostics that
are hard to see from the TUI alone:

```text
memory indexed=6 rendered=3 omitted=3 chars=176 budget_hit=false history=2
tokens turn ↑1.2k ↓43 = 1.2k · session 4.6k/128k (3%) · mode review_all
mcp active servers=project-index,context7 source=elgar-mcp.json
mcp inactive
approval pending write approval-1 target=hello-world.md scope=inside_launch_folder
```

These lines are derived from JSONL events only. They do not add model context,
call providers, or create a second source of truth.

Session context events are written as `harness_session_context_status` after
provider metrics are recorded. They include cumulative provider-reported token
usage for the active session, the configured context window when known, the
permission mode, and compact pending-approval status. This is a running session
usage indicator, not a promise that every historical token is still present in
the next prompt.

Memory context events are written as `harness_turn_prompt_context_built`. They
include indexed/rendered/omitted fact counts, rendered memory characters,
history turn count, and whether the memory budget was hit.

MCP availability is written as `harness_mcp_status` at harness-loop setup. It
records active/inactive state, config source, server ids, and exposed tool
count. It must not log secrets or full MCP response bodies.

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
