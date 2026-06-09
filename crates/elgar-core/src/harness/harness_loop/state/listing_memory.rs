//! Compact memory extracted from verified directory listings.
//!
//! The harness stores only small, verified hints from `ls` results so later
//! model decisions can avoid repeating the same directory listing.

use crate::harness::{DirectoryEntryKind, DirectorySnapshot};

const MAX_LISTING_DIRS: usize = 8;
const MAX_LISTING_FILES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct DirectoryListingMemory {
    pub path: String,
    pub dirs: Vec<String>,
    pub files: Vec<String>,
    pub omitted_dirs: usize,
    pub omitted_files: usize,
    pub truncated: bool,
}

impl DirectoryListingMemory {
    /// Build compact same-turn memory from verified `ls` evidence.
    pub fn from_snapshot(path: String, snapshot: &DirectorySnapshot) -> Self {
        let mut all_dirs = Vec::new();
        let mut all_files = Vec::new();

        for entry in &snapshot.entries {
            if entry.depth != 0 {
                continue;
            }

            match entry.kind {
                DirectoryEntryKind::Directory => all_dirs.push(entry.display_path.clone()),
                DirectoryEntryKind::File => all_files.push(entry.display_path.clone()),
            }
        }

        let omitted_dirs = all_dirs.len().saturating_sub(MAX_LISTING_DIRS);
        let omitted_files = all_files.len().saturating_sub(MAX_LISTING_FILES);

        Self {
            path,
            dirs: all_dirs.into_iter().take(MAX_LISTING_DIRS).collect(),
            files: all_files.into_iter().take(MAX_LISTING_FILES).collect(),
            omitted_dirs,
            omitted_files,
            truncated: snapshot.truncated || snapshot.count_truncated,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.files.is_empty()
    }

    pub fn render_for_prompt(&self) -> String {
        let mut line = format!("- listing memory for `{}`:", self.path);
        if self.dirs.is_empty() {
            line.push_str(" dirs: (none)");
        } else {
            line.push_str(" dirs: ");
            line.push_str(&quoted_values(&self.dirs));
            if self.omitted_dirs > 0 {
                line.push_str(&format!(" (+{} more)", self.omitted_dirs));
            }
        }

        if self.files.is_empty() {
            line.push_str("; files: (none)");
        } else {
            line.push_str("; files: ");
            line.push_str(&quoted_values(&self.files));
            if self.omitted_files > 0 {
                line.push_str(&format!(" (+{} more)", self.omitted_files));
            }
        }

        if self.truncated {
            line.push_str("; listing was truncated");
        }
        line
    }

    pub fn render_duplicate_hint(&self) -> String {
        format!(
            "- Existing listing for `{}` is already available. Use one visible child path, read a visible file, grep/find, or answer_now. {}",
            self.path,
            self.render_for_prompt().trim_start_matches("- ")
        )
    }
}

fn quoted_values(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
