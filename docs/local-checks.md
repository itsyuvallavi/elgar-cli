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
as `provider-smoke`, `controller-smoke`, or `tui-controller-smoke`.
