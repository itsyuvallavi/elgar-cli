use std::path::{Path, PathBuf};

pub(crate) fn shell_quote_path(path: &Path) -> String {
    let path = path.as_os_str().to_string_lossy();
    format!("'{}'", path.replace('\'', "'\\''"))
}

pub(crate) fn shell_write_file_command(target_path: &Path, contents: &str) -> String {
    let delimiter = unique_heredoc_delimiter(contents);
    let mut body = contents.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let write_command = format!(
        "cat > {} <<'{}'\n{}{}",
        shell_quote_path(target_path),
        delimiter,
        body,
        delimiter
    );

    match target_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("mkdir -p {} && {write_command}", shell_quote_path(parent))
        }
        _ => write_command,
    }
}

pub(crate) fn shell_write_many_files_command(
    directories: &[PathBuf],
    files: &[(PathBuf, String)],
) -> String {
    let mut lines = vec!["set -e".to_string()];
    let mut mkdir_paths = directories.to_vec();
    mkdir_paths.extend(
        files
            .iter()
            .filter_map(|(path, _contents)| path.parent().map(Path::to_path_buf)),
    );
    let mkdir_paths = dedupe_paths(mkdir_paths);
    if !mkdir_paths.is_empty() {
        lines.push(format!(
            "mkdir -p {}",
            mkdir_paths
                .iter()
                .map(|path| shell_quote_path(path))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }

    for (path, contents) in files {
        let delimiter = unique_heredoc_delimiter(contents);
        let mut body = contents.clone();
        if !body.ends_with('\n') {
            body.push('\n');
        }
        lines.push(format!(
            "cat > {} <<'{}'\n{}{}",
            shell_quote_path(path),
            delimiter,
            body,
            delimiter
        ));
    }

    lines.join("\n")
}

fn unique_heredoc_delimiter(contents: &str) -> String {
    let base = "ELGAR_MARKDOWN_PLAN_EOF";
    if !contents.lines().any(|line| line == base) {
        return base.to_string();
    }

    (1..)
        .map(|index| format!("{base}_{index}"))
        .find(|candidate| !contents.lines().any(|line| line == candidate))
        .expect("unbounded delimiter search should find a unique value")
}

pub(crate) fn display_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_path_escapes_apostrophes() {
        assert_eq!(
            shell_quote_path(Path::new("/tmp/it's here/plan.md")),
            "'/tmp/it'\\''s here/plan.md'"
        );
    }

    #[test]
    fn unique_heredoc_delimiter_skips_existing_delimiters() {
        let contents = "body\nELGAR_MARKDOWN_PLAN_EOF\nELGAR_MARKDOWN_PLAN_EOF_1\n";

        assert_eq!(
            unique_heredoc_delimiter(contents),
            "ELGAR_MARKDOWN_PLAN_EOF_2"
        );
    }

    #[test]
    fn shell_write_file_command_creates_parent_directory() {
        let command = shell_write_file_command(Path::new("/tmp/elgar/plan.md"), "hello");

        assert!(command.starts_with("mkdir -p '/tmp/elgar' && cat > '/tmp/elgar/plan.md'"));
    }

    #[test]
    fn shell_write_many_files_command_dedupes_mkdir_paths_preserving_order() {
        let command = shell_write_many_files_command(
            &[
                PathBuf::from("/tmp/elgar"),
                PathBuf::from("/tmp/elgar"),
                PathBuf::from("/tmp/elgar/src"),
            ],
            &[
                (
                    PathBuf::from("/tmp/elgar/src/main.rs"),
                    "fn main() {}".to_string(),
                ),
                (PathBuf::from("/tmp/elgar/README.md"), "# App".to_string()),
            ],
        );

        assert!(command.starts_with("set -e\nmkdir -p '/tmp/elgar' '/tmp/elgar/src'"));
    }

    #[test]
    fn dedupe_paths_preserves_first_seen_order() {
        let paths = dedupe_paths(vec![
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("a"),
            PathBuf::from("c"),
            PathBuf::from("b"),
        ]);

        assert_eq!(
            paths,
            vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]
        );
    }
}
