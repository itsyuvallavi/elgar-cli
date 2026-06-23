//! Section renderer tests.

use super::{render_response_sections, ResponseSection};

#[test]
fn renders_sections_in_one_container() {
    let rendered = render_response_sections(&[
        ResponseSection {
            title: "Summary".to_string(),
            lines: vec!["Todo app created.".to_string()],
        },
        ResponseSection {
            title: "Files".to_string(),
            lines: vec!["- `app/page.tsx`".to_string()],
        },
    ]);

    assert!(rendered.starts_with(" ╭─ response "));
    assert!(rendered.contains("│ Summary"));
    assert!(rendered.contains("│   Todo app created."));
    assert!(rendered.contains("│ Files"));
    assert!(rendered.contains("`app/page.tsx`"));
    assert!(rendered.ends_with('╯'));
}

#[test]
fn aligns_wrapped_bullet_continuations() {
    let rendered = render_response_sections(&[ResponseSection {
        title: "Verification".to_string(),
        lines: vec![
            "- Run `npm start` and manually test adding completing and deleting a todo item in the browser."
                .to_string(),
        ],
    }]);

    let wrapped = rendered
        .lines()
        .filter(|line| line.contains("Run `npm start`") || line.contains("the browser"))
        .collect::<Vec<_>>();

    assert_eq!(wrapped.len(), 2, "{wrapped:#?}");
    assert!(wrapped[0].contains("│   - Run `npm start`"), "{wrapped:#?}");
    assert!(wrapped[1].contains("│     and deleting"), "{wrapped:#?}");
}

#[test]
fn section_container_fits_common_narrow_terminal_width() {
    let rendered = render_response_sections(&[
        ResponseSection {
            title: "Summary".to_string(),
            lines: vec![
                "I have access to tools that interact with your local project files and environment."
                    .to_string(),
            ],
        },
        ResponseSection {
            title: "File System & Inspection".to_string(),
            lines: vec![
                "- `find`: Search for files or folders by name pattern like `*.py` or `README`."
                    .to_string(),
            ],
        },
    ]);

    assert!(
        rendered.lines().all(|line| line.chars().count() <= 63),
        "{rendered}"
    );
}

#[test]
fn long_structured_answers_use_plain_sections() {
    let rendered = render_response_sections(&[
        ResponseSection {
            title: "Summary".to_string(),
            lines: vec![
                "This is a longer assistant response that should stay plain instead of boxed."
                    .to_string(),
            ],
        },
        ResponseSection {
            title: "File System & Inspection".to_string(),
            lines: vec![
                "- `read`: Read the contents of a specific file.".to_string(),
                "- `ls`: List files and directories within a folder.".to_string(),
                "- `find`: Search for files or directories by name pattern.".to_string(),
                "- `grep`: Search for specific text inside files.".to_string(),
            ],
        },
        ResponseSection {
            title: "Modification & Execution".to_string(),
            lines: vec![
                "- `bash`: Run shell commands.".to_string(),
                "- `write`: Create or overwrite a file.".to_string(),
                "- `edit`: Replace specific text within an existing file.".to_string(),
            ],
        },
        ResponseSection {
            title: "Notes".to_string(),
            lines: vec!["These actions are verified by the harness.".to_string()],
        },
    ]);

    assert!(!rendered.contains("╭─ response"), "{rendered}");
    assert!(rendered.starts_with("Summary\n"));
    assert!(rendered.contains("File System & Inspection"));
    assert!(rendered.contains("  - `read`:"));
}
