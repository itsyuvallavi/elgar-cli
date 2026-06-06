//! Footer label helpers for the inline terminal.
//!
//! These helpers keep cwd/project/model labels compact enough for narrow
//! terminals.

use std::path::Path;

pub(crate) fn footer_location_label(project_root: &Path, cwd: &Path) -> String {
    let project = compact_path_label(project_root);
    if cwd == project_root {
        return project;
    }

    if let Ok(relative) = cwd.strip_prefix(project_root) {
        let relative = relative.display().to_string();
        if relative.is_empty() {
            project
        } else {
            format!("{project}/{relative}")
        }
    } else {
        compact_path_label(cwd)
    }
}

pub(crate) fn align_footer_line(left: &str, right: &str, width: usize) -> String {
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

fn compact_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}
