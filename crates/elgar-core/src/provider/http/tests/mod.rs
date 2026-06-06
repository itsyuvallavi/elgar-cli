//! Tests for the local provider HTTP helper.
//!
//! These tests cover URL validation and response parsing without making live
//! network calls.

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
    let endpoint = HttpEndpoint::parse("http://[2001:db8::1]:1234/v1/chat/completions").unwrap();
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
