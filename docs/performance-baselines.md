# Local Performance Baselines

Use this command when you need a no-network baseline before TUI, provider, or
harness changes:

```sh
./bin/perf-baseline
```

It reports:

- TUI render/update latency for fixed transcript sizes.
- Stub provider phase timing for request start to first read and completion.
- `./bin/check-local` wall time.

The command is intended for local comparisons. Timings vary by machine and
current load, so compare runs from the same machine instead of treating the
numbers as universal thresholds.

The command does not call LM Studio or any live provider. Live LM Studio
performance remains a manual follow-up using the smoke commands in
`docs/live-provider-smoke.md`.

The command writes no project files. It prints the baseline to stdout and runs
the same no-network cargo checks as `./bin/check-local`.
