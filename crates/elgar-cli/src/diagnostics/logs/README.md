# diagnostics/logs

## Purpose

Read existing `.elgar/log/system` JSONL files and render the latest turn or
harness-loop summary for humans.

## Files

- `mod.rs` exposes the `elgar logs latest` command API.
- `scan.rs` finds newest system logs and reads legacy turn summaries.
- `summary.rs` extracts current harness-loop diagnostics from JSONL events.
- `render.rs` formats summaries for terminal output.
- `types.rs` defines diagnostic errors.

## Rule

This folder is read-only. It must not create logs, call providers, or decide
runtime behavior.
