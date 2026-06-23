//! Unit coverage for direct primitive target fidelity.

use serde_json::json;

use super::*;

#[test]
fn accepts_matching_read() {
    let request = request(
        StructuredRequestKind::Read,
        json!({"path":"./package.json"}),
    );
    assert!(validate_tool_target("read package.json", &request).is_none());
}

#[test]
fn rejects_wrong_read_target() {
    let request = request(StructuredRequestKind::Read, json!({"path":"app/page.tsx"}));
    assert!(validate_tool_target("read postcss.config.mjs", &request).is_some());
}

#[test]
fn accepts_contextual_basename_read_target() {
    let request = request(
        StructuredRequestKind::Read,
        json!({"path":"Nextjs-1/package.json"}),
    );
    assert!(validate_tool_target("show me package.json", &request).is_none());
}

#[test]
fn rejects_generated_basename_read_target() {
    let request = request(
        StructuredRequestKind::Read,
        json!({"path":"node_modules/example/package.json"}),
    );
    assert!(validate_tool_target("show me package.json", &request).is_some());
}

#[test]
fn accepts_user_language_folder_listing() {
    let request = request(StructuredRequestKind::Ls, json!({"path":"Nextjs-1/app"}));
    assert!(validate_tool_target("show me the app folder", &request).is_none());
}

#[test]
fn rejects_wrong_grep_target() {
    let request = request(
        StructuredRequestKind::Grep,
        json!({"path":".","query":"tailwind"}),
    );
    assert!(validate_tool_target("grep tailwind in tailwind.config.ts", &request).is_some());
}

#[test]
fn rejects_wrong_search_target() {
    let request = request(
        StructuredRequestKind::Find,
        json!({"path":".","pattern":"*config*"}),
    );
    assert!(validate_tool_target("search for tailwind in tailwind.config.ts", &request).is_some());
}

#[test]
fn accepts_matching_search_target() {
    let request = request(
        StructuredRequestKind::Grep,
        json!({"path":"tailwind.config.ts","query":"tailwind"}),
    );
    assert!(validate_tool_target("search for tailwind in tailwind.config.ts", &request).is_none());
}

#[test]
fn accepts_search_inside_target() {
    let request = request(
        StructuredRequestKind::Grep,
        json!({"path":"tailwind.config.ts","query":"tailwind"}),
    );
    assert!(
        validate_tool_target("search inside tailwind.config.ts for tailwind", &request).is_none()
    );
}

#[test]
fn accepts_user_language_file_read() {
    let request = request(
        StructuredRequestKind::Read,
        json!({"path":"postcss.config.mjs"}),
    );
    assert!(validate_tool_target("show me postcss.config.mjs", &request).is_none());
}

fn request(
    kind: StructuredRequestKind,
    arguments: serde_json::Value,
) -> ValidatedStructuredRequest {
    ValidatedStructuredRequest {
        kind,
        reason: "test".to_string(),
        arguments: Some(arguments),
    }
}
