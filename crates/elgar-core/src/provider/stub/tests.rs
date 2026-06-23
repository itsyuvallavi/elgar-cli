//! Tests for the no-network provider stub.
//!
//! These tests verify deterministic text behavior without depending on LM
//! Studio or archived tool-call fields.

use super::ProviderStub;
use crate::provider::{ChatMessage, ControllerProvider, ProviderRequestMetadata};

#[test]
fn stub_tool_request_returns_no_network_text() {
    let output = ProviderStub::default()
        .chat_with_tools_with_metadata(
            "create a folder called demo",
            &ProviderRequestMetadata::new("stub-provider", None, "request-1"),
            Vec::new(),
        )
        .unwrap();

    assert!(output.text.contains("No live provider call was made."));
    assert!(output.text.contains("create a folder called demo"));
}

#[test]
fn stub_plain_request_returns_no_network_text() {
    let output = ProviderStub::default().ask("hello").output;

    assert!(output.text.contains("hello"));
    assert!(output.text.contains("No live provider call was made."));
}

#[test]
fn stub_route_request_returns_json_chat_response() {
    let output = ProviderStub::default()
        .chat_messages_with_metadata(
            vec![
                ChatMessage::system("Return one compact JSON object using the routing schema."),
                ChatMessage::user("hello"),
            ],
            &ProviderRequestMetadata::new("stub-provider", None, "request-1"),
        )
        .unwrap();

    assert!(output.text.contains("\"route\":\"chat\""));
    assert!(output.text.contains("No live provider call was made."));
}
