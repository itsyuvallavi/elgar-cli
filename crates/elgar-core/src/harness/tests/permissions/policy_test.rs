//! Tests for conservative harness permission decisions.

use serde_json::json;

use crate::harness::{
    decide_primitive_permission, PermissionDecisionKind, PermissionMode, PrimitiveToolRegistry,
    StructuredRequestKind, ValidatedStructuredRequest,
};

fn request(kind: StructuredRequestKind) -> ValidatedStructuredRequest {
    ValidatedStructuredRequest {
        kind,
        reason: "test".to_string(),
        arguments: Some(json!({})),
    }
}

#[test]
fn permission_policy_allows_read_only_primitives() {
    let registry = PrimitiveToolRegistry::stage_3a();

    for kind in [
        StructuredRequestKind::Read,
        StructuredRequestKind::Ls,
        StructuredRequestKind::Find,
        StructuredRequestKind::Grep,
    ] {
        let decision =
            decide_primitive_permission(&registry, &request(kind), PermissionMode::ReviewAll);

        assert_eq!(decision.kind, PermissionDecisionKind::Allow);
        assert!(decision.allows_execution());
    }
}

#[test]
fn permission_policy_blocks_risky_primitives_until_approval_exists() {
    let registry = PrimitiveToolRegistry::stage_3a();

    for kind in [
        StructuredRequestKind::Bash,
        StructuredRequestKind::Write,
        StructuredRequestKind::Edit,
    ] {
        let decision =
            decide_primitive_permission(&registry, &request(kind), PermissionMode::ReviewAll);

        assert_eq!(decision.kind, PermissionDecisionKind::NeedsApproval);
        assert!(!decision.allows_execution());
        assert!(decision.reason.contains("requires approval"));
    }
}

#[test]
fn permission_policy_allows_safe_relative_write_in_workspace_write_mode() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let request = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "test".to_string(),
        arguments: Some(json!({"path":"app/page.tsx","content":"export default null"})),
    };

    let decision = decide_primitive_permission(&registry, &request, PermissionMode::WorkspaceWrite);

    assert_eq!(decision.kind, PermissionDecisionKind::Allow);
    assert!(decision.reason.contains("workspace_write"));
}

#[test]
fn permission_policy_keeps_unsafe_write_approval_in_workspace_write_mode() {
    let registry = PrimitiveToolRegistry::stage_3a();

    for path in ["/tmp/outside.txt", "../outside.txt"] {
        let request = ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "test".to_string(),
            arguments: Some(json!({"path":path,"content":"demo"})),
        };

        let decision =
            decide_primitive_permission(&registry, &request, PermissionMode::WorkspaceWrite);

        assert_eq!(decision.kind, PermissionDecisionKind::NeedsApproval);
    }
}

#[test]
fn permission_policy_allows_trusted_full_access_risky_primitives() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let write = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "test".to_string(),
        arguments: Some(json!({"path":"app/page.tsx","content":"demo"})),
    };
    let edit = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Edit,
        reason: "test".to_string(),
        arguments: Some(json!({"path":"app/page.tsx","old_text":"old","new_text":"new"})),
    };
    let bash = ValidatedStructuredRequest {
        kind: StructuredRequestKind::Bash,
        reason: "test".to_string(),
        arguments: Some(json!({"command":"pwd"})),
    };

    for request in [write, edit, bash] {
        let decision = decide_primitive_permission(&registry, &request, PermissionMode::FullAccess);

        assert_eq!(decision.kind, PermissionDecisionKind::Allow);
        assert!(decision.allows_execution());
        assert!(decision.reason.contains("full_access"));
    }
}

#[test]
fn permission_policy_keeps_unsafe_write_and_edit_approval_in_full_access() {
    let registry = PrimitiveToolRegistry::stage_3a();

    for kind in [StructuredRequestKind::Write, StructuredRequestKind::Edit] {
        let request = ValidatedStructuredRequest {
            kind,
            reason: "test".to_string(),
            arguments: Some(
                json!({"path":"../outside.txt","content":"demo","old_text":"old","new_text":"new"}),
            ),
        };

        let decision = decide_primitive_permission(&registry, &request, PermissionMode::FullAccess);

        assert_eq!(decision.kind, PermissionDecisionKind::NeedsApproval);
    }
}
