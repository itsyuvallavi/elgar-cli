//! Direct display rendering for explicit read and list requests.
//!
//! Direct file and directory display requests should not go through final-answer
//! synthesis: the harness already has verified evidence, so this module renders
//! that evidence directly for the user.

use crate::harness::harness_loop::state::types::Evidence;

const FILE_CONTENT_START: &str = "\n\n```text\n";
const FILE_CONTENT_END: &str = "\n```";
const DIRECTORY_ENTRIES_START: &str = "\n\nEntries:\n";
const DIRECTORY_OMITTED_START: &str = "\nOmitted:\n";

pub(super) fn direct_display_text(evidence: &[Evidence]) -> Option<String> {
    direct_read_display_text(evidence).or_else(|| direct_ls_display_text(evidence))
}

fn direct_read_display_text(evidence: &[Evidence]) -> Option<String> {
    let item = evidence.last()?;
    let path = item.label.strip_prefix("read:")?;
    let contents = verified_file_contents(&item.body)?;
    let mut rendered = format!("`{path}`\n\n```text\n{contents}\n```");
    if item.truncated {
        rendered.push_str("\n\n[truncated: file exceeded read limit]");
    }
    Some(rendered)
}

fn direct_ls_display_text(evidence: &[Evidence]) -> Option<String> {
    let item = evidence.last()?;
    let path = item.label.strip_prefix("ls:")?;
    let entries = verified_directory_entries(&item.body)?;
    let mut rendered = format!("`{path}`\n\n```text\n{entries}```");
    if item.truncated {
        rendered.push_str("\n\n[truncated: directory listing exceeded limit]");
    }
    Some(rendered)
}

fn verified_file_contents(body: &str) -> Option<&str> {
    let start = body.find(FILE_CONTENT_START)? + FILE_CONTENT_START.len();
    let rest = &body[start..];
    let end = rest.rfind(FILE_CONTENT_END)?;
    Some(&rest[..end])
}

fn verified_directory_entries(body: &str) -> Option<&str> {
    let start = body.find(DIRECTORY_ENTRIES_START)? + DIRECTORY_ENTRIES_START.len();
    let rest = &body[start..];
    let end = rest.find(DIRECTORY_OMITTED_START).unwrap_or(rest.len());
    let entries = &rest[..end];
    if entries.trim().is_empty() {
        return Some("(empty)\n");
    }
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_evidence(body: &str, truncated: bool) -> Evidence {
        Evidence {
            label: "read:hello-world.md".to_string(),
            body: body.to_string(),
            bytes: body.len(),
            truncated,
        }
    }

    fn ls_evidence(body: &str, truncated: bool) -> Evidence {
        Evidence {
            label: "ls:app".to_string(),
            body: body.to_string(),
            bytes: body.len(),
            truncated,
        }
    }

    #[test]
    fn renders_verified_file_contents_without_summary() {
        let evidence = vec![read_evidence(
            "Read-only project file selected by Elgar harness.\nRoot: /tmp\nPath: hello-world.md\nBytes: 12\nRendered bytes: 12\nTruncated: false\n\n```text\n# Hello\nbody\n```",
            false,
        )];

        let rendered = direct_display_text(&evidence).expect("direct display");

        assert!(rendered.starts_with("`hello-world.md`"));
        assert!(rendered.contains("# Hello\nbody"));
        assert!(!rendered.contains("Summary"));
    }

    #[test]
    fn marks_truncated_file_contents() {
        let evidence = vec![read_evidence(
            "Read-only project file selected by Elgar harness.\n\n```text\npartial\n```",
            true,
        )];

        let rendered = direct_display_text(&evidence).expect("direct display");

        assert!(rendered.contains("[truncated: file exceeded read limit]"));
    }

    #[test]
    fn renders_verified_directory_entries_without_summary() {
        let evidence = vec![ls_evidence(
            "Read-only directory summary selected by Elgar harness.\nRoot: /tmp\nPath: app\nFiles counted: 3\nDirectories counted: 0\nTotal bytes: 128\nCount truncated: false\nNote: file contents were not read.\n\nEntries:\n[file] globals.css\n[file] layout.tsx\n[file] page.tsx\n",
            false,
        )];

        let rendered = direct_display_text(&evidence).expect("direct display");

        assert!(rendered.starts_with("`app`"));
        assert!(rendered.contains("[file] globals.css"));
        assert!(rendered.contains("[file] layout.tsx"));
        assert!(rendered.contains("[file] page.tsx"));
        assert!(!rendered.contains("Summary"));
        assert!(!rendered.contains("Evidence Used"));
    }

    #[test]
    fn marks_truncated_directory_entries() {
        let evidence = vec![ls_evidence(
            "Read-only directory summary selected by Elgar harness.\n\nEntries:\n[file] page.tsx\n\n[truncated: directory entries exceeded entry or depth limits]\n",
            true,
        )];

        let rendered = direct_display_text(&evidence).expect("direct display");

        assert!(rendered.contains("[truncated: directory listing exceeded limit]"));
    }

    #[test]
    fn ignores_non_read_evidence() {
        let body = "Find matches selected by Elgar harness.";
        let evidence = vec![Evidence {
            label: "find:.:README*".to_string(),
            body: body.to_string(),
            bytes: body.len(),
            truncated: false,
        }];

        assert!(direct_display_text(&evidence).is_none());
    }
}
