//! Stable evidence keys for primitive harness requests.

use std::path::{Component, Path};

use crate::{
    harness::harness_loop::{
        evidence::request_args::{request_path, request_pattern, request_query},
        state::types::EvidenceKey,
    },
    harness::{StructuredRequestKind, ValidatedStructuredRequest},
};

/// Build the stable budget key for one validated request.
pub(in crate::harness::harness_loop) fn evidence_key_for_request(
    request: &ValidatedStructuredRequest,
) -> EvidenceKey {
    match request.kind {
        StructuredRequestKind::Read => EvidenceKey::Read(normalize_evidence_path(
            request_path(request).unwrap_or_default(),
        )),
        StructuredRequestKind::Ls => EvidenceKey::Ls(normalize_evidence_path(
            request_path(request).unwrap_or("."),
        )),
        StructuredRequestKind::Find => EvidenceKey::Find(
            normalize_evidence_path(request_path(request).unwrap_or(".")),
            request_pattern(request).unwrap_or_default().to_string(),
        ),
        StructuredRequestKind::Grep => EvidenceKey::Grep(
            normalize_evidence_path(request_path(request).unwrap_or(".")),
            request_query(request).unwrap_or_default().to_string(),
        ),
        StructuredRequestKind::Bash
        | StructuredRequestKind::Write
        | StructuredRequestKind::Edit => EvidenceKey::Primitive(request.kind.as_str().to_string()),
        StructuredRequestKind::McpCall => mcp_evidence_key_from_request(request),
    }
}

pub(in crate::harness::harness_loop) fn mcp_evidence_label(
    server_id: &str,
    tool_name: &str,
    tool_arguments: &serde_json::Value,
) -> String {
    mcp_evidence_key(server_id, tool_name, tool_arguments).as_label()
}

fn mcp_evidence_key_from_request(request: &ValidatedStructuredRequest) -> EvidenceKey {
    let arguments = request.arguments.as_ref();
    let server_id = arguments
        .and_then(|arguments| arguments.get("server"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let tool_name = arguments
        .and_then(|arguments| arguments.get("tool"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let tool_arguments = arguments
        .and_then(|arguments| arguments.get("arguments"))
        .unwrap_or(&serde_json::Value::Null);

    mcp_evidence_key(server_id, tool_name, tool_arguments)
}

fn mcp_evidence_key(
    server_id: &str,
    tool_name: &str,
    tool_arguments: &serde_json::Value,
) -> EvidenceKey {
    EvidenceKey::Mcp(
        server_id.trim().to_string(),
        tool_name.trim().to_string(),
        stable_json_fingerprint(tool_arguments),
    )
}

fn stable_json_fingerprint(value: &serde_json::Value) -> String {
    let canonical = canonical_json(value);
    format!("{:016x}", fnv1a64(canonical.as_bytes()))
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
        }
        serde_json::Value::Array(items) => {
            let rendered = items.iter().map(canonical_json).collect::<Vec<_>>();
            format!("[{}]", rendered.join(","))
        }
        serde_json::Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let rendered = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    format!("{key}:{}", canonical_json(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", rendered.join(","))
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(in crate::harness::harness_loop) fn normalize_evidence_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ".".to_string();
    }

    let source = Path::new(trimmed);
    let mut parts = Vec::new();
    let mut absolute = false;
    for component in source.components() {
        match component {
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
            Component::RootDir => {
                absolute = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last().map(String::as_str), Some("..")) || parts.is_empty() {
                    if !absolute {
                        parts.push("..".to_string());
                    }
                } else {
                    parts.pop();
                }
            }
            Component::Normal(value) => {
                parts.push(value.to_string_lossy().into_owned());
            }
        }
    }

    let normalized = parts.join("/");
    if absolute {
        if normalized.is_empty() {
            "/".to_string()
        } else {
            format!("/{normalized}")
        }
    } else if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{mcp_evidence_label, stable_json_fingerprint};

    #[test]
    fn mcp_fingerprint_is_stable_across_object_field_order() {
        let left = json!({"libraryId": "/vercel/next.js", "query": "middleware auth"});
        let right = json!({"query": "middleware auth", "libraryId": "/vercel/next.js"});

        assert_eq!(
            stable_json_fingerprint(&left),
            stable_json_fingerprint(&right)
        );
    }

    #[test]
    fn mcp_labels_include_argument_fingerprint() {
        let first = mcp_evidence_label(
            "context7",
            "query-docs",
            &json!({"libraryId": "/vercel/next.js", "query": "middleware auth"}),
        );
        let second = mcp_evidence_label(
            "context7",
            "query-docs",
            &json!({"libraryId": "/vercel/next.js", "query": "middleware.ts"}),
        );

        assert_ne!(first, second);
        assert!(first.starts_with("mcp:context7:query-docs:"));
    }
}
