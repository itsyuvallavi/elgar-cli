use std::{
    io,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    time::Duration,
};

use crate::provider::types::ProviderError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl HttpEndpoint {
    pub(super) fn parse(url: &str) -> Result<Self, ProviderError> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            ProviderError::configuration("only http:// provider URLs are supported")
        })?;
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            ProviderError::configuration("provider URL must include a request path")
        })?;
        let (host, port) = parse_authority(authority)?;
        Ok(Self {
            host,
            port,
            path: format!("/{path}"),
        })
    }

    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn socket_addr(&self) -> Result<SocketAddr, ProviderError> {
        if self.host.eq_ignore_ascii_case("localhost") {
            return format!("127.0.0.1:{}", self.port)
                .parse::<SocketAddr>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"));
        }

        let host = self
            .host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&self.host);
        let ip = host.parse::<IpAddr>().map_err(|_| {
            ProviderError::configuration("provider URL host must be localhost or a loopback IP")
        })?;
        if !ip.is_loopback() {
            return Err(ProviderError::configuration(
                "provider URL host must be localhost or a loopback IP",
            ));
        }

        Ok(SocketAddr::new(ip, self.port))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HttpStatusCode(u16);

impl HttpStatusCode {
    pub(super) fn as_u16(self) -> u16 {
        self.0
    }

    pub(super) fn is_success(self) -> bool {
        (200..300).contains(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HttpResponse {
    pub status_code: HttpStatusCode,
    pub body: String,
}

fn parse_authority(authority: &str) -> Result<(String, u16), ProviderError> {
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => {
            let parsed_port = port
                .parse::<u16>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"))?;
            (host, parsed_port)
        }
        None => (authority, 80),
    };

    if host.trim().is_empty() {
        return Err(ProviderError::configuration(
            "provider URL host must not be empty",
        ));
    }

    Ok((host.to_string(), port))
}

pub(super) fn post_json(
    endpoint: &HttpEndpoint,
    body: &str,
    timeout: Duration,
) -> Result<HttpResponse, ProviderError> {
    let mut stream = connect_with_timeout(endpoint, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    write_json_request(&mut stream, endpoint, body, "application/json")?;

    read_http_response(stream)
}

pub(super) fn post_json_streaming(
    endpoint: &HttpEndpoint,
    body: &str,
    timeout: Duration,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
) -> Result<HttpResponse, ProviderError> {
    let mut stream = connect_with_timeout(endpoint, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    write_json_request(
        &mut stream,
        endpoint,
        body,
        "text/event-stream, application/json",
    )?;

    read_streaming_http_response(stream, on_body_chunk)
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
    TcpStream::connect_timeout(&endpoint.socket_addr()?, timeout)
        .map_err(|error| ProviderError::network(error.to_string()))
}

fn read_http_response(mut stream: TcpStream) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|error| match error.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                ProviderError::network("provider request timed out")
            }
            _ => ProviderError::network(error.to_string()),
        })?;
    let raw = String::from_utf8(bytes)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    parse_http_response(&raw)
}

fn read_streaming_http_response(
    mut stream: TcpStream,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    let mut read_buffer = [0_u8; 4096];
    let mut header: Option<StreamingHeader> = None;
    let mut body = Vec::new();
    let mut chunk_buffer = Vec::new();
    let mut chunked_complete = false;

    loop {
        let read = stream
            .read(&mut read_buffer)
            .map_err(|error| match error.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    ProviderError::network("provider request timed out")
                }
                _ => ProviderError::network(error.to_string()),
            })?;
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

#[derive(Debug, Clone, Copy)]
struct StreamingHeader {
    status_code: HttpStatusCode,
    is_chunked: bool,
}

fn process_streaming_body_bytes(
    bytes: &[u8],
    header: &StreamingHeader,
    body: &mut Vec<u8>,
    chunk_buffer: &mut Vec<u8>,
    chunked_complete: &mut bool,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    if bytes.is_empty() {
        return Ok(());
    }

    if header.is_chunked {
        if *chunked_complete {
            return Ok(());
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
            on_body_chunk(&text)?;
        }
        Ok(())
    }
}

fn drain_complete_chunked_chunks(
    chunk_buffer: &mut Vec<u8>,
    body: &mut Vec<u8>,
    emit_chunks: bool,
    chunked_complete: &mut bool,
    on_body_chunk: &mut dyn FnMut(&str) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    loop {
        let Some(size_end) = find_crlf(chunk_buffer, 0) else {
            return Ok(());
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
            return Ok(());
        }

        if size == 0 {
            chunk_buffer.drain(..data_end + 2);
            *chunked_complete = true;
            return Ok(());
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
            on_body_chunk(&text)?;
        }
        chunk_buffer.drain(..data_end + 2);
    }
}

pub(super) fn parse_http_response(raw: &str) -> Result<HttpResponse, ProviderError> {
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

fn find_header_body_split(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_status_code(status_line: &str) -> Result<HttpStatusCode, ProviderError> {
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

fn has_chunked_transfer_encoding(head: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{parse_http_response, HttpEndpoint};
    use crate::provider::ProviderErrorKind;

    #[test]
    fn live_chat_endpoint_parses_http_local_urls_only() {
        let endpoint = HttpEndpoint::parse("http://127.0.0.1:1234/v1/chat/completions").unwrap();

        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 1234);
        assert_eq!(endpoint.path, "/v1/chat/completions");
        assert_eq!(endpoint.authority(), "127.0.0.1:1234");

        let https = HttpEndpoint::parse("https://127.0.0.1:1234/v1/chat/completions").unwrap_err();
        assert_eq!(https.kind, ProviderErrorKind::Configuration);
        assert!(https.message.contains("http://"));
    }

    #[test]
    fn endpoint_socket_addr_is_available_for_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://127.0.0.1:1234/v1/chat/completions").unwrap();

        assert_eq!(
            endpoint.socket_addr().unwrap().to_string(),
            "127.0.0.1:1234"
        );
    }

    #[test]
    fn loopback_numeric_ips_are_allowed_before_connect() {
        let endpoint = HttpEndpoint::parse("http://127.42.0.9:1234/v1/chat/completions").unwrap();

        assert_eq!(
            endpoint.socket_addr().unwrap().to_string(),
            "127.42.0.9:1234"
        );
    }

    #[test]
    fn bracketed_ipv6_loopback_is_allowed_before_connect() {
        let endpoint = HttpEndpoint::parse("http://[::1]:1234/v1/chat/completions").unwrap();

        assert_eq!(endpoint.authority(), "[::1]:1234");
        assert_eq!(endpoint.socket_addr().unwrap().to_string(), "[::1]:1234");
    }

    #[test]
    fn localhost_maps_to_loopback_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://localhost:1234/v1/chat/completions").unwrap();

        assert_eq!(
            endpoint.socket_addr().unwrap().to_string(),
            "127.0.0.1:1234"
        );
    }

    #[test]
    fn localhost_matching_is_case_insensitive_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://LOCALHOST:1234/v1/chat/completions").unwrap();

        assert_eq!(
            endpoint.socket_addr().unwrap().to_string(),
            "127.0.0.1:1234"
        );
    }

    #[test]
    fn non_loopback_numeric_ips_are_rejected_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://192.168.1.10:1234/v1/chat/completions").unwrap();
        let error = endpoint.socket_addr().unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::Configuration);
        assert!(error.message.contains("loopback IP"));
    }

    #[test]
    fn bracketed_non_loopback_ipv6_is_rejected_before_connect_timeout() {
        let endpoint =
            HttpEndpoint::parse("http://[2001:db8::1]:1234/v1/chat/completions").unwrap();
        let error = endpoint.socket_addr().unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::Configuration);
        assert!(error.message.contains("loopback IP"));
    }

    #[test]
    fn non_local_hostnames_are_rejected_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://example.test:1234/v1/chat/completions").unwrap();
        let error = endpoint.socket_addr().unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::Configuration);
        assert!(error.message.contains("localhost or a loopback IP"));
    }

    #[test]
    fn http_success_response_parses_as_provider_output() {
        let response = parse_http_response(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
            {\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hello\"}}]}",
        )
        .unwrap();

        assert!(response.status_code.is_success());
        assert_eq!(
            response.body,
            "{\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hello\"}}]}"
        );
    }

    #[test]
    fn http_chunked_response_body_is_decoded() {
        let response = parse_http_response(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\ndata: a\r\n7\r\ndata: b\r\n0\r\n\r\n",
        )
        .unwrap();

        assert!(response.status_code.is_success());
        assert_eq!(response.body, "data: adata: b");
    }

    #[test]
    fn malformed_http_response_maps_to_parse_error() {
        let error = parse_http_response("HTTP/1.1 nope\r\n\r\n{}").unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
        assert!(error.message.contains("status code"));
    }
}
