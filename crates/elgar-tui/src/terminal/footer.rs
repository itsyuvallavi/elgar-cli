use std::path::{Path, PathBuf};

pub(super) fn footer_location_label(project_root: &Path, cwd: &Path) -> String {
    let mut parts = vec![project_footer_label(project_root)];
    if cwd != project_root {
        parts.push(compact_cwd_label(project_root, cwd));
    }
    if let Some(branch) = current_git_branch(project_root) {
        parts.push(format!("({branch})"));
    }
    parts.join(" ")
}

pub(super) fn align_footer_line(left: &str, right: &str, width: usize) -> String {
    let left_width = left.chars().count();
    let right_width = right.chars().count();
    let minimum_gap = 2;
    if width > left_width + right_width + minimum_gap {
        format!(
            "{left}{:gap$}{right}",
            "",
            gap = width - left_width - right_width
        )
    } else {
        format!("{left}  {right}")
    }
}

fn current_git_branch(project_root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(project_root.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return non_empty_label(branch);
    }
    None
}

fn non_empty_label(label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

fn compact_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn project_footer_label(project_root: &Path) -> String {
    if let Some(home_label) = home_relative_label(project_root) {
        return home_label;
    }
    compact_repo_label(project_root)
}

fn home_relative_label(path: &Path) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    let relative = path.strip_prefix(home).ok()?;
    let label = relative.display().to_string();
    if label.is_empty() {
        Some("~".to_string())
    } else {
        Some(format!("~/{}", label))
    }
}

fn compact_repo_label(project_root: &Path) -> String {
    let repo = compact_path_label(project_root);
    let Some(parent) = project_root.parent() else {
        return repo;
    };
    let Some(parent_name) = parent.file_name().and_then(|name| name.to_str()) else {
        return repo;
    };
    if parent_name.is_empty() {
        repo
    } else {
        format!("{parent_name}/{repo}")
    }
}

fn compact_cwd_label(project_root: &Path, cwd: &Path) -> String {
    if cwd == project_root {
        ".".to_string()
    } else if let Ok(relative) = cwd.strip_prefix(project_root) {
        let label = relative.display().to_string();
        if label.is_empty() {
            ".".to_string()
        } else {
            label
        }
    } else {
        compact_path_label(cwd)
    }
}
