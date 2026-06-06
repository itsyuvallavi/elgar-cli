//! Minimal local HTTP client for provider requests.
//!
//! Elgar talks to LM Studio on localhost. This folder owns URL parsing, TCP
//! connection timeouts, request writing, response reading, and HTTP/chunk
//! parsing without pulling in a larger HTTP client.

mod endpoint;
mod response;
mod transport;
mod types;

pub(super) use endpoint::HttpEndpoint;
pub(super) use transport::{post_json, post_json_streaming};
pub(super) use types::HttpTimeouts;

#[cfg(test)]
pub(super) use response::parse_http_response;

#[cfg(test)]
mod tests;
