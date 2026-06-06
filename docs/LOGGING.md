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
