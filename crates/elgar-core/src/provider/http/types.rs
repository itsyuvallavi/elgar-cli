//! Shared HTTP helper types.
//!
//! These small types keep status, response body, and timeout values explicit
//! across endpoint parsing, transport, and response parsing.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::provider) struct HttpStatusCode(pub(in crate::provider) u16);

impl HttpStatusCode {
    pub(in crate::provider) fn as_u16(self) -> u16 {
        self.0
    }

    pub(in crate::provider) fn is_success(self) -> bool {
        (200..300).contains(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::provider) struct HttpResponse {
    pub status_code: HttpStatusCode,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::provider) struct HttpTimeouts {
    pub(in crate::provider) connect: Duration,
    pub(in crate::provider) read: Duration,
    pub(in crate::provider) write: Duration,
    pub(in crate::provider) request: Duration,
}

impl HttpTimeouts {
    /// Creates phase-specific HTTP timeouts from config values.
    pub(in crate::provider) fn from_millis(
        connect_millis: u64,
        read_millis: u64,
        write_millis: u64,
        request_millis: u64,
    ) -> Self {
        Self {
            connect: duration_from_millis(connect_millis),
            read: duration_from_millis(read_millis),
            write: duration_from_millis(write_millis),
            request: duration_from_millis(request_millis),
        }
    }
}

fn duration_from_millis(millis: u64) -> Duration {
    Duration::from_millis(millis.max(1))
}
