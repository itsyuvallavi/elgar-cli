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
        response::{
            find_header_body_split, has_chunked_transfer_encoding, parse_http_response,
            parse_status_code, process_streaming_body_bytes, StreamingHeader,
        },
        types::{HttpResponse, HttpTimeouts},
    },
    types::ProviderError,
    ProviderCancelToken,
};

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
struct RequestDeadline {
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

    fn remaining(self, phase: &str) -> Result<Duration, ProviderError> {
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
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
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

fn read_streaming_http_response(
    mut stream: TcpStream,
    timeouts: HttpTimeouts,
    deadline: RequestDeadline,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
    cancel: &ProviderCancelToken,
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];
    let mut header: Option<StreamingHeader> = None;
    let mut body = Vec::new();
    let mut chunk_buffer = Vec::new();
    let mut chunked_complete = false;
    let mut idle_started = Instant::now();

    loop {
        cancel.error_if_canceled()?;
        deadline.remaining("stream read")?;
        stream
            .set_read_timeout(Some(
                timeouts
                    .read
                    .min(deadline.remaining("stream read")?)
                    .min(CANCEL_POLL_INTERVAL),
            ))
            .map_err(|error| ProviderError::network(error.to_string()))?;
        let read = match stream.read(&mut read_buffer) {
            Ok(read) => read,
            Err(error) if is_read_timeout(&error) => {
                cancel.error_if_canceled()?;
                deadline.remaining("stream read")?;
                if idle_started.elapsed() >= timeouts.read {
                    return Err(read_error(error, "stream read", timeouts.read));
                }
                continue;
            }
            Err(error) => return Err(read_error(error, "stream read", timeouts.read)),
        };
        if read == 0 {
            break;
        }
        idle_started = Instant::now();

        bytes.extend_from_slice(&read_buffer[..read]);
        if header.is_none() {
            let Some(split) = find_header_body_split(&bytes) else {
                continue;
            };
            let head = String::from_utf8(bytes[..split].to_vec())
                .map_err(|error| ProviderError::response_parse(error.to_string()))?;
            let status_code = parse_status_code(head.lines().next().ok_or_else(|| {
                ProviderError::response_parse("HTTP response missing status line")
            })?)?;
            let is_chunked = has_chunked_transfer_encoding(&head);
            header = Some(StreamingHeader {
                status_code,
                is_chunked,
            });
            let tail = bytes[(split + 4)..].to_vec();
            bytes.clear();
            process_streaming_body_bytes(
                &tail,
                header.as_ref().unwrap(),
                &mut body,
                &mut chunk_buffer,
                &mut chunked_complete,
                on_body_chunk,
            )?;
        } else if let Some(header) = header.as_ref() {
            process_streaming_body_bytes(
                &read_buffer[..read],
                header,
                &mut body,
                &mut chunk_buffer,
                &mut chunked_complete,
                on_body_chunk,
            )?;
        }
    }

    let header =
        header.ok_or_else(|| ProviderError::response_parse("HTTP response missing headers"))?;
    if header.is_chunked && !chunked_complete {
        return Err(ProviderError::response_parse(
            "chunked body ended before terminal chunk",
        ));
    }
    let body = String::from_utf8(body)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    Ok(HttpResponse {
        status_code: header.status_code,
        body,
    })
}

fn read_error(error: io::Error, phase: &str, timeout: Duration) -> ProviderError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => ProviderError::network(format!(
            "provider {phase} timed out after {}ms",
            timeout.as_millis()
        )),
        _ => ProviderError::network(error.to_string()),
    }
}

fn is_read_timeout(error: &io::Error) -> bool {
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
