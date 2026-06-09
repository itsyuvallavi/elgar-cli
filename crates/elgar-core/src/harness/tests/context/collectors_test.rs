//! Read-only harness context collector tests.

use std::fs;

use crate::harness::{
    collect_directory_summary, collect_find_matches, collect_grep_matches, collect_project_file,
    DirectoryOptions, FindOptions, GrepOptions, ProjectFileOptions,
};

#[test]
fn collect_read_primitive_reads_bounded_relative_text_file() {
    let root = std::env::temp_dir().join(format!("elgar-project-file-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"demo"}"#).unwrap();

    let snapshot =
        collect_project_file(&root, "package.json", ProjectFileOptions { max_bytes: 100 }).unwrap();

    assert_eq!(snapshot.display_path, "package.json");
    assert_eq!(snapshot.contents, r#"{"name":"demo"}"#);
    assert!(!snapshot.truncated);
}

#[test]
fn collect_read_primitive_allows_absolute_text_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-project-file-absolute-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("absolute.txt");
    fs::write(&file, "absolute read").unwrap();

    let snapshot = collect_project_file(
        &root,
        &file.to_string_lossy(),
        ProjectFileOptions::default(),
    )
    .unwrap();

    assert_eq!(snapshot.display_path, file.to_string_lossy());
    assert_eq!(snapshot.contents, "absolute read");
}

#[test]
fn collect_read_primitive_truncates_large_text_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-project-file-truncate-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("large.txt"), "abcdef").unwrap();

    let snapshot =
        collect_project_file(&root, "large.txt", ProjectFileOptions { max_bytes: 3 }).unwrap();

    assert_eq!(snapshot.contents, "abc");
    assert_eq!(snapshot.rendered_bytes, 3);
    assert!(snapshot.truncated);
}

#[test]
fn collect_read_primitive_rejects_directory_path() {
    let root = std::env::temp_dir().join(format!(
        "elgar-project-file-directory-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("folder")).unwrap();

    let error = collect_project_file(&root, "folder", ProjectFileOptions::default()).unwrap_err();

    assert!(error.to_string().contains("directories cannot be read"));
}

#[test]
fn collect_read_primitive_rejects_binary_or_non_utf8_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-project-file-binary-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("binary.bin"), [0xff, 0xfe, 0xfd]).unwrap();

    let error =
        collect_project_file(&root, "binary.bin", ProjectFileOptions::default()).unwrap_err();

    assert!(error.to_string().contains("binary or non-UTF-8"));
}

#[test]
fn collect_find_matches_returns_bounded_path_matches() {
    let root = std::env::temp_dir().join(format!("elgar-find-test-{}", std::process::id()));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}",
    )
    .unwrap();

    let snapshot = collect_find_matches(&root, ".", "page", FindOptions::default()).unwrap();

    assert!(snapshot.matches.iter().any(|path| path == "app/page.tsx"));
}

#[test]
fn collect_grep_matches_returns_bounded_text_matches() {
    let root = std::env::temp_dir().join(format!("elgar-grep-test-{}", std::process::id()));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}",
    )
    .unwrap();

    let snapshot =
        collect_grep_matches(&root, ".", "export default", GrepOptions::default()).unwrap();

    assert!(snapshot
        .matches
        .iter()
        .any(|item| item.path == "app/page.tsx" && item.line_number == 1));
}

#[test]
fn collect_directory_summary_counts_and_samples_directory() {
    let root = std::env::temp_dir().join(format!(
        "elgar-directory-summary-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("node_modules/demo")).unwrap();
    fs::write(root.join("node_modules/demo/package.json"), "{}").unwrap();

    let snapshot = collect_directory_summary(
        &root,
        "node_modules",
        DirectoryOptions {
            max_depth: 2,
            max_entries: 10,
            max_counted_paths: 100,
            max_rendered_bytes: 1000,
        },
    )
    .unwrap();

    assert_eq!(snapshot.display_path, "node_modules");
    assert_eq!(snapshot.total_files, 1);
    assert_eq!(snapshot.total_directories, 1);
    assert!(!snapshot.entries.is_empty());
}

#[test]
fn collect_project_file_allows_existing_parent_relative_file() {
    let base = std::env::temp_dir().join(format!(
        "elgar-project-file-parent-test-{}",
        std::process::id()
    ));
    let root = base.join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(base.join("shared.txt"), "outside project").unwrap();

    let snapshot =
        collect_project_file(&root, "../shared.txt", ProjectFileOptions::default()).unwrap();

    assert_eq!(snapshot.display_path, "../shared.txt");
    assert_eq!(snapshot.contents, "outside project");
}

#[test]
fn collect_project_file_rejects_missing_file() {
    let root = std::env::temp_dir();

    let error = collect_project_file(
        root,
        "missing-elgar-file.txt",
        ProjectFileOptions::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("file does not exist"));
}
