//! Conservative permission policy for harness primitive requests.
//!
//! Stage 3 exposes read-only primitives for execution. Risky primitives are
//! known to the model but cannot execute until an approval flow exists.

use crate::harness::{PrimitiveToolRegistry, ValidatedStructuredRequest};

use super::types::PermissionDecision;

/// Decide whether one validated primitive request may execute now.
pub fn decide_primitive_permission(
    registry: &PrimitiveToolRegistry,
    request: &ValidatedStructuredRequest,
) -> PermissionDecision {
    let Some(tool) = registry.get(request.kind) else {
        return PermissionDecision::deny(format!("unknown primitive `{}`", request.kind.as_str()));
    };

    if !tool.enabled_in_stage {
        return PermissionDecision::deny(format!(
            "primitive `{}` is disabled in this stage",
            tool.id.as_str()
        ));
    }

    if !tool.executable_in_stage && tool.requires_permission {
        return PermissionDecision::needs_approval(format!(
            "primitive `{}` requires approval and is not executable until the approval flow exists",
            tool.id.as_str()
        ));
    }

    if !tool.executable_in_stage {
        return PermissionDecision::deny(format!(
            "primitive `{}` is not executable in this stage",
            tool.id.as_str()
        ));
    }

    if tool.requires_permission {
        return PermissionDecision::needs_approval(format!(
            "primitive `{}` requires approval before execution",
            tool.id.as_str()
        ));
    }

    PermissionDecision::allow(format!(
        "primitive `{}` is read-only and executable in this stage",
        tool.id.as_str()
    ))
}
