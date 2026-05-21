# Provider Compatibility Metadata

Elgar can read optional provider/model behavior metadata from `elgar-provider.json`.
This metadata is local configuration, not a registry or remote discovery result.

Example:

```json
{
  "provider": "lm-studio",
  "base_url": "http://127.0.0.1:1234/v1",
  "default_model": "openai/gpt-oss-20b",
  "mode": "live",
  "stream": true,
  "compatibility": {
    "context_window_tokens": 128000,
    "output_token_limit_field": "max_tokens",
    "reasoning": {
      "response_fields": ["reasoning_content"],
      "stream_fields": ["reasoning_content", "thinking"]
    },
    "supports_streaming_usage": false,
    "supports_developer_role": false
  }
}
```

All fields inside `compatibility` are optional:

- `context_window_tokens`: context window to display and use for local context accounting.
- `output_token_limit_field`: request field name to use when an output limit is added later. Supported values are `max_tokens`, `max_completion_tokens`, and `max_output_tokens`.
- `reasoning.response_fields`: response message fields known to contain reasoning summaries.
- `reasoning.stream_fields`: streaming delta fields known to contain reasoning summaries.
- `supports_streaming_usage`: whether streaming responses are expected to include usage data.
- `supports_developer_role`: whether Elgar may send the controller instruction as a `developer` message instead of `system`.

Compatibility metadata is intentionally inert unless code consumes a specific
field. Existing top-level `context_window_tokens` remains supported for backward
compatibility, but `compatibility.context_window_tokens` takes precedence when
both are present.
