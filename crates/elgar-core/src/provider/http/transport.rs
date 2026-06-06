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
};

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
pub(in crate::provider) fn post_json(
    endpoint: &HttpEndpoint,
    body: &str,
    timeouts: HttpTimeouts,
) -> Result<HttpResponse, ProviderError> {
    let deadline = RequestDeadline::new(timeouts.request);
    let mut stream = connect_with_timeout(
        endpoint,
        timeouts.connect.min(deadline.remaining("connect")?),
    )?;
    stream
        .set_read_timeout(Some(timeouts.read.min(deadline.remaining("read")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeouts.write.min(deadline.remaining("write")?)))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    write_json_request(&mut stream, endpoint, body, "application/json")?;

    read_http_response(stream, timeouts, deadline)
}

/// Sends a JSON request and forwards response-body chunks while reading.
pub(in crate::provider) fn post_json_streaming(
    endpoint: &HttpEndpoint,
    body: &str,
    timeouts: HttpTimeouts,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
) -> Result<HttpResponse, ProviderError> {
    let deadline = RequestDeadline::new(timeouts.request);
    let mut stream = connect_with_timeout(
        endpoint,
        timeouts.connect.min(deadline.remaining("connect")?),
    )?;
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

    read_streaming_http_response(stream, timeouts, deadline, on_body_chunk)
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
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];

    loop {
        deadline.remaining("read")?;
        stream
            .set_read_timeout(Some(timeouts.read.min(deadline.remaining("read")?)))
            .map_err(|error| ProviderError::network(error.to_string()))?;
        let read = stream
            .read(&mut read_buffer)
            .map_err(|error| read_error(error, "read", timeouts.read))?;
        if read == 0 {
            break;
        }
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
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];
    let mut header: Option<StreamingHeader> = None;
    let mut body = Vec::new();
    let mut chunk_buffer = Vec::new();
    let mut chunked_complete = false;

    loop {
        deadline.remaining("stream read")?;
        stream
            .set_read_timeout(Some(timeouts.read.min(deadline.remaining("stream read")?)))
            .map_err(|error| ProviderError::network(error.to_string()))?;
        let read = stream
            .read(&mut read_buffer)
            .map_err(|error| read_error(error, "stream read", timeouts.read))?;
        if read == 0 {
            break;
        }

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

fn request_timeout_error(phase: &str, timeout: Duration) -> ProviderError {
    ProviderError::network(format!(
        "provider request timed out during {phase} after {}ms",
        timeout.as_millis()
    ))
}
