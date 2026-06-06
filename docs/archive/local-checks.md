# Local Checks

Use `./bin/check-local` before handing off Core, CLI, TUI, or provider-adjacent
changes.

The command runs the fast no-network verification path:

```sh
./bin/check-local
```

It runs:

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace`

It does not require LM Studio and does not run live provider smoke commands such
as `provider-smoke`.

## Performance Baseline

Use the local performance baseline separately when you need wall-clock numbers:

```sh
./bin/perf-baseline
```

It remains no-network by default and measures TUI render/update latency, stub
provider phases, and one `./bin/check-local` wall-time run. It is not part of
the default local check path.

Use the latest live trace performance summary after a TUI/provider smoke:

```sh
./bin/perf-trace
```

It reads `.elgar/traces/*.jsonl` and renders the newest `turn_perf_summary`
without calling the provider.
