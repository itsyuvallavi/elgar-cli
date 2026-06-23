//! Currently executable read-only evidence collection for primitive harness tools.
//!
//! Context modules collect bounded local facts for primitive tools such as
//! `read`, `ls`, `find`, and `grep`. They do not decide when the model should
//! request those facts, and they do not execute side effects.

mod directory;
mod find;
mod grep;
mod noise;
mod path;
mod project_file;

pub use directory::{
    collect_directory_summary, DirectoryEntry, DirectoryEntryKind, DirectoryError,
    DirectoryOmission, DirectoryOptions, DirectorySnapshot,
};
pub use find::{collect_find_matches, FindError, FindOptions, FindSnapshot};
pub use grep::{collect_grep_matches, GrepError, GrepMatch, GrepOptions, GrepSnapshot};
pub use project_file::{
    collect_project_file, ProjectFileError, ProjectFileOptions, ProjectFileSnapshot,
};
