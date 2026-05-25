use std::path::{Path, PathBuf};

use elgar_core::event::{FileActionVerification, VerifiedActionResult};

use super::{user_display_path, ConversationLineStyle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateWriteToolBatch {
    pub(super) line_index: usize,
    items: Vec<CreateWriteToolItem>,
}

impl CreateWriteToolBatch {
    pub(super) fn new(line_index: usize, item: CreateWriteToolItem) -> Self {
        Self {
            line_index,
            items: vec![item],
        }
    }

    pub(super) fn push(&mut self, item: CreateWriteToolItem) {
        self.items.push(item);
    }

    pub(super) fn render(&self) -> String {
        if self.items.len() == 1 {
            return self.items[0].render_single();
        }

        let file_count = self.items.iter().filter(|item| item.is_file()).count();
        let directory_count = self.items.len().saturating_sub(file_count);
        let root = project_create_batch_root(&self.items)
            .map(user_display_path)
            .unwrap_or_else(|| "the requested location".to_string());
        let outside_project = project_outside_counts(&self.items);
        let mut parts = Vec::new();

        if directory_count > 0 {
            parts.push(pluralize_count(directory_count, "folder", "folders"));
        }
        if file_count > 0 {
            parts.push(pluralize_count(file_count, "file", "files"));
        }

        let mut rendered = format!(
            "Tool result\nCreated project: {root}\nVerified: {}",
            parts.join(", ")
        );

        if let Some(outside_project) = outside_project {
            rendered.push_str("\nOutside project: ");
            rendered.push_str(&outside_project);
        }

        rendered
    }

    pub(super) fn line_style(&self) -> ConversationLineStyle {
        if self.items.len() == 1 {
            ConversationLineStyle::Plain
        } else {
            ConversationLineStyle::Tool
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CreateWriteToolItem {
    File(String),
    Directory(String),
}

impl CreateWriteToolItem {
    fn render_single(&self) -> String {
        match self {
            Self::File(path) => format!("Wrote {}.", user_display_path(path)),
            Self::Directory(path) => format!("Created {}.", user_display_path(path)),
        }
    }

    fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }
}

pub(super) fn create_write_tool_item(result: &VerifiedActionResult) -> Option<CreateWriteToolItem> {
    match result {
        VerifiedActionResult::FileWritten { path } => Some(CreateWriteToolItem::File(path.clone())),
        VerifiedActionResult::File(FileActionVerification::FileCreated { path }) => {
            Some(CreateWriteToolItem::File(path.clone()))
        }
        VerifiedActionResult::File(FileActionVerification::DirectoryCreated { path }) => {
            Some(CreateWriteToolItem::Directory(path.clone()))
        }
        VerifiedActionResult::File(
            FileActionVerification::FilePatched { .. }
            | FileActionVerification::FileOverwritten { .. }
            | FileActionVerification::FileDeleted { .. }
            | FileActionVerification::FileMoved { .. },
        )
        | VerifiedActionResult::Shell(_) => None,
    }
}

fn project_create_batch_root(items: &[CreateWriteToolItem]) -> Option<PathBuf> {
    let common_root = project_create_common_root(items)?;
    for item in items {
        let CreateWriteToolItem::Directory(path) = item else {
            continue;
        };
        let directory = Path::new(path);
        if directory == common_root || !is_generic_project_child_directory(directory) {
            return Some(directory.to_path_buf());
        }
    }

    Some(common_root)
}

fn project_create_common_root(items: &[CreateWriteToolItem]) -> Option<PathBuf> {
    let first = items.first()?;
    let mut root = batch_item_project_anchor(first)?;

    for item in items.iter().skip(1) {
        let path = batch_item_project_anchor(item)?;
        while !path.starts_with(&root) {
            if !root.pop() {
                return None;
            }
        }
    }

    if root.as_os_str().is_empty() {
        None
    } else {
        Some(root)
    }
}

fn is_generic_project_child_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "app" | "components" | "pages" | "public" | "src" | "styles"
    )
}

fn batch_item_project_anchor(item: &CreateWriteToolItem) -> Option<PathBuf> {
    let path = batch_item_path(item)?;
    let mut anchor = path.to_path_buf();
    if item.is_file() {
        anchor.pop();
    }
    non_empty_path(&anchor).map(Path::to_path_buf)
}

fn project_outside_counts(items: &[CreateWriteToolItem]) -> Option<String> {
    let root = project_create_batch_root(items)?;
    let mut file_count = 0;
    let mut directory_count = 0;

    for item in items {
        let Some(path) = batch_item_path(item) else {
            continue;
        };

        if path == root || path.starts_with(&root) {
            continue;
        }

        if item.is_file() {
            file_count += 1;
        } else {
            directory_count += 1;
        }
    }

    let mut parts = Vec::new();
    if directory_count > 0 {
        parts.push(pluralize_count(directory_count, "folder", "folders"));
    }
    if file_count > 0 {
        parts.push(pluralize_count(file_count, "file", "files"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn batch_item_path(item: &CreateWriteToolItem) -> Option<&Path> {
    let path = match item {
        CreateWriteToolItem::File(path) | CreateWriteToolItem::Directory(path) => Path::new(path),
    };

    non_empty_path(path)
}

fn non_empty_path(path: &Path) -> Option<&Path> {
    (!path.as_os_str().is_empty()).then_some(path)
}

fn pluralize_count(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}
