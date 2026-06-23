//! Conservative permission policy for harness primitive requests.
//!
//! Read-only primitives execute directly. Risky primitives usually pass through
//! the approval command path, except for explicit bounded permission modes.

use std::path::{Component, Path};

use crate::harness::{PrimitiveToolRegistry, StructuredRequestKind, ValidatedStructuredRequest};

use super::types::{PermissionDecision, PermissionMode};

/// Decide whether one validated primitive request may execute now.
pub fn decide_primitive_permission(
    registry: &PrimitiveToolRegistry,
    request: &ValidatedStructuredRequest,
    mode: PermissionMode,
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

    if mode == PermissionMode::WorkspaceWrite
        && request.kind == StructuredRequestKind::Write
        && request_has_safe_relative_write_target(request)
    {
        return PermissionDecision::allow(
            "primitive `write` is allowed by permission mode `workspace_write` for a safe relative target",
        );
    }

    if mode == PermissionMode::FullAccess {
        match request.kind {
            StructuredRequestKind::Write if request_has_safe_relative_write_target(request) => {
                return PermissionDecision::allow(
                    "primitive `write` is allowed by permission mode `full_access` for a safe relative target",
                );
            }
            StructuredRequestKind::Edit if request_has_safe_relative_edit_target(request) => {
                return PermissionDecision::allow(
                    "primitive `edit` is allowed by permission mode `full_access` for a safe relative target",
                );
            }
            StructuredRequestKind::Bash => {
                return PermissionDecision::allow(
                    "primitive `bash` is allowed by trusted permission mode `full_access` in the launch folder",
                );
            }
            _ => {}
        }
    }

    if !tool.executable_in_stage && tool.requires_permission {
        return PermissionDecision::needs_approval(format!(
            "primitive `{}` requires approval before side-effect execution",
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

fn request_has_safe_relative_write_target(request: &ValidatedStructuredRequest) -> bool {
    let Some(arguments) = request.arguments.as_ref() else {
        return false;
    };
    let Some(path) = arguments.get("path").and_then(|value| value.as_str()) else {
        return false;
    };
    if arguments
        .get("content")
        .and_then(|value| value.as_str())
        .is_none()
    {
        return false;
    }

    let path = Path::new(path.trim());
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

fn request_has_safe_relative_edit_target(request: &ValidatedStructuredRequest) -> bool {
    let Some(arguments) = request.arguments.as_ref() else {
        return false;
    };
    let Some(path) = arguments.get("path").and_then(|value| value.as_str()) else {
        return false;
    };
    if arguments
        .get("old_text")
        .and_then(|value| value.as_str())
        .is_none()
    {
        return false;
    }
    if arguments
        .get("new_text")
        .and_then(|value| value.as_str())
        .is_none()
    {
        return false;
    }

    is_safe_relative_path(path)
}

fn is_safe_relative_path(path: &str) -> bool {
    let path = Path::new(path.trim());
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}
