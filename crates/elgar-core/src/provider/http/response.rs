//! Parses HTTP responses and chunked bodies.
//!
//! Transport code gives this module raw HTTP bytes or body chunks. This module
//! owns status parsing, header detection, and chunked-transfer decoding.

use crate::provider::{
    http::types::{HttpResponse, HttpStatusCode, StreamingBodyAction},
    types::ProviderError,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct StreamingHeader {
    pub(super) status_code: HttpStatusCode,
    pub(super) is_chunked: bool,
}

pub(in crate::provider) fn parse_http_response(raw: &str) -> Result<HttpResponse, ProviderError> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing header/body split"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing status line"))?;
    let status_code = parse_status_code(status_line)?;
    let body = if has_chunked_transfer_encoding(head) {
        decode_chunked_body(body)?
    } else {
        body.to_string()
    };

    Ok(HttpResponse { status_code, body })
}

pub(super) fn find_header_body_split(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(super) fn parse_status_code(status_line: &str) -> Result<HttpStatusCode, ProviderError> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing version"))?;
    let status = parts
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing status code"))?;
    let code = status
        .parse::<u16>()
        .map_err(|_| ProviderError::response_parse("HTTP response status code is invalid"))?;
    Ok(HttpStatusCode(code))
}

pub(super) fn has_chunked_transfer_encoding(head: &str) -> bool {
    head.lines().skip(1).any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("chunked"))
    })
}

pub(super) fn process_streaming_body_bytes(
    bytes: &[u8],
    header: &StreamingHeader,
    body: &mut Vec<u8>,
    chunk_buffer: &mut Vec<u8>,
    chunked_complete: &mut bool,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<StreamingBodyAction, ProviderError>,
) -> Result<StreamingBodyAction, ProviderError> {
    if bytes.is_empty() {
        return Ok(StreamingBodyAction::Continue);
    }

    if header.is_chunked {
        if *chunked_complete {
            return Ok(StreamingBodyAction::Continue);
        }
        chunk_buffer.extend_from_slice(bytes);
        drain_complete_chunked_chunks(
            chunk_buffer,
            body,
            header.status_code.is_success(),
            chunked_complete,
            on_body_chunk,
        )
    } else {
        body.extend_from_slice(bytes);
        if header.status_code.is_success() {
            let text = String::from_utf8_lossy(bytes);
            return on_body_chunk(&text);
        }
        Ok(StreamingBodyAction::Continue)
    }
}

fn drain_complete_chunked_chunks(
    chunk_buffer: &mut Vec<u8>,
    body: &mut Vec<u8>,
    emit_chunks: bool,
    chunked_complete: &mut bool,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<StreamingBodyAction, ProviderError>,
) -> Result<StreamingBodyAction, ProviderError> {
    loop {
        let Some(size_end) = find_crlf(chunk_buffer, 0) else {
            return Ok(StreamingBodyAction::Continue);
        };
        let size_line = std::str::from_utf8(&chunk_buffer[..size_end])
            .map_err(|error| ProviderError::response_parse(error.to_string()))?;
        let size_hex = size_line
            .split_once(';')
            .map(|(size, _extension)| size)
            .unwrap_or(size_line)
            .trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| ProviderError::response_parse("chunked body chunk size is invalid"))?;
        let data_start = size_end + 2;
        let data_end = data_start + size;
        if chunk_buffer.len() < data_end + 2 {
            return Ok(StreamingBodyAction::Continue);
        }

        if size == 0 {
            chunk_buffer.drain(..data_end + 2);
            *chunked_complete = true;
            return Ok(StreamingBodyAction::Continue);
        }

        if &chunk_buffer[data_end..data_end + 2] != b"\r\n" {
            return Err(ProviderError::response_parse(
                "chunked body missing chunk terminator",
            ));
        }

        let data = chunk_buffer[data_start..data_end].to_vec();
        body.extend_from_slice(&data);
        if emit_chunks {
            let text = String::from_utf8_lossy(&data);
            if on_body_chunk(&text)? == StreamingBodyAction::Stop {
                return Ok(StreamingBodyAction::Stop);
            }
        }
        chunk_buffer.drain(..data_end + 2);
    }
}

fn decode_chunked_body(body: &str) -> Result<String, ProviderError> {
    let bytes = body.as_bytes();
    let mut offset = 0;
    let mut decoded = Vec::new();

    loop {
        let size_end = find_crlf(bytes, offset)
            .ok_or_else(|| ProviderError::response_parse("chunked body missing chunk size"))?;
        let size_line = std::str::from_utf8(&bytes[offset..size_end])
            .map_err(|error| ProviderError::response_parse(error.to_string()))?;
        let size_hex = size_line
            .split_once(';')
            .map(|(size, _extension)| size)
            .unwrap_or(size_line)
            .trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| ProviderError::response_parse("chunked body chunk size is invalid"))?;
        offset = size_end + 2;

        if size == 0 {
            return String::from_utf8(decoded)
                .map_err(|error| ProviderError::response_parse(error.to_string()));
        }

        let data_end = offset + size;
        if bytes.len() < data_end + 2 {
            return Err(ProviderError::response_parse(
                "chunked body ended before chunk data",
            ));
        }

        decoded.extend_from_slice(&bytes[offset..data_end]);
        if &bytes[data_end..data_end + 2] != b"\r\n" {
            return Err(ProviderError::response_parse(
                "chunked body missing chunk terminator",
            ));
        }
        offset = data_end + 2;
    }
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|position| start + position)
}
