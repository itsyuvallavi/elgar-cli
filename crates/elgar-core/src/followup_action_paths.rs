use std::path::{Path, PathBuf};

use crate::{action::ActionRequest, model_runtime::ValidatedModelToolAction, session::Session};

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

fn followup_base_path(session: &Session, path: &Path) -> Option<PathBuf> {
    match path.strip_prefix(&session.project_root) {
        Ok(relative) if relative.as_os_str().is_empty() => None,
        Ok(relative) => Some(relative.to_path_buf()),
        Err(_) => Some(path.to_path_buf()),
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
        let target = PathBuf::from("src").join("main.rs");
        let action = validated(ActionRequest::CreateFile(CreateFileAction {
            target_path: target,
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
    fn safe_create_already_under_base_is_left_alone() {
        let expected = PathBuf::from("verified-app").join("src");
        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: expected.clone(),
        }));

        let retargeted =
            retarget_safe_create_to_followup_base(Some(Path::new("verified-app")), action);

        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
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
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
    }

    #[test]
    fn repeated_desktop_relative_prefix_is_not_duplicated_under_desktop() {
        let base = PathBuf::from("/Users/yuval/Desktop");
        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: PathBuf::from("Desktop").join("Desktop").join("test"),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("test");
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
    }

    #[test]
    fn relative_absolute_prefix_is_not_duplicated_under_desktop() {
        let base = PathBuf::from("/Users/yuval/Desktop");
        let action = validated(ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("Users")
                .join("yuval")
                .join("Desktop")
                .join("ElgarLiveE2E")
                .join("react-ts-project-plan.md"),
            contents: "# Plan\n".to_string(),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("ElgarLiveE2E").join("react-ts-project-plan.md");
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateFile(create_file) = retargeted.request else {
            panic!("expected create file action");
        };
        assert_eq!(create_file.target_path, expected);
    }

    #[test]
    fn absolute_create_path_is_left_absolute() {
        let expected = PathBuf::from("/Users/yuval/Desktop/ElgarLiveE2E");
        let action = validated(ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: expected.clone(),
        }));

        let retargeted =
            retarget_safe_create_to_followup_base(Some(Path::new("/Users/yuval/Desktop")), action);

        let ActionRequest::CreateDirectory(create_directory) = retargeted.request else {
            panic!("expected create directory action");
        };
        assert_eq!(create_directory.target_path, expected);
    }

    #[test]
    fn followup_base_folder_name_prefix_is_not_duplicated() {
        let base = PathBuf::from("/Users/yuval/Desktop/ElgarLiveE2E");
        let action = validated(ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("ElgarLiveE2E").join("react-ts-project-plan.md"),
            contents: "# Plan\n".to_string(),
        }));

        let retargeted = retarget_safe_create_to_followup_base(Some(base.as_path()), action);

        let expected = base.join("react-ts-project-plan.md");
        assert_eq!(retargeted.target_label, expected.display().to_string());
        let ActionRequest::CreateFile(create_file) = retargeted.request else {
            panic!("expected create file action");
        };
        assert_eq!(create_file.target_path, expected);
    }

    #[test]
    fn non_create_action_is_left_alone() {
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
