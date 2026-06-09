//! Tests for conservative harness permission decisions.

use serde_json::json;

use crate::harness::{
    decide_primitive_permission, PermissionDecisionKind, PrimitiveToolRegistry,
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
        let decision = decide_primitive_permission(&registry, &request(kind));

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
        let decision = decide_primitive_permission(&registry, &request(kind));

        assert_eq!(decision.kind, PermissionDecisionKind::NeedsApproval);
        assert!(!decision.allows_execution());
        assert!(decision.reason.contains("requires approval"));
    }
}
