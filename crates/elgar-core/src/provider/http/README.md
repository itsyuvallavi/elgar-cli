# Provider HTTP

This folder is Elgar's tiny local HTTP client for provider calls.

It exists so LM Studio requests do not need a large HTTP dependency. The code is
limited to localhost/loopback provider URLs.

## Files

- `mod.rs` - wires the HTTP helper modules together.
- `endpoint.rs` - parses provider URLs and rejects non-localhost targets.
- `types.rs` - holds small shared HTTP types: status, response, timeouts.
- `transport.rs` - opens the TCP connection, writes requests, and reads
  non-streaming responses.
- `stream_transport.rs` - reads streaming responses and can stop at provider
  completion without waiting for socket close.
- `response.rs` - parses HTTP status, headers, and chunked response bodies.
- `tests/mod.rs` - tests endpoint validation and response parsing.

## Flow

```text
LM Studio provider
  -> HttpEndpoint::parse(...)
  -> post_json_cancelable(...) or post_json_streaming_cancelable(...)
  -> transport.rs writes/reads TCP
  -> response.rs parses HTTP/chunked body
  -> provider parser turns body into ProviderOutput
```

## Current Scope

This is intentionally small:

- supports `http://` only
- supports localhost and loopback IPs only
- supports JSON POST requests
- supports normal and streaming response reads
- supports chunked transfer decoding
- supports cooperative cancellation while waiting on provider reads

It does not try to be a general-purpose HTTP client.
