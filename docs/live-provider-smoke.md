# Live Provider Smoke Commands

Elgar's normal CLI and controller path remains no-network by default. Live LM
Studio calls are only made through the explicit smoke commands below.

Before running either command, start LM Studio, load the model, and set
`ELGAR_LM_STUDIO_MODEL` to the actual loaded model name shown by LM Studio. Do
not leave it as a placeholder.

`ELGAR_LM_STUDIO_BASE_URL` is optional. When unset, Elgar uses
`http://127.0.0.1:1234/v1`.

## Provider Config Decision

For the current smoke stage, keep provider configuration in environment
variables.

Use:

- `ELGAR_LM_STUDIO_MODEL`: required loaded LM Studio model name
- `ELGAR_LM_STUDIO_BASE_URL`: optional OpenAI-compatible base URL

Do not add JSON config yet. The smoke commands are explicit, short-lived, and
developer-run, so env vars keep live provider access visible and avoid creating
a persistent provider mode before the CLI and TUI are ready to share it.

Add JSON config when provider mode becomes persistent or shared, such as:

- normal CLI or TUI sessions need to remember a provider/model between runs
- CLI and TUI need one common provider configuration source
- more than one provider/backend is selectable
- provider health/model-list checks become part of normal startup
- user-facing provider settings need validation, display, or editing

Until then, default CLI/TUI paths remain no-network/stub, and live provider
access remains opt-in through the smoke commands below.

## Next Slice Decision

After the explicit TUI live smoke path, the next product slice should be the
existing Harness issue `ELG-129 Create fast local check command`.

This should come before deeper TUI polish, more provider commands, or new
permissioned action types because the project now needs one repeatable
no-network guardrail for controller truth, action lifecycle, renderer behavior,
and CLI/TUI boundaries.

## Provider Smoke

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
cargo run -p elgar-cli -- provider-smoke "Say hello in one sentence."
```

With a custom LM Studio base URL:

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
ELGAR_LM_STUDIO_BASE_URL="http://127.0.0.1:1234/v1" \
cargo run -p elgar-cli -- provider-smoke "Say hello in one sentence."
```

## Controller Smoke

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
cargo run -p elgar-cli -- controller-smoke "Say hello in one sentence."
```

With a custom LM Studio base URL:

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
ELGAR_LM_STUDIO_BASE_URL="http://127.0.0.1:1234/v1" \
cargo run -p elgar-cli -- controller-smoke "Say hello in one sentence."
```

Expected successful controller smoke shape:

```text
user: Say hello in one sentence.
provider started: lm-studio request lm-studio-request-1
provider finished: lm-studio request lm-studio-request-1: Hello!
assistant Provider: Hello!
```

## TUI Controller Smoke

This renders the same explicit live controller path through `TuiShell`, so the
output uses TUI conversation/status copy. The normal TUI smoke path remains
stub/no-network.

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
cargo run -p elgar-cli -- tui-controller-smoke "Say hello in one sentence."
```

With a custom LM Studio base URL:

```sh
ELGAR_LM_STUDIO_MODEL="actual-loaded-model-name" \
ELGAR_LM_STUDIO_BASE_URL="http://127.0.0.1:1234/v1" \
cargo run -p elgar-cli -- tui-controller-smoke "Say hello in one sentence."
```
