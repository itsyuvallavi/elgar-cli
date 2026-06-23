//! Write outcome detection for harness file mutations.
//!
//! This module inspects the target before a write so execution logs can say
//! whether the write created, updated, or left a file unchanged.

use std::{fs, path::Path};

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness) struct WriteOutcome {
    existed_before: bool,
    content_changed: Option<bool>,
    write_outcome: &'static str,
    existing_read_error: Option<String>,
}

impl WriteOutcome {
    pub(in crate::harness) fn inspect(target: &Path, content: &str) -> Self {
        if !target.exists() {
            return Self {
                existed_before: false,
                content_changed: Some(true),
                write_outcome: "created",
                existing_read_error: None,
            };
        }

        match fs::read(target) {
            Ok(existing) => {
                let changed = existing != content.as_bytes();
                Self {
                    existed_before: true,
                    content_changed: Some(changed),
                    write_outcome: if changed { "updated" } else { "unchanged" },
                    existing_read_error: None,
                }
            }
            Err(error) => Self {
                existed_before: true,
                content_changed: None,
                write_outcome: "updated",
                existing_read_error: Some(error.to_string()),
            },
        }
    }

    pub(in crate::harness) fn metadata(&self) -> Value {
        let mut metadata = json!({
            "existed_before": self.existed_before,
            "content_changed": self.content_changed,
            "write_outcome": self.write_outcome,
        });
        if let Some(error) = self.existing_read_error.as_ref() {
            metadata["existing_read_error"] = json!(error);
        }
        metadata
    }

    pub(in crate::harness) fn raw_lines(&self) -> String {
        let changed = self
            .content_changed
            .map(|changed| changed.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut lines = format!(
            "existed_before: {}\ncontent_changed: {}\nwrite_outcome: {}\n",
            self.existed_before, changed, self.write_outcome
        );
        if let Some(error) = self.existing_read_error.as_ref() {
            lines.push_str(&format!("existing_read_error: {error}\n"));
        }
        lines
    }
}
