use std::path::{Component, Path, PathBuf};

use crate::session::Session;

pub(crate) fn common_path_prefix(left: &Path, right: &Path) -> Option<PathBuf> {
    let mut common = PathBuf::new();
    for (left, right) in left.components().zip(right.components()) {
        if left != right {
            break;
        }
        common.push(left.as_os_str());
    }
    (!common.as_os_str().is_empty()).then_some(common)
}

pub(crate) fn path_has_no_meaningful_parent(path: &Path) -> bool {
    path.parent()
        .is_none_or(|parent| parent.as_os_str().is_empty())
}

pub(crate) fn cwd_relative_path(session: &Session, path: &Path) -> PathBuf {
    path.strip_prefix(&session.cwd)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn absolute_session_path(session: &Session, path: &Path) -> PathBuf {
    normalize_path(if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(path) = project_root_relative_path_from_cwd_prefixed_relative(session, path)
    {
        path
    } else {
        session.cwd.join(path)
    })
}

pub(crate) fn project_root_relative_path_from_cwd_prefixed_relative(
    session: &Session,
    path: &Path,
) -> Option<PathBuf> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }

    let cwd_relative = session.cwd.strip_prefix(&session.project_root).ok()?;
    if cwd_relative.as_os_str().is_empty() || !path.starts_with(cwd_relative) {
        return None;
    }

    Some(session.project_root.join(path))
}

pub(crate) fn path_is_within(path: &Path, root: &Path) -> bool {
    normalize_path(path).starts_with(normalize_path(root))
}

pub(crate) fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
