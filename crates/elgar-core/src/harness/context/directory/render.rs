//! Directory snapshot rendering.

use super::types::DirectorySnapshot;

impl DirectorySnapshot {
    /// Render bounded directory evidence for the model.
    pub fn render_for_model(&self) -> String {
        let mut rendered = format!(
            "Read-only directory summary selected by Elgar harness.\nRoot: {}\nPath: {}\nFiles counted: {}\nDirectories counted: {}\nTotal bytes: {}\nCount truncated: {}\nNote: file contents were not read.\n\nEntries:\n",
            self.root.display(),
            self.display_path,
            self.total_files,
            self.total_directories,
            self.total_bytes,
            self.count_truncated
        );

        for entry in &self.entries {
            rendered.push_str(&"  ".repeat(entry.depth));
            rendered.push_str(entry.kind.prefix());
            rendered.push_str(&entry.display_path);
            rendered.push('\n');

            if rendered.len() >= self.max_rendered_bytes {
                rendered.push_str("\n[truncated: rendered directory exceeded byte limit]\n");
                return rendered;
            }
        }

        if !self.omitted.is_empty() {
            rendered.push_str("\nOmitted:\n");
            for omission in &self.omitted {
                rendered.push_str("- ");
                rendered.push_str(&omission.display_path);
                rendered.push_str(": ");
                rendered.push_str(&omission.reason);
                rendered.push('\n');
            }
        }

        if self.truncated {
            rendered.push_str("\n[truncated: directory entries exceeded entry or depth limits]\n");
        }

        rendered
    }
}
