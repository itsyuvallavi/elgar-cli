use std::{
    io,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
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
        if self.host == "localhost" {
            return format!("127.0.0.1:{}", self.port)
                .parse::<SocketAddr>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"));
        }

        self.authority().parse::<SocketAddr>().map_err(|_| {
            ProviderError::configuration("provider URL host must be an IP address or localhost")
        })
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

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.path,
        endpoint.authority(),
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ProviderError::network(error.to_string()))?;

    read_http_response(stream)
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

pub(super) fn parse_http_response(raw: &str) -> Result<HttpResponse, ProviderError> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing header/body split"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing status line"))?;
    let status_code = parse_status_code(status_line)?;

    Ok(HttpResponse {
        status_code,
        body: body.to_string(),
    })
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
    fn localhost_maps_to_loopback_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://localhost:1234/v1/chat/completions").unwrap();

        assert_eq!(
            endpoint.socket_addr().unwrap().to_string(),
            "127.0.0.1:1234"
        );
    }

    #[test]
    fn non_local_hostnames_are_rejected_before_connect_timeout() {
        let endpoint = HttpEndpoint::parse("http://example.test:1234/v1/chat/completions").unwrap();
        let error = endpoint.socket_addr().unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::Configuration);
        assert!(error.message.contains("IP address or localhost"));
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
    fn malformed_http_response_maps_to_parse_error() {
        let error = parse_http_response("HTTP/1.1 nope\r\n\r\n{}").unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
        assert!(error.message.contains("status code"));
    }
}
