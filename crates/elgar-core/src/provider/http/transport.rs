//! Sends local HTTP requests over TCP.
//!
//! This module owns connection setup, request writing, normal response reading,
//! streaming response reading, and timeout error messages.

use std::{
    io,
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

use crate::provider::{
    http::{
        endpoint::HttpEndpoint,
        response::parse_http_response,
        stream_transport::read_streaming_http_response,
        types::{HttpResponse, HttpTimeouts, StreamingBodyAction},
    },
    types::ProviderError,
    ProviderCancelToken,
};

pub(super) const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub(super) struct RequestDeadline {
    started: Instant,
    timeout: Duration,
}

impl RequestDeadline {
    fn new(timeout: Duration) -> Self {
        Self {
            started: Instant::now(),
            timeout,
        }
    }

    pub(super) fn remaining(self, phase: &str) -> Result<Duration, ProviderError> {
        let elapsed = self.started.elapsed();
        if elapsed >= self.timeout {
            return Err(request_timeout_error(phase, self.timeout));
        }

        Ok(self.timeout - elapsed)
    }
}

/// Sends one JSON request and returns the full HTTP response body.
pub(in crate::provider) fn post_json_cancelable(
    endpoint: &HttpEndpoint,
    body: &str,
    timeouts: HttpTimeouts,
    cancel: &ProviderCancelToken,
) -> Result<HttpResponse, ProviderError> {
    cancel.error_if_canceled()?;
    let deadline = RequestDeadline::new(timeouts.request);
    let mut stream = connect_with_timeout(
        endpoint,
        timeouts.connect.min(deadline.remaining("connect")?),
    )?;
    cancel.error_if_canceled()?;
    stream
        .set_read_timeout(Some(timeouts.read.min(deadline.remaining("read")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeouts.write.min(deadline.remaining("write")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    cancel.error_if_canceled()?;
    write_json_request(&mut stream, endpoint, body, "application/json")?;
    cancel.error_if_canceled()?;

    read_http_response(stream, timeouts, deadline, cancel)
}

/// Sends a JSON request and forwards response-body chunks while reading.
pub(in crate::provider) fn post_json_streaming_cancelable(
    endpoint: &HttpEndpoint,
    body: &str,
    timeouts: HttpTimeouts,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<StreamingBodyAction, ProviderError>,
    cancel: &ProviderCancelToken,
) -> Result<HttpResponse, ProviderError> {
    cancel.error_if_canceled()?;
    let deadline = RequestDeadline::new(timeouts.request);
    let mut stream = connect_with_timeout(
        endpoint,
        timeouts.connect.min(deadline.remaining("connect")?),
    )?;
    cancel.error_if_canceled()?;
    stream
        .set_read_timeout(Some(timeouts.read.min(deadline.remaining("read")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeouts.write.min(deadline.remaining("write")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    write_json_request(
        &mut stream,
        endpoint,
        body,
        "text/event-stream, application/json",
    )?;
    cancel.error_if_canceled()?;

    read_streaming_http_response(stream, timeouts, deadline, on_body_chunk, cancel)
}

fn write_json_request(
    stream: &mut TcpStream,
    endpoint: &HttpEndpoint,
    body: &str,
    accept: &str,
) -> Result<(), ProviderError> {
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.path,
        endpoint.authority(),
        accept,
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ProviderError::network(error.to_string()))
}

fn connect_with_timeout(
    endpoint: &HttpEndpoint,
    timeout: Duration,
) -> Result<TcpStream, ProviderError> {
    TcpStream::connect_timeout(&endpoint.socket_addr()?, timeout).map_err(|error| {
        match error.kind() {
            io::ErrorKind::TimedOut => ProviderError::network(format!(
                "provider connect timed out after {}ms",
                timeout.as_millis()
            )),
            _ => ProviderError::network(error.to_string()),
        }
    })
}

fn read_http_response(
    mut stream: TcpStream,
    timeouts: HttpTimeouts,
    deadline: RequestDeadline,
    cancel: &ProviderCancelToken,
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];
    let mut idle_started = Instant::now();

    loop {
        cancel.error_if_canceled()?;
        deadline.remaining("read")?;
        stream
            .set_read_timeout(Some(
                timeouts
                    .read
                    .min(deadline.remaining("read")?)
                    .min(CANCEL_POLL_INTERVAL),
            ))
            .map_err(|error| ProviderError::network(error.to_string()))?;
        let read = match stream.read(&mut read_buffer) {
            Ok(read) => read,
            Err(error) if is_read_timeout(&error) => {
                cancel.error_if_canceled()?;
                deadline.remaining("read")?;
                if idle_started.elapsed() >= timeouts.read {
                    return Err(read_error(error, "read", timeouts.read));
                }
                continue;
            }
            Err(error) => return Err(read_error(error, "read", timeouts.read)),
        };
        if read == 0 {
            break;
        }
        idle_started = Instant::now();
        bytes.extend_from_slice(&read_buffer[..read]);
    }

    let raw = String::from_utf8(bytes)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    parse_http_response(&raw)
}

pub(super) fn read_error(error: io::Error, phase: &str, timeout: Duration) -> ProviderError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => ProviderError::network(format!(
            "provider {phase} timed out after {}ms",
            timeout.as_millis()
        )),
        _ => ProviderError::network(error.to_string()),
    }
}

pub(super) fn is_read_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn request_timeout_error(phase: &str, timeout: Duration) -> ProviderError {
    ProviderError::network(format!(
        "provider request timed out during {phase} after {}ms",
        timeout.as_millis()
    ))
}
