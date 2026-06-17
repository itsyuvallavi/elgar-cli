//! Reads streaming HTTP responses and forwards decoded body chunks.
//!
//! The regular transport module owns connection setup. This module owns the
//! streaming read loop so SSE completion can stop reads without waiting for a
//! delayed socket close.

use std::{io::Read, net::TcpStream, time::Instant};

use crate::provider::{
    http::{
        response::{
            find_header_body_split, has_chunked_transfer_encoding, parse_status_code,
            process_streaming_body_bytes, StreamingHeader,
        },
        transport::{is_read_timeout, read_error, RequestDeadline, CANCEL_POLL_INTERVAL},
        types::{HttpResponse, HttpTimeouts, StreamingBodyAction},
    },
    types::ProviderError,
    ProviderCancelToken,
};

pub(super) fn read_streaming_http_response(
    mut stream: TcpStream,
    timeouts: HttpTimeouts,
    deadline: RequestDeadline,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<StreamingBodyAction, ProviderError>,
    cancel: &ProviderCancelToken,
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];
    let mut header: Option<StreamingHeader> = None;
    let mut body = Vec::new();
    let mut chunk_buffer = Vec::new();
    let mut chunked_complete = false;
    let mut callback_complete = false;
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
            if process_streaming_body_bytes(
                &tail,
                header.as_ref().unwrap(),
                &mut body,
                &mut chunk_buffer,
                &mut chunked_complete,
                on_body_chunk,
            )? == StreamingBodyAction::Stop
            {
                callback_complete = true;
                break;
            }
        } else if let Some(header) = header.as_ref() {
            if process_streaming_body_bytes(
                &read_buffer[..read],
                header,
                &mut body,
                &mut chunk_buffer,
                &mut chunked_complete,
                on_body_chunk,
            )? == StreamingBodyAction::Stop
            {
                callback_complete = true;
                break;
            }
        }
    }

    let header =
        header.ok_or_else(|| ProviderError::response_parse("HTTP response missing headers"))?;
    if header.is_chunked && !chunked_complete && !callback_complete {
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
