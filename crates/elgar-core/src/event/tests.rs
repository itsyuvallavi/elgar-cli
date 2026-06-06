//! Tests for event serialization compatibility.

use super::ProviderOutput;

#[test]
fn provider_output_deserializes_old_content_only_shape() {
    let output: ProviderOutput = serde_json::from_str(r#"{"text":"provider response"}"#).unwrap();

    assert_eq!(output.text, "provider response");
    assert_eq!(output.thinking, None);
}
