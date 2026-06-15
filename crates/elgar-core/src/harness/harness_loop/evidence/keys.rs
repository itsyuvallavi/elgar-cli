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
    mutation_epoch: usize,
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
        StructuredRequestKind::Write | StructuredRequestKind::Edit => {
            EvidenceKey::SideEffectVersion(
                request.kind.as_str().to_string(),
                normalize_evidence_path(request_path(request).unwrap_or_default()),
                stable_json_fingerprint(
                    request
                        .arguments
                        .as_ref()
                        .unwrap_or(&serde_json::Value::Null),
                ),
            )
        }
        StructuredRequestKind::Bash => EvidenceKey::SideEffectEpoch(
            "bash".to_string(),
            stable_json_fingerprint(
                request
                    .arguments
                    .as_ref()
                    .unwrap_or(&serde_json::Value::Null),
            ),
            mutation_epoch,
        ),
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

pub(in crate::harness::harness_loop) fn invalid_mcp_evidence_label(
    arguments: Option<&serde_json::Value>,
) -> String {
    invalid_mcp_evidence_key(arguments).as_label()
}

fn mcp_evidence_key_from_request(request: &ValidatedStructuredRequest) -> EvidenceKey {
    let arguments = request.arguments.as_ref();
    let Some(server_id) = arguments
        .and_then(|arguments| arguments.get("server"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return invalid_mcp_evidence_key(arguments);
    };
    let Some(tool_name) = arguments
        .and_then(|arguments| arguments.get("tool"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return invalid_mcp_evidence_key(arguments);
    };
    let tool_arguments = arguments
        .and_then(|arguments| arguments.get("arguments"))
        .unwrap_or(&serde_json::Value::Null);
    if !matches!(
        tool_arguments,
        serde_json::Value::Null | serde_json::Value::Object(_)
    ) {
        return invalid_mcp_evidence_key(arguments);
    }

    mcp_evidence_key(server_id, tool_name, tool_arguments)
}

fn invalid_mcp_evidence_key(arguments: Option<&serde_json::Value>) -> EvidenceKey {
    EvidenceKey::InvalidMcp(stable_json_fingerprint(
        arguments.unwrap_or(&serde_json::Value::Null),
    ))
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

    use super::{invalid_mcp_evidence_label, mcp_evidence_label, stable_json_fingerprint};

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

    #[test]
    fn invalid_mcp_label_uses_invalid_prefix() {
        let label = invalid_mcp_evidence_label(Some(
            &json!({"arguments": {"server": "context7", "tool": "query-docs"}}),
        ));

        assert!(label.starts_with("invalid_mcp_call:"));
        assert!(!label.contains("unknown"));
    }
}
