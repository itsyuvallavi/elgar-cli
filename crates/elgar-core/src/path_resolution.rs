use std::path::{Path, PathBuf};

use crate::{
    action::{Action, ActionRequest},
    model_runtime::ValidatedModelToolAction,
    session::Session,
};

#[derive(Debug, Clone)]
pub(crate) struct AgentPathResolution {
    pub(crate) requested_project_base: Option<PathBuf>,
    pub(crate) followup_base: Option<PathBuf>,
    pub(crate) workspace_root: PathBuf,
}

impl AgentPathResolution {
    pub(crate) fn new(
        requested_project_base: Option<PathBuf>,
        followup_base: Option<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            requested_project_base,
            followup_base,
            workspace_root: workspace_root.into(),
        }
    }

    fn active_base(&self) -> Option<&Path> {
        self.requested_project_base
            .as_deref()
            .or(self.followup_base.as_deref())
    }
}

pub(crate) fn resolve_agent_action_paths(
    mut action: ValidatedModelToolAction,
    resolution: &AgentPathResolution,
) -> ValidatedModelToolAction {
    expand_home_paths(&mut action);

    if let Some(base) = resolution.active_base() {
        action = retarget_action_to_project_base(base, Some(&resolution.workspace_root), action);
    }

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

pub(crate) fn retarget_safe_create_to_followup_base(
    base: Option<&Path>,
    mut validated: ValidatedModelToolAction,
) -> ValidatedModelToolAction {
    let Some(base) = base else {
        return validated;
    };

    match &mut validated.request {
        ActionRequest::CreateFile(create_file) => {
            if let Some(target_path) = retargeted_safe_create_path(&create_file.target_path, base) {
                create_file.target_path = target_path;
            }
        }
        ActionRequest::CreateDirectory(create_directory) => {
            if let Some(target_path) =
                retargeted_safe_create_path(&create_directory.target_path, base)
            {
                create_directory.target_path = target_path;
            }
        }
        _ => return validated,
    }

    validated.target_label = validated.request.approval_target();
    validated
}

pub(crate) fn explicit_request_base(input: &str, home: Option<PathBuf>) -> Option<PathBuf> {
    let normalized = input.to_ascii_lowercase();
    let home = home?;
    if normalized.contains("desktop") {
        return Some(home.join("Desktop"));
    }
    if mentions_home_location(&normalized) {
        return Some(home);
    }
    None
}

pub(crate) fn followup_base_path_for_request(
    session: &Session,
    need_folder: bool,
    need_plan: bool,
) -> Option<PathBuf> {
    if !need_folder && !need_plan {
        return None;
    }

    let memory = session.project_memory();
    if need_plan {
        if let Some(path) = memory
            .latest_structured_plan()
            .map(|plan| &plan.project_root)
            .filter(|path| path.is_dir())
            .and_then(|path| followup_base_path(session, path))
        {
            return Some(path);
        }
        if let Some(path) = memory
            .latest_verified_plan()
            .map(|plan| &plan.project_root)
            .filter(|path| path.is_dir())
            .and_then(|path| followup_base_path(session, path))
        {
            return Some(path);
        }
    }

    if need_folder {
        return memory
            .latest_verified_folder()
            .map(|reference| &reference.path)
            .filter(|path| path.is_dir())
            .and_then(|path| followup_base_path(session, path));
    }

    None
}

pub(crate) fn project_base_target_path(
    target_path: &Path,
    base: &Path,
    workspace_root: Option<&Path>,
) -> Option<PathBuf> {
    if target_path.starts_with(base) {
        return None;
    }

    if target_path.is_absolute() {
        if let Some(workspace_root) = workspace_root {
            if let Ok(relative) = target_path.strip_prefix(workspace_root) {
                return project_base_target_path(relative, base, None);
            }
        }
        if let Some(target_path) = sibling_project_target_path(target_path, base) {
            return Some(target_path);
        }
        return strip_base_suffix_prefix(target_path, base).map(|suffix| {
            if suffix.as_os_str().is_empty() {
                base.to_path_buf()
            } else {
                base.join(suffix)
            }
        });
    }

    if let Some(suffix) = strip_base_suffix_prefix(target_path, base) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    if let Some(suffix) = strip_repeated_base_name_prefix(target_path, base) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    if let Some(suffix) = strip_generic_project_root_prefix(target_path) {
        return Some(if suffix.as_os_str().is_empty() {
            base.to_path_buf()
        } else {
            base.join(suffix)
        });
    }

    Some(base.join(target_path))
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

fn retarget_action_to_project_base(
    base: &Path,
    workspace_root: Option<&Path>,
    mut validated: ValidatedModelToolAction,
) -> ValidatedModelToolAction {
    match &mut validated.request {
        ActionRequest::CreateFile(create_file) => {
            if let Some(target_path) =
                project_base_target_path(&create_file.target_path, base, workspace_root)
            {
                create_file.target_path = target_path;
            }
        }
        ActionRequest::CreateDirectory(create_directory) => {
            if let Some(target_path) =
                project_base_target_path(&create_directory.target_path, base, workspace_root)
            {
                create_directory.target_path = target_path;
            }
        }
        ActionRequest::PatchFile(patch_file) => {
            if let Some(target_path) =
                project_base_target_path(&patch_file.target_path, base, workspace_root)
            {
                patch_file.target_path = target_path;
            }
        }
        ActionRequest::OverwriteFile(overwrite_file) => {
            if let Some(target_path) =
                project_base_target_path(&overwrite_file.target_path, base, workspace_root)
            {
                overwrite_file.target_path = target_path;
            }
        }
        ActionRequest::DeleteFile(delete_file) => {
            if let Some(target_path) =
                project_base_target_path(&delete_file.target_path, base, workspace_root)
            {
                delete_file.target_path = target_path;
            }
        }
        ActionRequest::MoveFile(move_file) => {
            if let Some(source_path) =
                project_base_target_path(&move_file.source_path, base, workspace_root)
            {
                move_file.source_path = source_path;
            }
            if let Some(target_path) =
                project_base_target_path(&move_file.target_path, base, workspace_root)
            {
                move_file.target_path = target_path;
            }
        }
        ActionRequest::ShellCommand(_) => return validated,
    }

    validated.target_label = validated.request.approval_target();
    validated
}

fn followup_base_path(session: &Session, path: &Path) -> Option<PathBuf> {
    match path.strip_prefix(&session.project_root) {
        Ok(relative) if relative.as_os_str().is_empty() => None,
        Ok(relative) => Some(relative.to_path_buf()),
        Err(_) => Some(path.to_path_buf()),
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

fn retargeted_safe_create_path(target_path: &Path, base: &Path) -> Option<PathBuf> {
    if target_path.is_absolute() || target_path.starts_with(base) {
        return None;
    }

    if let Some(suffix) = strip_home_relative_prefix(target_path) {
        return if suffix.as_os_str().is_empty() {
            Some(base.to_path_buf())
        } else {
            Some(base.join(suffix))
        };
    }

    if let Some(suffix) = strip_relative_prefix(target_path, &absolute_path_components(base)) {
        return Some(base.join(suffix));
    }

    if let Some(suffix) = strip_repeated_base_name_prefix(target_path, base) {
        return if suffix.as_os_str().is_empty() {
            Some(base.to_path_buf())
        } else {
            Some(base.join(suffix))
        };
    }

    Some(base.join(target_path))
}

fn sibling_project_target_path(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let parent = base.parent()?;
    let relative = target_path.strip_prefix(parent).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    let std::path::Component::Normal(first) = first else {
        return None;
    };
    if Some(first) == base.file_name() || !is_generic_project_root_component(first) {
        return None;
    }
    let suffix = components.as_path();
    Some(if suffix.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(suffix)
    })
}

fn is_generic_project_root_component(value: &std::ffi::OsStr) -> bool {
    let value = value.to_string_lossy().to_ascii_lowercase();
    matches!(
        value.as_str(),
        "project"
            | "app"
            | "my-app"
            | "my-next-app"
            | "my-nextapp"
            | "my-nextjs-app"
            | "react-app"
            | "react-project"
            | "vite-project"
    )
}

fn strip_base_suffix_prefix(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let target_components = normal_path_components(target_path);
    let base_components = normal_path_components(base);
    for start in 0..base_components.len() {
        let base_suffix = &base_components[start..];
        if base_suffix.is_empty() || target_components.len() < base_suffix.len() {
            continue;
        }
        for target_start in 0..=target_components.len() - base_suffix.len() {
            let target_end = target_start + base_suffix.len();
            if target_components[target_start..target_end] == base_suffix[..] {
                return Some(target_components[target_end..].iter().collect());
            }
        }
    }
    None
}

fn strip_repeated_base_name_prefix(target_path: &Path, base: &Path) -> Option<PathBuf> {
    let base_name = base.file_name()?;
    let mut remaining = target_path;
    let mut stripped = false;

    while let Ok(suffix) = remaining.strip_prefix(Path::new(base_name)) {
        stripped = true;
        if suffix.as_os_str().is_empty() {
            return Some(PathBuf::new());
        }
        remaining = suffix;
    }

    stripped.then(|| remaining.to_path_buf())
}

fn strip_generic_project_root_prefix(target_path: &Path) -> Option<PathBuf> {
    let mut components = target_path.components();
    let first = components.next()?;
    let first = first.as_os_str().to_string_lossy().to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "project"
            | "my-nextjs-app"
            | "nextjs-app"
            | "next-app"
            | "nextjs-project"
            | "next-tailwind-project"
            | "next-tailwind-ts-project"
            | "next-tailwind-app"
    ) {
        return Some(components.as_path().to_path_buf());
    }

    None
}

fn mentions_home_location(input: &str) -> bool {
    input.contains("~/")
        || input.contains("in ~")
        || input.contains("under ~")
        || input.contains("inside ~")
        || input.contains("my home")
        || input.contains("home folder")
        || input.contains("home directory")
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

fn absolute_path_components(path: &Path) -> Vec<std::ffi::OsString> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect()
}

fn strip_relative_prefix(target_path: &Path, prefix: &[std::ffi::OsString]) -> Option<PathBuf> {
    if prefix.is_empty() {
        return None;
    }

    let mut target_components = target_path.components();
    for prefix_component in prefix {
        let component = target_components.next()?;
        if component.as_os_str() != prefix_component {
            return None;
        }
    }

    let suffix = target_components.as_path();
    if suffix.as_os_str().is_empty() {
        None
    } else {
        Some(suffix.to_path_buf())
    }
}

fn normal_path_components(path: &Path) -> Vec<std::ffi::OsString> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{CreateDirectoryAction, CreateFileAction, ShellCommandAction};

    fn validated(request: ActionRequest) -> ValidatedModelToolAction {
        ValidatedModelToolAction {
            tool_call_id: "call-1".to_string(),
            target_label: request.approval_target(),
            request,
            summary: "summary".to_string(),
        }
    }

    #[test]
    fn explicit_desktop_request_base_uses_supplied_home() {
        assert_eq!(
            explicit_request_base("create this on the desktop", Some("home".into())),
            Some(PathBuf::from("home").join("Desktop"))
        );
    }

    #[test]
    fn explicit_home_request_base_uses_supplied_home() {
        assert_eq!(
            explicit_request_base(
                "i want you to create a folder in ~/ call it myfirstproject",
                Some("home".into())
            ),
            Some(PathBuf::from("home"))
        );
    }

    #[test]
    fn safe_create_retarget_updates_path_and_label() {
        let base = PathBuf::from("verified-app");
        let action = validated(ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("src").join("main.rs"),
            contents: "fn main() {}\n".to_string(),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("src").join("main.rs");
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateFile(create_file) = retargeted.request else {
            panic!("expected create file action");
        };
        assert_eq!(create_file.target_path, expected);
    }

    #[test]
    fn safe_create_tilde_path_is_retargeted_under_home_base() {
        let base = PathBuf::from("/Users/yuval");
        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: PathBuf::from("~/myfirstproject"),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("myfirstproject");
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
    }

    #[test]
    fn desktop_relative_prefix_is_not_duplicated_under_desktop() {
        let base = PathBuf::from("/Users/yuval/Desktop");
        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: PathBuf::from("Desktop").join("ElgarLiveE2E"),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("ElgarLiveE2E");
        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
    }

    #[test]
    fn requested_desktop_project_target_path_does_not_duplicate_desktop_or_folder() {
        let base = Path::new("/Users/yuval/Desktop/TEST");
        let workspace = Path::new("/Users/yuval/__git/elgar");

        assert_eq!(
            project_base_target_path(Path::new("Desktop/TEST"), base, Some(workspace)),
            Some(PathBuf::from("/Users/yuval/Desktop/TEST"))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("Desktop/TEST/package.json"),
                base,
                Some(workspace)
            ),
            Some(PathBuf::from("/Users/yuval/Desktop/TEST/package.json"))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/Desktop/Desktop/TEST/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
        assert_eq!(
            project_base_target_path(
                Path::new("/Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js"),
                base,
                Some(workspace),
            ),
            Some(PathBuf::from(
                "/Users/yuval/Desktop/TEST/tailwind.config.js"
            ))
        );
    }

    #[test]
    fn requested_project_target_path_retargets_generic_model_roots() {
        let base = Path::new("FreshNextApp");

        assert_eq!(
            project_base_target_path(Path::new("project"), base, None),
            Some(PathBuf::from("FreshNextApp"))
        );
        assert_eq!(
            project_base_target_path(Path::new("project/package.json"), base, None),
            Some(PathBuf::from("FreshNextApp/package.json"))
        );
        assert_eq!(
            project_base_target_path(Path::new("my-nextjs-app/tsconfig.json"), base, None),
            Some(PathBuf::from("FreshNextApp/tsconfig.json"))
        );
        assert_eq!(
            project_base_target_path(Path::new("app/page.tsx"), base, None),
            Some(PathBuf::from("FreshNextApp/app/page.tsx"))
        );
        assert_eq!(
            project_base_target_path(Path::new("FreshNextApp/package.json"), base, None),
            None
        );
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
    fn non_create_safe_retarget_is_left_alone() {
        let action = validated(ActionRequest::ShellCommand(ShellCommandAction::new(
            "cargo test",
            ".",
        )));
        let original_target_label = action.target_label.clone();

        let retargeted =
            retarget_safe_create_to_followup_base(Some(Path::new("verified-app")), action);

        assert_eq!(retargeted.target_label, original_target_label);
        let ActionRequest::ShellCommand(shell) = retargeted.request else {
            panic!("expected shell command action");
        };
        assert_eq!(shell.cwd, PathBuf::from("."));
    }
}
