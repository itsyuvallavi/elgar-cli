# Live Provider Configuration and Smoke Commands

Elgar can use LM Studio in normal CLI and terminal TUI runs when a local provider
config file enables live mode. Without that file, Elgar falls back to
stub/no-network mode.

Before running either command, start LM Studio, load the model, and set
`ELGAR_LM_STUDIO_MODEL` to the actual loaded model name shown by LM Studio. Do
not leave it as a placeholder.

`ELGAR_LM_STUDIO_BASE_URL` is optional. When unset, Elgar uses
`http://127.0.0.1:1234/v1`.

## Normal CLI/TUI Config

The repo-local config file is:

```text
elgar-provider.json
```

Current local config:

```json
{
  "provider": "lm-studio",
  "base_url": "http://127.0.0.1:1234/v1",
  "default_model": "openai/gpt-oss-20b",
  "mode": "live",
  "connect_timeout_millis": 2000,
  "read_timeout_millis": 120000,
  "write_timeout_millis": 5000,
  "request_timeout_millis": 180000,
  "stream": true
}
```

The model name must match the loaded model name shown in LM Studio.
The timeout fields are explicit: connect is short because LM Studio is local,
while read/request are longer so slower local generations do not fail at 30s.

When `mode` is `live`, normal CLI text and `tui-terminal` use the configured LM
Studio model. The line-oriented `tui` command remains a stub/no-network harness
path for now.

To temporarily disable this file without editing it:

```sh
ELGAR_PROVIDER_CONFIG=off cargo run -p elgar-cli -- tui-terminal
```

Smoke commands still support environment variables:

- `ELGAR_LM_STUDIO_MODEL`: required loaded LM Studio model name
- `ELGAR_LM_STUDIO_BASE_URL`: optional OpenAI-compatible base URL

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

## Manual Terminal TUI Dogfood

This is optional and live. It is not part of `./bin/check-local`.

```sh
cargo run -p elgar-cli -- tui-terminal
```

Use this short checklist:

- Ask `hello` and confirm LM Studio answers.
- Ask `what can you do?` and confirm the reply stays concise.
- Run `/copy` and confirm the copied-message hint appears.
- Run `/clear` and confirm only the visible conversation clears.
- Ask `create file manual-dogfood.md`, then run `/reject` and confirm no file is written.
- Ask `create file manual-dogfood.md` again, then run `/approve` and confirm the file is written.
- Run `/q` and confirm the terminal exits cleanly.

## LM Studio Latency And Prompt Cache Notes

The latest local trace for a short `what can you do?` answer showed LM Studio
reporting prompt cache reuse as `0/214` tokens and taking about five seconds.
Treat that as provider-side behavior: `0/N` means LM Studio did not reuse a
prompt prefix for that request, even though Elgar keeps the fixed controller
system prompt compact.

Elgar records no-network-safe request metadata and, on live calls, provider
metrics such as serialized request bytes, token usage when returned by the
OpenAI-compatible response, first chunk latency for streaming calls, and total
duration. These metrics can identify slow local provider turns, but they do not
prove why LM Studio did or did not reuse its prompt cache.

## Timeout And Cancel Semantics

`timeout_millis` remains the legacy fallback. The preferred fields are:

- `connect_timeout_millis`: opening the local socket.
- `write_timeout_millis`: sending the JSON request.
- `read_timeout_millis`: waiting between response reads.
- `request_timeout_millis`: total request budget.

`/cancel` drops visible/session updates from the active provider turn. It does
not yet abort an already-running provider socket immediately.
