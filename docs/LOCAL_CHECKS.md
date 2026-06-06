# Local Checks

## Fast Checks

Format:

```sh
cargo fmt
```

Check CLI:

```sh
cargo check -p elgar-cli
```

Check TUI:

```sh
cargo check -p elgar-tui
```

Check core:

```sh
cargo check -p elgar-core
```

## Tests

CLI tests:

```sh
cargo test -p elgar-cli
```

TUI tests:

```sh
cargo test -p elgar-tui
```

Core tests:

```sh
cargo test -p elgar-core
```

## Local Install

Install the current repo binary:

```sh
./bin/install-local
```

Then run:

```sh
elgar
```

## Full Local Script

The historical all-in-one script is:

```sh
./bin/check-local
```

Use it after reviewing `bin/`; some scripts may still reference archived
features.
