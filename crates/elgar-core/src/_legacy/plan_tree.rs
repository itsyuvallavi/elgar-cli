use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedPathKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedPathTreeEntry {
    path: PathBuf,
    kind: ExpectedPathKind,
    status: String,
}

impl ExpectedPathTreeEntry {
    pub fn directory(path: impl Into<PathBuf>, status: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: ExpectedPathKind::Directory,
            status: status.into(),
        }
    }

    pub fn file(path: impl Into<PathBuf>, status: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: ExpectedPathKind::File,
            status: status.into(),
        }
    }
}

#[derive(Debug, Default)]
struct ExpectedPathTree {
    directory_status: Option<String>,
    directories: BTreeMap<String, ExpectedPathTree>,
    files: BTreeMap<String, String>,
}

impl ExpectedPathTree {
    fn insert_directory(&mut self, components: &[String], status: &str) {
        if components.is_empty() {
            self.directory_status = Some(status.to_string());
            return;
        }
        let child = self.directories.entry(components[0].clone()).or_default();
        child.insert_directory(&components[1..], status);
    }

    fn insert_file(&mut self, components: &[String], status: &str) {
        let Some((file_name, parents)) = components.split_last() else {
            return;
        };
        let mut node = self;
        for parent in parents {
            node = node.directories.entry(parent.clone()).or_default();
        }
        node.files.insert(file_name.clone(), status.to_string());
    }

    fn render_children(&self, indent: usize, lines: &mut Vec<String>) {
        let prefix = "  ".repeat(indent);
        for (name, child) in &self.directories {
            let status = child
                .directory_status
                .as_ref()
                .map(|status| format!("[{status}] "))
                .unwrap_or_default();
            lines.push(format!("{prefix}{status}{name}/"));
            child.render_children(indent + 1, lines);
        }
        for (name, status) in &self.files {
            lines.push(format!("{prefix}[{status}] {name}"));
        }
    }
}

pub fn render_expected_path_tree(root: &Path, entries: &[ExpectedPathTreeEntry]) -> Vec<String> {
    let mut tree = ExpectedPathTree::default();
    for entry in entries {
        let components = relative_path_components(root, &entry.path);
        match entry.kind {
            ExpectedPathKind::Directory => tree.insert_directory(&components, &entry.status),
            ExpectedPathKind::File => tree.insert_file(&components, &entry.status),
        }
    }

    let mut lines = Vec::new();
    if let Some(status) = &tree.directory_status {
        lines.push(format!("[{status}] ./"));
    }
    tree.render_children(0, &mut lines);
    lines
}

fn relative_path_components(root: &Path, path: &Path) -> Vec<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .filter_map(|component| {
            let text = component.as_os_str().to_string_lossy();
            if text.is_empty() {
                None
            } else {
                Some(text.into_owned())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_grouped_expected_paths() {
        let root = Path::new("/repo/app");
        let entries = vec![
            ExpectedPathTreeEntry::directory("/repo/app/src", "missing"),
            ExpectedPathTreeEntry::directory("/repo/app/tests", "ok"),
            ExpectedPathTreeEntry::file("/repo/app/src/main.py", "missing"),
            ExpectedPathTreeEntry::file("/repo/app/tests/test_main.py", "ok"),
            ExpectedPathTreeEntry::file("/repo/app/README.md", "ok"),
        ];

        assert_eq!(
            render_expected_path_tree(root, &entries),
            vec![
                "[missing] src/",
                "  [missing] main.py",
                "[ok] tests/",
                "  [ok] test_main.py",
                "[ok] README.md",
            ]
        );
    }
}
