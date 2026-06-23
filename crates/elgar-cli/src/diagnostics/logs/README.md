# diagnostics/logs

## Purpose

Read existing `.elgar/log/system` JSONL files and render the latest turn or
harness-loop summary for humans.

## Files

- `mod.rs` exposes the `elgar logs latest` command API.
- `follow.rs` tails the newest system JSONL file and renders compact request
  lifecycle lines for live debugging.
- `follow_render.rs` formats individual followed JSONL events, including
  provider timing, memory, session context, MCP status, and approval status.
- `scan.rs` finds newest system logs and reads legacy turn summaries.
- `summary.rs` extracts current harness-loop diagnostics from JSONL events.
- `render.rs` formats summaries for terminal output.
- `types.rs` defines diagnostic errors.

## Rule

This folder is read-only. It must not create logs, call providers, or decide
runtime behavior.

## Commands

- `elgar logs latest` prints the latest completed harness or legacy summary,
  including memory, context, and MCP status when those events exist.
- `elgar logs --follow` keeps running and prints request start, first streamed
  chunk, provider close, worker receipt, render, memory/context/MCP status, and
  error events as they appear.
