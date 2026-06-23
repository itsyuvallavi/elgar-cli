use std::path::{Path, PathBuf};

use crate::{
    action::{Action, ActionRequest, ShellCommandAction},
    model_runtime::ValidatedModelToolAction,
    session::Session,
};

#[derive(Debug, Clone)]
pub(crate) struct AgentPathResolution;

impl AgentPathResolution {
    pub(crate) fn new(
        _requested_project_base: Option<PathBuf>,
        _followup_base: Option<PathBuf>,
        _workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self
    }
}

pub(crate) fn resolve_agent_action_paths(
    mut action: ValidatedModelToolAction,
    _resolution: &AgentPathResolution,
) -> ValidatedModelToolAction {
    expand_home_paths(&mut action);
    action.target_label = action.request.approval_target();
    action
}

pub(crate) fn allowed_root_for_action(session: &Session, action: &Action) -> PathBuf {
    let Some(target_path) = action_filesystem_target(action) else {
        return session.cwd.clone();
    };

    if target_path.is_absolute() {
        if let Some(home) = home_dir() {
            if target_path.starts_with(&home) {
                return home;
            }
        }
        return target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| session.cwd.clone());
    }

    session.cwd.clone()
}

pub(crate) fn resolve_shell_action_paths_for_session(session: &Session, action: &Action) -> Action {
    let ActionRequest::ShellCommand(shell) = &action.request else {
        return action.clone();
    };

    let mut resolved = action.clone();
    resolved.request = ActionRequest::ShellCommand(resolve_shell_command_paths(session, shell));
    resolved
}

pub(crate) fn resolve_shell_command_paths(
    session: &Session,
    action: &ShellCommandAction,
) -> ShellCommandAction {
    let mut resolved = action.clone();
    expand_home_path(&mut resolved.cwd);
    resolved.cwd = session_path(session, &resolved.cwd);
    let cwd = resolved.cwd.clone();

    resolved.expected_directory = resolved
        .expected_directory
        .map(|path| shell_expected_path(&cwd, path));
    resolved.expected_directories = resolved
        .expected_directories
        .into_iter()
        .map(|path| shell_expected_path(&cwd, path))
        .collect();
    resolved.expected_file = resolved
        .expected_file
        .map(|path| shell_expected_path(&cwd, path));
    resolved.expected_files = resolved
        .expected_files
        .into_iter()
        .map(|path| shell_expected_path(&cwd, path))
        .collect();

    resolved
}

fn expand_home_paths(validated: &mut ValidatedModelToolAction) {
    match &mut validated.request {
        ActionRequest::CreateFile(create_file) => expand_home_path(&mut create_file.target_path),
        ActionRequest::CreateDirectory(create_directory) => {
            expand_home_path(&mut create_directory.target_path)
        }
        ActionRequest::PatchFile(patch_file) => expand_home_path(&mut patch_file.target_path),
        ActionRequest::OverwriteFile(overwrite_file) => {
            expand_home_path(&mut overwrite_file.target_path)
        }
        ActionRequest::DeleteFile(delete_file) => expand_home_path(&mut delete_file.target_path),
        ActionRequest::MoveFile(move_file) => {
            expand_home_path(&mut move_file.source_path);
            expand_home_path(&mut move_file.target_path);
        }
        ActionRequest::ShellCommand(shell) => expand_home_path(&mut shell.cwd),
    }
}

fn session_path(session: &Session, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        relative_session_path(session, path)
    }
}

fn relative_session_path(session: &Session, path: &Path) -> PathBuf {
    let path = normalize_relative_path(path);
    if path.as_os_str().is_empty() {
        return session.cwd.clone();
    }

    if path.file_name() == session.cwd.file_name() && path.components().count() == 1 {
        return session.cwd.clone();
    }

    if let Ok(cwd_relative) = session.cwd.strip_prefix(&session.project_root) {
        if let Some(resolved) = path_under_current_cwd(session, cwd_relative, &path) {
            return resolved;
        }

        if let Some(project_name) = session.project_root.file_name() {
            let project_prefixed_cwd = PathBuf::from(project_name).join(cwd_relative);
            if let Some(resolved) = path_under_current_cwd(session, &project_prefixed_cwd, &path) {
                return resolved;
            }
        }
    }

    if let Some(project_name) = session.project_root.file_name() {
        if let Ok(project_relative) = path.strip_prefix(project_name) {
            return session.project_root.join(project_relative);
        }
    }

    session.cwd.join(path)
}

fn path_under_current_cwd(session: &Session, cwd_prefix: &Path, path: &Path) -> Option<PathBuf> {
    if cwd_prefix.as_os_str().is_empty() {
        return None;
    }

    let suffix = path.strip_prefix(cwd_prefix).ok()?;
    if suffix.as_os_str().is_empty() {
        Some(session.cwd.clone())
    } else {
        Some(session.cwd.join(suffix))
    }
}

fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::ParentDir => normalized.push(".."),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str())
            }
        }
    }
    normalized
}

fn shell_expected_path(cwd: &Path, mut path: PathBuf) -> PathBuf {
    expand_home_path(&mut path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn expand_home_path(path: &mut PathBuf) {
    let Some(home) = home_dir() else {
        return;
    };
    if let Some(suffix) = strip_home_relative_prefix(path) {
        *path = if suffix.as_os_str().is_empty() {
            home
        } else {
            home.join(suffix)
        };
    }
}

fn action_filesystem_target(action: &Action) -> Option<&Path> {
    match &action.request {
        ActionRequest::CreateFile(create_file) => Some(&create_file.target_path),
        ActionRequest::CreateDirectory(create_directory) => Some(&create_directory.target_path),
        ActionRequest::PatchFile(patch_file) => Some(&patch_file.target_path),
        ActionRequest::OverwriteFile(overwrite_file) => Some(&overwrite_file.target_path),
        ActionRequest::DeleteFile(delete_file) => Some(&delete_file.target_path),
        ActionRequest::MoveFile(move_file) => Some(&move_file.target_path),
        ActionRequest::ShellCommand(_) => None,
    }
}

fn strip_home_relative_prefix(target_path: &Path) -> Option<PathBuf> {
    let target = target_path.as_os_str().to_string_lossy();
    if target == "~" || target.eq_ignore_ascii_case("$home") {
        return Some(PathBuf::new());
    }
    for prefix in ["~/", "$HOME/", "$home/"] {
        if let Some(rest) = target.strip_prefix(prefix) {
            return Some(PathBuf::from(rest));
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::{Action, ActionRequest, CreateDirectoryAction, ShellCommandAction},
        model_runtime::ValidatedModelToolAction,
        session::Session,
    };

    fn validated(request: ActionRequest) -> ValidatedModelToolAction {
        ValidatedModelToolAction {
            tool_call_id: "call-1".to_string(),
            target_label: request.approval_target(),
            request,
            summary: "summary".to_string(),
        }
    }

    #[test]
    fn resolve_expands_home_for_file_and_shell_cwd() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some(home) = home else {
            return;
        };

        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: PathBuf::from("~/myfirstproject"),
        }));
        let resolution = AgentPathResolution::new(None, None, "/workspace");
        let resolved = resolve_agent_action_paths(action, &resolution);
        let ActionRequest::CreateDirectory(create_directory) = resolved.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, home.join("myfirstproject"));

        let action = validated(ActionRequest::ShellCommand(ShellCommandAction::new(
            "npm run dev",
            "~/myfirstproject",
        )));
        let resolved = resolve_agent_action_paths(action, &resolution);
        let ActionRequest::ShellCommand(shell) = resolved.request else {
            panic!("expected shell command action");
        };
        assert_eq!(shell.cwd, home.join("myfirstproject"));
    }

    #[test]
    fn resolve_shell_paths_uses_session_cwd_for_cwd_and_expected_paths() {
        let session = Session::new("session", "/workspace", "/workspace/playground");
        let mut action = ShellCommandAction::new("printf ok > out.txt", "project");
        action.expected_file = Some("out.txt".into());
        action.expected_files = vec!["nested/result.txt".into()];
        action.expected_directory = Some("nested".into());
        action.expected_directories = vec!["other".into()];

        let resolved = resolve_shell_command_paths(&session, &action);

        assert_eq!(resolved.cwd, PathBuf::from("/workspace/playground/project"));
        assert_eq!(
            resolved.expected_file,
            Some(PathBuf::from("/workspace/playground/project/out.txt"))
        );
        assert_eq!(
            resolved.expected_files,
            vec![PathBuf::from(
                "/workspace/playground/project/nested/result.txt"
            )]
        );
        assert_eq!(
            resolved.expected_directory,
            Some(PathBuf::from("/workspace/playground/project/nested"))
        );
        assert_eq!(
            resolved.expected_directories,
            vec![PathBuf::from("/workspace/playground/project/other")]
        );
    }

    #[test]
    fn resolve_shell_cwd_deduplicates_project_prefixed_current_cwd() {
        let session = Session::new(
            "session",
            "/Users/yuval/__git/elgar",
            "/Users/yuval/__git/elgar/playground/Nextjs-1",
        );
        let action =
            ShellCommandAction::new("ls -R playground/Nextjs-1", "elgar/playground/Nextjs-1");

        let resolved = resolve_shell_command_paths(&session, &action);

        assert_eq!(
            resolved.cwd,
            PathBuf::from("/Users/yuval/__git/elgar/playground/Nextjs-1")
        );
    }

    #[test]
    fn resolve_shell_cwd_deduplicates_project_relative_current_cwd() {
        let session = Session::new(
            "session",
            "/Users/yuval/__git/elgar",
            "/Users/yuval/__git/elgar/playground/Nextjs-1",
        );
        let action = ShellCommandAction::new("ls -R playground/Nextjs-1", "playground/Nextjs-1");

        let resolved = resolve_shell_command_paths(&session, &action);

        assert_eq!(
            resolved.cwd,
            PathBuf::from("/Users/yuval/__git/elgar/playground/Nextjs-1")
        );
    }

    #[test]
    fn resolve_shell_cwd_uses_current_cwd_prefix_for_child_project() {
        let session = Session::new(
            "session",
            "/Users/yuval/__git/elgar",
            "/Users/yuval/__git/elgar/playground",
        );
        let action = ShellCommandAction::new("ls -R playground/Nextjs-1", "playground/Nextjs-1");

        let resolved = resolve_shell_command_paths(&session, &action);

        assert_eq!(
            resolved.cwd,
            PathBuf::from("/Users/yuval/__git/elgar/playground/Nextjs-1")
        );
    }

    #[test]
    fn allowed_root_for_relative_file_action_is_session_cwd() {
        let session = Session::new("session", "/workspace", "/workspace/playground");
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: PathBuf::from("demo"),
            }),
            "create demo",
        );

        assert_eq!(
            allowed_root_for_action(&session, &action),
            PathBuf::from("/workspace/playground")
        );
    }
}
