//! Pre-approval target preview for risky filesystem tools.
//!
//! This module creates display metadata only. It does not approve, execute,
//! create directories, or perform final symlink validation.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::harness::{StructuredRequestKind, ValidatedStructuredRequest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalTargetPreview {
    pub requested_path: String,
    pub resolved_preview_path: String,
    pub is_absolute: bool,
    pub scope: ApprovalTargetScope,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalTargetScope {
    InsideLaunchFolder,
    OutsideLaunchFolder,
    Unknown,
}

impl ApprovalTargetScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InsideLaunchFolder => "inside_launch_folder",
            Self::OutsideLaunchFolder => "outside_launch_folder",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) fn preview_request_target(
    launch_cwd: &Path,
    request: &ValidatedStructuredRequest,
) -> Option<ApprovalTargetPreview> {
    if !matches!(
        request.kind,
        StructuredRequestKind::Write | StructuredRequestKind::Edit
    ) {
        return None;
    }

    let requested_path = request
        .arguments
        .as_ref()?
        .get("path")?
        .as_str()?
        .trim()
        .to_string();
    if requested_path.is_empty() {
        return None;
    }

    let root = lexical_normalize(launch_cwd);
    let requested = Path::new(&requested_path);
    let is_absolute = requested.is_absolute();
    let resolved = if is_absolute {
        lexical_normalize(requested)
    } else {
        lexical_normalize(root.join(requested))
    };
    let scope = if root.as_os_str().is_empty() {
        ApprovalTargetScope::Unknown
    } else if resolved.starts_with(&root) {
        ApprovalTargetScope::InsideLaunchFolder
    } else {
        ApprovalTargetScope::OutsideLaunchFolder
    };
    let warning = match scope {
        ApprovalTargetScope::OutsideLaunchFolder => {
            Some("Approving may modify files outside the launch folder.".to_string())
        }
        _ => None,
    };

    Some(ApprovalTargetPreview {
        requested_path,
        resolved_preview_path: resolved.display().to_string(),
        is_absolute,
        scope,
        warning,
    })
}

fn lexical_normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use crate::harness::{StructuredRequestKind, ValidatedStructuredRequest};

    use super::{preview_request_target, ApprovalTargetScope};

    #[test]
    fn relative_write_target_previews_inside_launch_folder() {
        let request = request(StructuredRequestKind::Write, "hello.txt");

        let preview = preview_request_target(Path::new("/project"), &request).unwrap();

        assert_eq!(preview.requested_path, "hello.txt");
        assert_eq!(preview.resolved_preview_path, "/project/hello.txt");
        assert!(!preview.is_absolute);
        assert_eq!(preview.scope, ApprovalTargetScope::InsideLaunchFolder);
        assert_eq!(preview.warning, None);
    }

    #[test]
    fn absolute_write_target_inside_launch_folder_is_marked_absolute() {
        let request = request(StructuredRequestKind::Write, "/project/hello.txt");

        let preview = preview_request_target(Path::new("/project"), &request).unwrap();

        assert!(preview.is_absolute);
        assert_eq!(preview.scope, ApprovalTargetScope::InsideLaunchFolder);
        assert_eq!(preview.warning, None);
    }

    #[test]
    fn absolute_write_target_outside_launch_folder_warns() {
        let request = request(StructuredRequestKind::Write, "/tmp/hello.txt");

        let preview = preview_request_target(Path::new("/project"), &request).unwrap();

        assert!(preview.is_absolute);
        assert_eq!(preview.scope, ApprovalTargetScope::OutsideLaunchFolder);
        assert!(preview.warning.is_some());
    }

    #[test]
    fn relative_edit_parent_escape_warns() {
        let request = request(StructuredRequestKind::Edit, "../outside.txt");

        let preview = preview_request_target(Path::new("/project/app"), &request).unwrap();

        assert!(!preview.is_absolute);
        assert_eq!(preview.resolved_preview_path, "/project/outside.txt");
        assert_eq!(preview.scope, ApprovalTargetScope::OutsideLaunchFolder);
        assert!(preview.warning.is_some());
    }

    #[test]
    fn read_request_has_no_target_preview() {
        let request = request(StructuredRequestKind::Read, "hello.txt");

        assert!(preview_request_target(Path::new("/project"), &request).is_none());
    }

    fn request(kind: StructuredRequestKind, path: &str) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind,
            reason: "test".to_string(),
            arguments: Some(json!({ "path": path })),
        }
    }
}
