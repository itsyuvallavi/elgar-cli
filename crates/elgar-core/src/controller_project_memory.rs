use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    action::{Action, ActionRequest, FileActionVerification},
    controller_reporting::verified_shell_expected_directories,
    event::VerifiedActionResult,
    session::{
        Session, StructuredProjectPlan, StructuredProjectPlanStatus, VerifiedFolderReference,
        VerifiedPlanReference,
    },
};

pub(crate) fn record_verified_project_memory(
    session: &mut Session,
    action: &Action,
    result: &VerifiedActionResult,
) {
    let action_id = action.id.clone();
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            let path = verified_directory_path(session, result).unwrap_or_else(|| {
                resolve_session_path(&session.cwd, &create_directory.target_path)
            });
            session.record_verified_folder_reference(VerifiedFolderReference {
                path,
                source_action_id: action_id,
            });
        }
        ActionRequest::CreateFile(create_file) => {
            let path = verified_file_write_path(session, result)
                .unwrap_or_else(|| resolve_session_path(&session.cwd, &create_file.target_path));
            if should_record_file_write_as_verified_plan(
                session,
                &create_file.target_path,
                &create_file.contents,
                &path,
            ) {
                record_verified_plan_memory(session, &action_id, path);
            }
        }
        ActionRequest::OverwriteFile(overwrite_file) => {
            let path = verified_file_write_path(session, result)
                .unwrap_or_else(|| resolve_session_path(&session.cwd, &overwrite_file.target_path));
            if should_record_file_write_as_verified_plan(
                session,
                &overwrite_file.target_path,
                &overwrite_file.contents,
                &path,
            ) {
                record_verified_plan_memory(session, &action_id, path);
            }
        }
        ActionRequest::ShellCommand(shell_command) => {
            let shell_cwd = normalize_path(&shell_command.cwd);
            for path in verified_shell_expected_directories(shell_command) {
                let path = normalize_path(path);
                if shell_expected_directory_is_current_or_ancestor(&path, &shell_cwd) {
                    continue;
                }
                session.record_verified_folder_reference(VerifiedFolderReference {
                    path,
                    source_action_id: action_id.clone(),
                });
            }

            if let Some(path) = shell_command
                .expected_file
                .as_ref()
                .filter(|path| is_plan_path(path))
                .cloned()
                .or_else(|| {
                    shell_command
                        .expected_files
                        .iter()
                        .find(|path| is_plan_path(path))
                        .cloned()
                })
            {
                let reference = VerifiedPlanReference {
                    project_root: path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| session.project_root.clone()),
                    path,
                    source_action_id: action_id,
                };
                session.record_verified_plan_reference(reference.clone());
                session
                    .record_structured_project_plan(structured_plan_from_verified_plan(&reference));
            }
        }
        _ => {}
    }
}

fn record_verified_plan_memory(session: &mut Session, action_id: &str, path: PathBuf) {
    let project_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| session.project_root.clone());
    let reference = VerifiedPlanReference {
        path,
        project_root,
        source_action_id: action_id.to_string(),
    };
    session.record_verified_plan_reference(reference.clone());
    session.record_structured_project_plan(structured_plan_from_verified_plan(&reference));
}

fn structured_plan_from_verified_plan(reference: &VerifiedPlanReference) -> StructuredProjectPlan {
    let (expected_directories, expected_files) = fs::read_to_string(&reference.path)
        .map(|contents| {
            extract_expected_plan_paths(&contents, &reference.project_root, &reference.path)
        })
        .unwrap_or_else(|_| (Vec::new(), Vec::new()));

    StructuredProjectPlan {
        source_action_id: Some(reference.source_action_id.clone()),
        source_plan_path: reference.path.clone(),
        project_root: reference.project_root.clone(),
        stage: "verified-plan".to_string(),
        status: Default::default(),
        expected_directories,
        expected_files,
    }
}

fn extract_expected_plan_paths(
    contents: &str,
    project_root: &Path,
    source_plan_path: &Path,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut expected_directories = Vec::new();
    let mut expected_files = Vec::new();
    let mut tree_stack: Vec<PathBuf> = Vec::new();
    let mut plain_indent_stack: Vec<(usize, PathBuf)> = Vec::new();
    let lines = contents.lines().collect::<Vec<_>>();
    let mut fenced_path_block: Option<bool> = None;

    for (index, line) in lines.iter().enumerate() {
        if let Some(path_block) = markdown_fence_path_mode(line) {
            fenced_path_block = fenced_path_block.map(|_| None).unwrap_or(Some(path_block));
            continue;
        }
        if fenced_path_block == Some(false) {
            continue;
        }

        if let Some(root_path) = tree_root_path_from_line(line) {
            tree_stack.clear();
            tree_stack.push(root_path.clone());
            plain_indent_stack.clear();
            plain_indent_stack.push((0, root_path.clone()));
            push_plan_path(
                project_root,
                source_plan_path,
                root_path,
                PlanPathKind::Directory,
                &mut expected_directories,
                &mut expected_files,
            );
            continue;
        }

        let infer_directory =
            tree_line_depth(line).is_some_and(|depth| next_tree_depth(&lines, index) > Some(depth));
        if let Some((path, kind)) = tree_path_from_line(line, &mut tree_stack, infer_directory) {
            push_plan_path(
                project_root,
                source_plan_path,
                path,
                kind,
                &mut expected_directories,
                &mut expected_files,
            );
            continue;
        }

        let infer_plain_directory = plain_line_indent(line)
            .is_some_and(|indent| next_plain_indent(&lines, index) > Some(indent));
        if let Some((path, kind)) =
            plain_indented_path_from_line(line, &mut plain_indent_stack, infer_plain_directory)
        {
            push_plan_path(
                project_root,
                source_plan_path,
                path,
                kind,
                &mut expected_directories,
                &mut expected_files,
            );
            continue;
        }

        if let Some((path, kind)) = inline_path_from_line(line) {
            push_plan_path(
                project_root,
                source_plan_path,
                path,
                kind,
                &mut expected_directories,
                &mut expected_files,
            );
        }
    }

    (expected_directories, expected_files)
}

#[derive(Debug, Clone, Copy)]
enum PlanPathKind {
    Directory,
    File,
}

fn push_plan_path(
    project_root: &Path,
    source_plan_path: &Path,
    path: PathBuf,
    kind: PlanPathKind,
    expected_directories: &mut Vec<PathBuf>,
    expected_files: &mut Vec<PathBuf>,
) {
    let Some(path) = resolve_plan_path(project_root, &path) else {
        return;
    };
    if is_plan_self_reference(source_plan_path, &path) {
        return;
    }

    if matches!(kind, PlanPathKind::File) {
        push_parent_plan_directories(project_root, &path, expected_directories);
    }

    let bucket = match kind {
        PlanPathKind::Directory => expected_directories,
        PlanPathKind::File => expected_files,
    };
    if !bucket.contains(&path) {
        bucket.push(path);
    }
}

fn is_plan_self_reference(source_plan_path: &Path, candidate: &Path) -> bool {
    let Some(source_name) = source_plan_path.file_name() else {
        return false;
    };
    candidate.file_name() == Some(source_name) && is_plan_path(candidate)
}

fn push_parent_plan_directories(
    project_root: &Path,
    path: &Path,
    expected_directories: &mut Vec<PathBuf>,
) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(relative_parent) = parent.strip_prefix(project_root) else {
        return;
    };

    let mut current = project_root.to_path_buf();
    for component in relative_parent.components() {
        let Component::Normal(segment) = component else {
            return;
        };
        current.push(segment);
        if current != project_root && !expected_directories.contains(&current) {
            expected_directories.push(current.clone());
        }
    }
}

fn tree_path_from_line(
    line: &str,
    stack: &mut Vec<PathBuf>,
    infer_directory: bool,
) -> Option<(PathBuf, PlanPathKind)> {
    let (marker_index, marker_len) = tree_marker(line)?;
    let depth = tree_depth(&line[..marker_index]);
    let name = line[marker_index + marker_len..].trim();
    let (name, kind) = clean_plan_path_token_with_inference(name, infer_directory)?;
    let parent = stack.get(depth).cloned().unwrap_or_default();
    let path = parent.join(&name);

    if matches!(kind, PlanPathKind::Directory) {
        if stack.len() <= depth + 1 {
            stack.resize(depth + 2, PathBuf::new());
        }
        stack[depth + 1] = path.clone();
        stack.truncate(depth + 2);
    }

    Some((path, kind))
}

fn plain_indented_path_from_line(
    line: &str,
    stack: &mut Vec<(usize, PathBuf)>,
    infer_directory: bool,
) -> Option<(PathBuf, PlanPathKind)> {
    let indent = plain_line_indent(line)?;
    let (name, kind) = clean_plan_path_token_with_inference(line.trim(), infer_directory)?;

    while stack
        .last()
        .is_some_and(|(stack_indent, _)| *stack_indent >= indent)
    {
        stack.pop();
    }

    let parent = stack
        .last()
        .map(|(_, path)| path.clone())
        .unwrap_or_default();
    let path = parent.join(&name);

    if matches!(kind, PlanPathKind::Directory) {
        stack.push((indent, path.clone()));
    }

    Some((path, kind))
}

fn plain_line_indent(line: &str) -> Option<usize> {
    if tree_marker(line).is_some() {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == line {
        return None;
    }
    Some(line.len().saturating_sub(line.trim_start().len()))
}

fn next_plain_indent(lines: &[&str], index: usize) -> Option<usize> {
    lines
        .get(index + 1)
        .and_then(|line| plain_line_indent(line))
}

fn tree_marker(line: &str) -> Option<(usize, usize)> {
    ["├──", "└──", "├─", "└─"]
        .into_iter()
        .filter_map(|marker| line.find(marker).map(|index| (index, marker.len())))
        .min_by_key(|(index, _)| *index)
}

fn tree_line_depth(line: &str) -> Option<usize> {
    let (marker_index, _) = tree_marker(line)?;
    Some(tree_depth(&line[..marker_index]))
}

fn next_tree_depth(lines: &[&str], index: usize) -> Option<usize> {
    lines.get(index + 1).and_then(|line| tree_line_depth(line))
}

fn tree_root_path_from_line(line: &str) -> Option<PathBuf> {
    if line != line.trim_start() {
        return None;
    }

    let trimmed = strip_markdown_list_marker(line.trim());
    if trimmed.chars().any(char::is_whitespace) || trimmed.contains("──") {
        return None;
    }
    let (path, kind) = clean_plan_path_token(trimmed)?;
    matches!(kind, PlanPathKind::Directory).then_some(path)
}

fn tree_depth(prefix: &str) -> usize {
    let pipe_depth = prefix.chars().filter(|ch| *ch == '│').count();
    if pipe_depth == 0 {
        return prefix.chars().count() / 4;
    }

    let trailing_indent_after_last_pipe = prefix
        .rsplit_once('│')
        .map(|(_, suffix)| suffix.chars().count() / 4)
        .unwrap_or(0);
    pipe_depth + trailing_indent_after_last_pipe
}

fn inline_path_from_line(line: &str) -> Option<(PathBuf, PlanPathKind)> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with('|')
        || trimmed.starts_with("```")
    {
        return None;
    }

    let line = strip_markdown_list_marker(trimmed);
    let (token, suffix) = line
        .split_once(char::is_whitespace)
        .map(|(token, suffix)| (token, Some(suffix.trim_start())))
        .unwrap_or((line, None));
    let token = token.trim_end_matches(|ch: char| matches!(ch, ',' | ';' | ':' | ')'));
    let path = clean_plan_path_token(token)?;

    if suffix.is_some_and(|suffix| !suffix.starts_with('|')) {
        return None;
    }

    Some(path)
}

fn clean_plan_path_token(token: &str) -> Option<(PathBuf, PlanPathKind)> {
    clean_plan_path_token_with_inference(token, false)
}

fn clean_plan_path_token_with_inference(
    token: &str,
    infer_directory: bool,
) -> Option<(PathBuf, PlanPathKind)> {
    if is_runtime_generated_plan_artifact(token) {
        return None;
    }

    let token = strip_inline_plan_comment(strip_markdown_list_marker(token))
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'');
    if token.is_empty()
        || token == "."
        || token.contains("://")
        || token.contains("...")
        || token.contains('=')
        || token.contains(':')
        || token.starts_with('$')
        || token.starts_with('~')
    {
        return None;
    }

    let kind = if token.ends_with('/') {
        PlanPathKind::Directory
    } else if path_has_file_extension(Path::new(token)) {
        PlanPathKind::File
    } else if infer_directory {
        PlanPathKind::Directory
    } else {
        return None;
    };

    Some((PathBuf::from(token.trim_end_matches('/')), kind))
}

fn is_runtime_generated_plan_artifact(token: &str) -> bool {
    let Some(annotation) = token
        .split_once(" #")
        .map(|(_, annotation)| annotation)
        .or_else(|| token.split_once(" (").map(|(_, annotation)| annotation))
    else {
        return false;
    };

    let annotation = annotation.trim().trim_end_matches(')').to_ascii_lowercase();
    let generated = annotation.contains("generated") || annotation.contains("created");
    let runtime = annotation.contains("runtime") || annotation.contains("run time");

    generated && runtime
}

fn strip_inline_plan_comment(token: &str) -> &str {
    token
        .split_once(" #")
        .map(|(path, _)| path)
        .or_else(|| token.split_once(" (").map(|(path, _)| path))
        .unwrap_or(token)
}

fn markdown_fence_path_mode(line: &str) -> Option<bool> {
    let trimmed = line.trim();
    let language = trimmed.strip_prefix("```")?.trim();
    Some(matches!(
        language.to_ascii_lowercase().as_str(),
        "" | "text" | "txt" | "tree" | "plaintext"
    ))
}

fn strip_markdown_list_marker(token: &str) -> &str {
    let trimmed = token.trim();
    let without_marker = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| numbered_markdown_list_item(trimmed))
        .unwrap_or(trimmed)
        .trim();

    without_marker
        .strip_prefix("[ ]")
        .or_else(|| without_marker.strip_prefix("[x]"))
        .or_else(|| without_marker.strip_prefix("[X]"))
        .unwrap_or(without_marker)
        .trim()
}

fn numbered_markdown_list_item(trimmed: &str) -> Option<&str> {
    let (prefix, suffix) = trimmed.split_once(". ")?;
    (!prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())).then_some(suffix)
}

fn path_has_file_extension(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with('.')
                || path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        !extension.is_empty()
                            && extension.len() <= 12
                            && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
                    })
        })
}

fn resolve_plan_path(project_root: &Path, path: &Path) -> Option<PathBuf> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        return None;
    }

    let path = if let Some(path) = strip_cwd_relative_project_root_prefix(project_root, path) {
        project_root.join(path)
    } else if path
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        == project_root.file_name()
    {
        project_root
            .parent()
            .map(|parent| parent.join(path))
            .unwrap_or_else(|| project_root.join(path))
    } else {
        project_root.join(path)
    };

    Some(path)
}

fn strip_cwd_relative_project_root_prefix(project_root: &Path, path: &Path) -> Option<PathBuf> {
    let path_components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let root_components = project_root
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();

    for prefix_len in (2..=path_components.len()).rev() {
        if prefix_len > root_components.len() {
            continue;
        }
        if root_components[root_components.len() - prefix_len..] == path_components[..prefix_len] {
            let mut stripped = PathBuf::new();
            for component in &path_components[prefix_len..] {
                stripped.push(component);
            }
            return Some(stripped);
        }
    }

    None
}

fn verified_directory_path(session: &Session, result: &VerifiedActionResult) -> Option<PathBuf> {
    match result {
        VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
            Some(resolve_session_path(&session.cwd, path))
        }
        _ => None,
    }
}

fn verified_file_write_path(session: &Session, result: &VerifiedActionResult) -> Option<PathBuf> {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            Some(resolve_session_path(&session.cwd, path))
        }
        VerifiedActionResult::File(FileActionVerification::FileCreated { path })
        | VerifiedActionResult::File(FileActionVerification::FileOverwritten { path }) => {
            Some(resolve_session_path(&session.cwd, path))
        }
        _ => None,
    }
}

fn resolve_session_path(base: &Path, target_path: impl AsRef<Path>) -> PathBuf {
    let target_path = target_path.as_ref();
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        base.join(target_path)
    }
}

fn shell_expected_directory_is_current_or_ancestor(path: &Path, cwd: &Path) -> bool {
    path == cwd || cwd.starts_with(path)
}

fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
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

pub(crate) fn is_plan_path_or_contents(path: &Path, contents: &str) -> bool {
    is_plan_path(path) || contents_looks_like_plan(contents)
}

fn should_record_file_write_as_verified_plan(
    session: &Session,
    requested_path: &Path,
    contents: &str,
    verified_path: &Path,
) -> bool {
    if is_plan_path(requested_path) || is_plan_path(verified_path) {
        return true;
    }

    contents_looks_like_plan(contents)
        && !is_active_plan_expected_implementation_file(session, verified_path)
}

fn is_active_plan_expected_implementation_file(session: &Session, path: &Path) -> bool {
    let Some(plan) = session.project_memory().latest_structured_plan() else {
        return false;
    };
    if plan.runtime_status() != StructuredProjectPlanStatus::Executing {
        return false;
    }

    let path = normalize_path(path);
    path != normalize_path(&plan.source_plan_path)
        && plan
            .expected_files
            .iter()
            .any(|expected| normalize_path(expected) == path)
}

fn is_plan_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    (extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt"))
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.to_ascii_lowercase().contains("plan"))
}

fn contents_looks_like_plan(contents: &str) -> bool {
    let lower = contents.to_ascii_lowercase();
    lower.contains("project plan")
        || lower.contains("# plan")
        || (lower.contains("## directory structure") && lower.contains("key files"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::{
        action::{
            Action, ActionRequest, CreateDirectoryAction, CreateFileAction, ShellCommandAction,
        },
        event::{FileActionVerification, ShellActionVerification, VerifiedActionResult},
        session::Session,
    };

    use super::*;

    #[test]
    fn records_plan_txt_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/plan.txt"),
                contents: "plan".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/plan.txt".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("plan.txt should be remembered");
        assert_eq!(plan.path, PathBuf::from("/repo/DesktopProject/plan.txt"));
        assert_eq!(plan.project_root, PathBuf::from("/repo/DesktopProject"));
    }

    #[test]
    fn records_verified_paths_relative_to_session_cwd() {
        let root = PathBuf::from("/repo");
        let cwd = root.join("playground");
        let mut session = Session::new("session", &root, &cwd);
        let folder_action = Action::proposed(
            "action-folder",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: PathBuf::from("WeatherApp"),
            }),
            "create folder",
        )
        .approve()
        .mark_applied();
        let folder_result = VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
            path: "WeatherApp".to_string(),
        });

        record_verified_project_memory(&mut session, &folder_action, &folder_result);

        assert_eq!(
            session
                .project_memory()
                .latest_verified_folder()
                .expect("folder should be remembered")
                .path,
            PathBuf::from("/repo/playground/WeatherApp")
        );

        let plan_action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("WeatherApp/project-plan.md"),
                contents: "# Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let plan_result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "WeatherApp/project-plan.md".to_string(),
        });

        record_verified_project_memory(&mut session, &plan_action, &plan_result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("plan should be remembered under cwd");
        assert_eq!(
            plan.path,
            PathBuf::from("/repo/playground/WeatherApp/project-plan.md")
        );
        assert_eq!(
            plan.project_root,
            PathBuf::from("/repo/playground/WeatherApp")
        );
    }

    #[test]
    fn shell_expected_directory_memory_skips_current_and_parent_directories() {
        let root = PathBuf::from("/repo");
        let cwd = root.join("playground/workspace");
        let mut session = Session::new("session", &root, &cwd);
        let mut shell = ShellCommandAction::new("find . -maxdepth 2", cwd.clone());
        shell.expected_directory = Some(cwd.join(".."));
        let action = Action::proposed(
            "action-shell",
            ActionRequest::ShellCommand(shell),
            "run find",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::Shell(ShellActionVerification {
            command: "find . -maxdepth 2".to_string(),
            cwd: cwd.display().to_string(),
            stdout: ".".to_string(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            exit_code: Some(0),
            elapsed_millis: 1,
            timed_out: false,
            verified_effect: Some(format!(
                "verified directory exists: {}",
                cwd.join("..").display()
            )),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert!(session.project_memory().latest_verified_folder().is_none());
    }

    #[test]
    fn shell_expected_directory_memory_keeps_child_directory() {
        let root = PathBuf::from("/repo");
        let cwd = root.join("playground/workspace");
        let expected = cwd.join("reports");
        let mut session = Session::new("session", &root, &cwd);
        let mut shell = ShellCommandAction::new("mkdir reports", cwd.clone());
        shell.expected_directory = Some(expected.clone());
        let action = Action::proposed(
            "action-shell",
            ActionRequest::ShellCommand(shell),
            "run mkdir",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::Shell(ShellActionVerification {
            command: "mkdir reports".to_string(),
            cwd: cwd.display().to_string(),
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            exit_code: Some(0),
            elapsed_millis: 1,
            timed_out: false,
            verified_effect: Some(format!("verified directory exists: {}", expected.display())),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert_eq!(
            session
                .project_memory()
                .latest_verified_folder()
                .expect("child expected directory should be remembered")
                .path,
            expected
        );
    }

    #[test]
    fn records_structured_plan_contract_from_verified_plan_file() {
        let root =
            std::env::temp_dir().join(format!("elgar-structured-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("DemoApp")).unwrap();
        fs::write(
            root.join("DemoApp/project-plan.md"),
            "# Plan\n\n```text\nDemoApp/\n├── src/\n│   └── main.py\n├── requirements.txt\n└── .gitignore\n```\n\n- docs/usage.md\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DemoApp/project-plan.md"),
                contents: "# Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("DemoApp/project-plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan file should create structured plan state");
        assert_eq!(structured.source_action_id.as_deref(), Some("action-plan"));
        assert_eq!(structured.project_root, root.join("DemoApp"));
        assert_eq!(
            structured.source_plan_path,
            root.join("DemoApp/project-plan.md")
        );
        assert!(structured
            .expected_directories
            .contains(&root.join("DemoApp")));
        assert!(structured
            .expected_directories
            .contains(&root.join("DemoApp/src")));
        assert!(structured
            .expected_files
            .contains(&root.join("DemoApp/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("DemoApp/requirements.txt")));
        assert!(structured
            .expected_files
            .contains(&root.join("DemoApp/.gitignore")));
        assert!(structured
            .expected_files
            .contains(&root.join("DemoApp/docs/usage.md")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_deeper_unicode_tree_prefixes_under_expected_parent() {
        let root =
            std::env::temp_dir().join(format!("elgar-deep-structured-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("DemoApp")).unwrap();
        fs::write(
            root.join("DemoApp/project-plan.md"),
            "# Plan\n\n```text\nDemoApp/\n├── src/\n│   ├── package/\n│   │   └── module.py\n└── requirements.txt\n```\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DemoApp/project-plan.md"),
                contents: "# Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("DemoApp/project-plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan file should create structured plan state");
        assert!(structured
            .expected_directories
            .contains(&root.join("DemoApp/src/package")));
        assert!(structured
            .expected_files
            .contains(&root.join("DemoApp/src/package/module.py")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_extensionless_tree_directories_when_child_paths_exist() {
        let root = std::env::temp_dir().join(format!(
            "elgar-extensionless-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tui-state-test")).unwrap();
        fs::write(
            root.join("tui-state-test/PLAN.md"),
            "# Project Plan\n\nThis document outlines the plan for a tiny Python CLI application.\n\n## File Tree\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("tui-state-test/PLAN.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("tui-state-test/PLAN.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified live plan should create structured plan state");
        assert!(structured
            .expected_directories
            .contains(&root.join("tui-state-test/src")));
        assert!(structured
            .expected_files
            .contains(&root.join("tui-state-test/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("tui-state-test/requirements.txt")));
        assert!(!structured
            .expected_files
            .contains(&root.join("tui-state-test/main.py")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn excludes_runtime_generated_tree_artifacts_from_expected_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-runtime-generated-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ReadingJournal")).unwrap();
        fs::write(
            root.join("ReadingJournal/plan.md"),
            "# Project Plan\n\n## File Tree\n```text\nReadingJournal/\n├── README.md\n├── requirements.txt\n├── journal.db             # SQLite database (generated at runtime)\n├── src/\n│   └── main.py\n└── tests/\n    └── test_main.py\n```\n\n## Verification\n- Run tests.\n\n## Acceptance Criteria\n- Runtime creates the database on first use.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("ReadingJournal/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("ReadingJournal/plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured plan state");
        assert!(structured
            .expected_files
            .contains(&root.join("ReadingJournal/README.md")));
        assert!(structured
            .expected_files
            .contains(&root.join("ReadingJournal/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("ReadingJournal/tests/test_main.py")));
        assert!(
            !structured
                .expected_files
                .contains(&root.join("ReadingJournal/journal.db")),
            "runtime-generated database should not be a required implementation file"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_single_dash_tree_markers_from_live_plan_output() {
        let root = std::env::temp_dir().join(format!(
            "elgar-single-dash-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("LivePlan")).unwrap();
        fs::write(
            root.join("LivePlan/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n└─ main.py\nrequirements.txt\n```\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("LivePlan/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("LivePlan/plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified live plan should create structured plan state");
        assert!(structured
            .expected_directories
            .contains(&root.join("LivePlan/src")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/requirements.txt")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_plain_indented_tree_children_under_directory_root() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plain-indented-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("LivePlan")).unwrap();
        fs::write(
            root.join("LivePlan/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n  main.py\nrequirements.txt\n```\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("LivePlan/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("LivePlan/plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified live plan should create structured plan state");
        assert!(structured
            .expected_directories
            .contains(&root.join("LivePlan/src")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/requirements.txt")));
        assert!(!structured
            .expected_files
            .contains(&root.join("LivePlan/main.py")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn strips_markdown_list_markers_from_indented_file_tree_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-markdown-list-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("plan-review-copy-test")).unwrap();
        fs::write(
            root.join("plan-review-copy-test/PLAN.md"),
            "# Project Plan\n\n```text\n  - app.py\n  - __init__.py\n  - cli.py\n  - README.md\n  - requirements.txt\n  - tests\n    - test_app.py\n```\n\n## Verification\n- Ensure all listed files exist.\n\n## Acceptance Criteria\n- The project directory exists with the specified structure.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("plan-review-copy-test/PLAN.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("plan-review-copy-test/PLAN.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified live plan should create structured plan state");
        let project = root.join("plan-review-copy-test");
        assert!(structured
            .expected_directories
            .contains(&project.join("tests")));
        assert!(structured.expected_files.contains(&project.join("app.py")));
        assert!(structured
            .expected_files
            .contains(&project.join("__init__.py")));
        assert!(structured.expected_files.contains(&project.join("cli.py")));
        assert!(
            structured
                .expected_files
                .contains(&project.join("README.md")),
            "expected files: {:#?}",
            structured.expected_files
        );
        assert!(structured
            .expected_files
            .contains(&project.join("requirements.txt")));
        assert!(structured
            .expected_files
            .contains(&project.join("tests/test_app.py")));
        assert!(!structured
            .expected_files
            .iter()
            .chain(structured.expected_directories.iter())
            .any(|path| path.to_string_lossy().contains("/- ")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_indented_tree_sibling_after_directory_returns_to_parent() {
        let root = std::env::temp_dir().join(format!(
            "elgar-indented-tree-sibling-parent-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("react-vite-project")).unwrap();
        fs::write(
            root.join("react-vite-project/plan.md"),
            "# Project Plan\n\n## File Tree\n- react-vite-project/\n  - package.json\n  - src/\n    - main.jsx\n    - App.jsx\n    - App.css\n  - README.md\n\n## Verification\n- Run `npm run dev`.\n\n## Acceptance Criteria\n- The app runs.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("react-vite-project/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("react-vite-project/plan.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured plan state");
        let project = root.join("react-vite-project");
        assert!(
            structured
                .expected_files
                .contains(&project.join("README.md")),
            "expected files: {:#?}",
            structured.expected_files
        );
        assert!(!structured
            .expected_files
            .contains(&project.join("src/README.md")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_realistic_tree_paths_with_inline_comments() {
        let root =
            std::env::temp_dir().join(format!("elgar-realistic-tree-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("advanced-plan-test")).unwrap();
        fs::write(
            root.join("advanced-plan-test/plan.md"),
            "# Project Plan: Task Tracker CLI\n\n## 1. File Tree\n```text\nadvanced-plan-test/\n├── src/\n│   ├── tasktracker_cli/\n│   │   ├── __init__.py\n│   │   ├── cli.py          # Argument parsing & command dispatch\n│   │   ├── storage.py      # JSON file storage helpers\n│   │   └── tasks.py        # Core task model & business logic\n├── tests/\n│   ├── __init__.py\n│   ├── test_cli.py         # CLI command integration tests\n│   └── test_storage.py     # Storage module unit tests\n├── README.md\n├── pyproject.toml\n└── plan.md                 # This project plan file\n```\n\n## 2. Verification Steps\n- Run `pytest tests/`.\n\n## 3. Acceptance Criteria\n- The CLI supports add, list, complete, and delete.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("advanced-plan-test/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("advanced-plan-test/plan.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified realistic plan should create structured plan state");
        let project = root.join("advanced-plan-test");
        for directory in ["src", "src/tasktracker_cli", "tests"] {
            assert!(
                structured
                    .expected_directories
                    .contains(&project.join(directory)),
                "missing directory {directory}"
            );
        }
        for file in [
            "src/tasktracker_cli/__init__.py",
            "src/tasktracker_cli/cli.py",
            "src/tasktracker_cli/storage.py",
            "src/tasktracker_cli/tasks.py",
            "tests/__init__.py",
            "tests/test_cli.py",
            "tests/test_storage.py",
            "README.md",
            "pyproject.toml",
        ] {
            assert!(
                structured.expected_files.contains(&project.join(file)),
                "missing file {file}"
            );
        }
        assert!(!structured.expected_files.contains(&project.join("plan.md")));
        assert!(!structured
            .expected_files
            .iter()
            .any(|path| path.to_string_lossy().contains('#')));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_react_vite_file_tree_without_json_or_jsx_snippets() {
        let root =
            std::env::temp_dir().join(format!("elgar-react-vite-tree-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("react-vite-plan-test")).unwrap();
        fs::write(
            root.join("react-vite-plan-test/plan.md"),
            "# Project Plan: Task Board React Vite App\n\n## File Tree\n```\nreact-vite-plan-test/\n├── package.json\n├── vite.config.js (optional)\n├── index.html\n├── src/\n│   ├── main.jsx (or main.tsx)\n│   ├── App.jsx (or App.tsx)\n│   ├── components/\n│   │   ├── Column.jsx\n│   │   └── TaskCard.jsx\n│   ├── styles/\n│   │   └── app.css\n│   └── tests/\n│       ├── App.test.jsx\n│       └── TaskBoard.test.jsx\n├── README.md\n└── .gitignore\n```\n\n## package.json\n```json\n{\n  \"scripts\": {\n    \"dev\": \"vite\"\n  },\n  \"dependencies\": {\n    \"react-dom\": \"^18.2.0\"\n  }\n}\n```\n\n## index.html\n```html\n<head></head>\n<body></body>\n```\n\n## src/App.jsx\n```jsx\nreturn (\n  <div className=\"board\"></div>\n);\n```\n\n## Verification Steps\n- Run `npm run dev`.\n\n## Acceptance Criteria\n- The board renders.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("react-vite-plan-test/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("react-vite-plan-test/plan.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified React plan should create structured plan state");
        let project = root.join("react-vite-plan-test");
        for directory in ["src", "src/components", "src/styles", "src/tests"] {
            assert!(
                structured
                    .expected_directories
                    .contains(&project.join(directory)),
                "missing directory {directory}"
            );
        }
        for file in [
            "package.json",
            "vite.config.js",
            "index.html",
            "src/main.jsx",
            "src/App.jsx",
            "src/components/Column.jsx",
            "src/components/TaskCard.jsx",
            "src/styles/app.css",
            "src/tests/App.test.jsx",
            "src/tests/TaskBoard.test.jsx",
            "README.md",
            ".gitignore",
        ] {
            assert!(
                structured.expected_files.contains(&project.join(file)),
                "missing file {file}"
            );
        }
        for junk in [
            "scripts\": {",
            "dependencies\": {",
            "<head>",
            "<body>",
            "return (",
        ] {
            assert!(
                !structured
                    .expected_directories
                    .iter()
                    .chain(structured.expected_files.iter())
                    .any(|path| path.to_string_lossy().contains(junk)),
                "unexpected snippet path {junk}"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_file_tree_without_requirements_contents_as_files() {
        let root = std::env::temp_dir().join(format!(
            "elgar-requirements-content-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ManualMemoryPlan1")).unwrap();
        fs::write(
            root.join("ManualMemoryPlan1/PLAN.md"),
            "# ManualMemoryPlan1 Project Plan\n\n## File Tree\n```\nManualMemoryPlan1/\n├── README.md\n├── src/\n│   └── main.py\n├── requirements.txt\n└── PLAN.md  (this file)\n```\n\n## requirements.txt Content\n```text\nclick==8.1.7\npathlib2==2.3.6  # for older Python compatibility, optional\n```\n\n## Verification Plan\n1. **Manual Check** - Install dependencies and run the CLI.\n\n## Acceptance Criteria\n- `README.md`, `src/main.py`, and `requirements.txt` exist.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("ManualMemoryPlan1/PLAN.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("ManualMemoryPlan1/PLAN.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured state");
        let project = root.join("ManualMemoryPlan1");
        assert!(structured
            .expected_directories
            .contains(&project.join("src")));
        assert!(structured
            .expected_files
            .contains(&project.join("README.md")));
        assert!(structured
            .expected_files
            .contains(&project.join("src/main.py")));
        assert!(structured
            .expected_files
            .contains(&project.join("requirements.txt")));
        assert!(!structured.expected_files.contains(&project.join("PLAN.md")));
        for dependency in ["click==8.1.7", "pathlib2==2.3.6"] {
            assert!(
                !structured
                    .expected_files
                    .iter()
                    .chain(structured.expected_directories.iter())
                    .any(|path| path.to_string_lossy().contains(dependency)),
                "dependency line was extracted as a path: {dependency}"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_inline_prose_path_subjects_after_file_tree() {
        let root = std::env::temp_dir().join(format!(
            "elgar-inline-prose-path-subjects-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("TodoPlan")).unwrap();
        fs::write(
            root.join("TodoPlan/PLAN.md"),
            "# Project Plan: Tiny Python CLI Todo App\n\n## File Tree\n```\nTodoPlan/\n├── README.md\n├── src/\n│   └── main.py\n└── requirements.txt\n```\n\n## README.md\nThe `README.md` will provide setup instructions.\n\n## src/main.py\n`main.py` will implement a minimal CLI todo application.\n\n## requirements.txt\nDependencies:\n- `click` (for CLI handling)\n\n## Verification\n- `python -m src.main --help` should display help.\n\n## Acceptance Criteria\n- The application must be runnable with `python -m src.main`.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("TodoPlan/PLAN.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("TodoPlan/PLAN.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured state");
        let project = root.join("TodoPlan");
        assert!(structured
            .expected_files
            .contains(&project.join("README.md")));
        assert!(structured
            .expected_files
            .contains(&project.join("src/main.py")));
        assert!(structured
            .expected_files
            .contains(&project.join("requirements.txt")));
        assert!(!structured.expected_files.contains(&project.join("main.py")));
        assert!(!structured.expected_files.contains(&project.join("click")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_todo_tree_label_and_keeps_child_paths_under_root() {
        let root =
            std::env::temp_dir().join(format!("elgar-todo-tree-label-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("TodoPlan")).unwrap();
        fs::write(
            root.join("TodoPlan/plan.md"),
            "# Project Plan\n\n## Directory Structure\n```\nTodoPlan/\n├── plan.md\n└── TODO: implementation files will be added here:\n    ├── README.md\n    ├── src/main.py\n    └── requirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("TodoPlan/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("TodoPlan/plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured state");
        let project = root.join("TodoPlan");
        assert!(structured
            .expected_files
            .contains(&project.join("README.md")));
        assert!(structured
            .expected_files
            .contains(&project.join("src/main.py")));
        assert!(structured
            .expected_files
            .contains(&project.join("requirements.txt")));
        assert!(!structured
            .expected_directories
            .iter()
            .chain(structured.expected_files.iter())
            .any(|path| path.to_string_lossy().contains("TODO:")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_cwd_relative_project_root_tree_without_duplication() {
        let root = std::env::temp_dir().join(format!(
            "elgar-cwd-relative-project-root-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("playground/ManualMemoryPlanSmokeFix")).unwrap();
        fs::write(
            root.join("playground/ManualMemoryPlanSmokeFix/plan.md"),
            "# Project Plan\n\n## File Tree\n```\nplayground/ManualMemoryPlanSmokeFix/\n├── README.md\n├── src/\n│   └── main.py\n├── requirements.txt\n└── plan.md\n```\n\n## Verification\n- Check files.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("playground/ManualMemoryPlanSmokeFix/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("playground/ManualMemoryPlanSmokeFix/plan.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified plan should create structured state");
        let project = root.join("playground/ManualMemoryPlanSmokeFix");
        assert!(structured
            .expected_directories
            .contains(&project.join("src")));
        for file in ["README.md", "src/main.py", "requirements.txt"] {
            assert!(
                structured.expected_files.contains(&project.join(file)),
                "missing expected file {file}; got {:#?}",
                structured.expected_files
            );
        }
        assert!(!structured.expected_files.contains(&project.join("plan.md")));
        assert!(!structured
            .expected_files
            .iter()
            .chain(structured.expected_directories.iter())
            .any(|path| path
                .to_string_lossy()
                .contains("ManualMemoryPlanSmokeFix/playground/ManualMemoryPlanSmokeFix")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_single_dash_unicode_tree_under_expected_parents() {
        let root = std::env::temp_dir().join(format!(
            "elgar-single-dash-react-tree-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("react-vite-plan-test-manual")).unwrap();
        fs::write(
            root.join("react-vite-plan-test-manual/plan.md"),
            "# Project Plan\n\n## File Tree\n```\nreact-vite-plan-test-manual/\n├─ package.json\n├─ src/\n│  ├─ main.jsx\n│  ├─ App.jsx\n│  ├─ components/\n│  │  ├─ TaskBoard.jsx\n│  │  ├─ Column.jsx\n│  │  └─ TaskItem.jsx\n│  ├─ styles/\n│  │  └─ app.css\n│  └─ tests/\n│     ├─ App.test.jsx\n│     └─ setupTests.js\n└─ README.md\n```\n\n## Verification Approach\n1. Run `npm run dev`.\n\n## Acceptance Criteria\n- The board renders.\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("react-vite-plan-test-manual/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root
                .join("react-vite-plan-test-manual/plan.md")
                .display()
                .to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified React plan should create structured plan state");
        let project = root.join("react-vite-plan-test-manual");
        for directory in ["src", "src/components", "src/styles", "src/tests"] {
            assert!(
                structured
                    .expected_directories
                    .contains(&project.join(directory)),
                "missing directory {directory}"
            );
        }
        for file in [
            "package.json",
            "src/main.jsx",
            "src/App.jsx",
            "src/components/TaskBoard.jsx",
            "src/components/Column.jsx",
            "src/components/TaskItem.jsx",
            "src/styles/app.css",
            "src/tests/App.test.jsx",
            "src/tests/setupTests.js",
            "README.md",
        ] {
            assert!(
                structured.expected_files.contains(&project.join(file)),
                "missing file {file}"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_parent_directories_for_expected_file_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-parent-dir-structured-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("LivePlan")).unwrap();
        fs::write(
            root.join("LivePlan/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\ntests/\n```\n",
        )
        .unwrap();
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-plan",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("LivePlan/plan.md"),
                contents: "# Project Plan\n".to_string(),
            }),
            "create plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: root.join("LivePlan/plan.md").display().to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let structured = session
            .project_memory()
            .latest_structured_plan()
            .expect("verified live plan should create structured plan state");
        assert!(structured
            .expected_directories
            .contains(&root.join("LivePlan/src")));
        assert!(structured
            .expected_directories
            .contains(&root.join("LivePlan/tests")));
        assert!(!structured
            .expected_directories
            .contains(&root.join("LivePlan")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/src/main.py")));
        assert!(structured
            .expected_files
            .contains(&root.join("LivePlan/requirements.txt")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_record_arbitrary_txt_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/notes.txt"),
                contents: "notes".to_string(),
            }),
            "create notes",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/notes.txt".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert!(session.project_memory().latest_verified_plan().is_none());
    }

    #[test]
    fn does_not_record_readme_markdown_as_verified_plan_memory() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/README.md"),
                contents: "# Demo".to_string(),
            }),
            "create readme",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/README.md".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        assert!(session.project_memory().latest_verified_plan().is_none());
    }

    #[test]
    fn records_readme_markdown_when_contents_are_a_plan() {
        let root = PathBuf::from("/repo");
        let mut session = Session::new("session", &root, &root);
        let action = Action::proposed(
            "action-1",
            ActionRequest::CreateFile(CreateFileAction {
                target_path: PathBuf::from("DesktopProject/README.md"),
                contents: "# React TypeScript Project Plan\n\n- Create package.json.".to_string(),
            }),
            "create readme plan",
        )
        .approve()
        .mark_applied();
        let result = VerifiedActionResult::File(FileActionVerification::FileCreated {
            path: "DesktopProject/README.md".to_string(),
        });

        record_verified_project_memory(&mut session, &action, &result);

        let plan = session
            .project_memory()
            .latest_verified_plan()
            .expect("README.md plan contents should be remembered");
        assert_eq!(plan.path, PathBuf::from("/repo/DesktopProject/README.md"));
    }
}
