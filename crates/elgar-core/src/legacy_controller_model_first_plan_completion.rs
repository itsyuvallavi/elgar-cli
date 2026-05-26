use std::path::{Path, PathBuf};

use crate::{action::ActionRequest, model_runtime::ValidatedModelToolAction, session::Session};

const VERIFIED_PLAN_CONTENT_BYTE_LIMIT: usize = 720;

#[derive(Debug, Clone)]
pub(crate) struct ModelFirstPlanCompletenessNeed {
    pub(crate) plan_path: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) expected_files: Vec<PathBuf>,
    pub(crate) missing_files: Vec<PathBuf>,
    pub(crate) plan_excerpt: String,
    pub(crate) plan_contents: String,
}

pub(crate) fn model_first_verified_plan_completion_need(
    session: &Session,
    input: &str,
    validated_actions: &[ValidatedModelToolAction],
) -> Option<ModelFirstPlanCompletenessNeed> {
    if !is_model_first_verified_plan_implementation_request(input) {
        return None;
    }
    if validated_actions.is_empty() {
        return None;
    }

    let reference = session.project_memory().latest_verified_plan()?;
    if !reference.path.is_file() || !reference.project_root.is_dir() {
        return None;
    }

    let contents = std::fs::read_to_string(&reference.path).ok()?;
    let expected_files =
        expected_files_from_verified_plan(&contents, &reference.project_root, &reference.path);
    if expected_files.len() < 2 {
        return None;
    }

    let missing_files = missing_expected_verified_plan_files(
        &expected_files,
        &reference.project_root,
        validated_actions,
    );
    if missing_files.is_empty() {
        return None;
    }

    Some(ModelFirstPlanCompletenessNeed {
        plan_path: reference.path.clone(),
        project_root: reference.project_root.clone(),
        expected_files,
        missing_files,
        plan_excerpt: truncate_line(
            &compact_prompt_text(&contents),
            VERIFIED_PLAN_CONTENT_BYTE_LIMIT,
        ),
        plan_contents: contents,
    })
}

pub(crate) fn is_model_first_verified_plan_implementation_request(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "do not implement",
            "don't implement",
            "dont implement",
            "do not execute",
            "don't execute",
            "dont execute",
            "do not create the files",
            "don't create the files",
            "dont create the files",
            "do not build",
            "don't build",
            "dont build",
        ],
    ) {
        return false;
    }

    contains_any(
        &lower,
        &[
            "implement",
            "create the project",
            "create a project",
            "create the rest",
            "rest of the project",
            "make the files",
            "build",
            "scaffold",
            "execute",
            "according to the plan",
        ],
    ) && VerifiedMemoryNeed::from_input(input).plan
}

pub(crate) fn expected_files_from_verified_plan(
    contents: &str,
    project_root: &Path,
    plan_path: &Path,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let tree_files = expected_files_from_tree_lines(contents, project_root, plan_path);
    for path in &tree_files {
        push_unique_path(&mut files, path.clone());
    }
    for token in contents.split_whitespace() {
        let Some(path) = expected_file_token_to_relative_path(token, project_root, plan_path)
        else {
            continue;
        };
        if tree_files
            .iter()
            .any(|tree_file| tree_file != &path && tree_file.ends_with(&path))
        {
            continue;
        }
        push_unique_path(&mut files, path);
    }
    files
}

fn expected_files_from_tree_lines(
    contents: &str,
    project_root: &Path,
    plan_path: &Path,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut directory_stack: Vec<String> = Vec::new();
    let entries: Vec<(usize, &str)> = contents.lines().filter_map(tree_line_entry).collect();

    for (index, (depth, entry)) in entries.iter().enumerate() {
        let entry = clean_tree_entry(entry);
        if entry.is_empty() {
            continue;
        }

        directory_stack.truncate(*depth);
        let has_child = entries
            .get(index + 1)
            .is_some_and(|(next_depth, _)| next_depth > depth);
        if entry.ends_with('/') || has_child {
            directory_stack.push(entry.trim_end_matches('/').to_string());
            continue;
        }

        let mut path = PathBuf::new();
        for directory in directory_stack.iter().take(*depth) {
            path.push(directory);
        }
        path.push(entry);
        if let Some(path) = expected_tree_file_to_relative_path(
            &path.display().to_string(),
            project_root,
            plan_path,
        ) {
            push_unique_path(&mut files, path);
        }
    }

    files
}

fn tree_line_entry(line: &str) -> Option<(usize, &str)> {
    let marker_text = ["├── ", "└── ", "├─ ", "└─ "]
        .iter()
        .find(|marker| line.contains(**marker))?;
    let marker = line.find(marker_text)?;
    let prefix = &line[..marker];
    let depth = prefix.chars().filter(|character| *character == '│').count();
    line.get(marker + marker_text.len()..)
        .map(|entry| (depth, entry))
}

fn clean_tree_entry(entry: &str) -> &str {
    entry
        .split(" # ")
        .next()
        .unwrap_or(entry)
        .trim()
        .trim_matches('`')
}

fn push_unique_path(files: &mut Vec<PathBuf>, path: PathBuf) {
    if !files.contains(&path) {
        files.push(path);
    }
}

pub(crate) fn expected_file_token_to_relative_path(
    token: &str,
    project_root: &Path,
    plan_path: &Path,
) -> Option<PathBuf> {
    let token = trim_markdown_path_punctuation(token);
    let token = trim_markdown_path_punctuation(token.trim_end_matches('.'));
    if token.is_empty() || token.contains("://") || token.ends_with('/') {
        return None;
    }

    let path = PathBuf::from(token.trim_start_matches("./"));
    if !looks_like_project_file_path(&path) {
        return None;
    }

    let relative = if path.is_absolute() {
        path.strip_prefix(project_root).ok()?.to_path_buf()
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    let plan_relative = plan_path
        .strip_prefix(project_root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| plan_path.file_name().map(PathBuf::from));
    if plan_relative.as_ref() == Some(&relative) {
        return None;
    }

    Some(relative)
}

fn expected_tree_file_to_relative_path(
    token: &str,
    project_root: &Path,
    plan_path: &Path,
) -> Option<PathBuf> {
    let token = trim_markdown_path_punctuation(token);
    let token = trim_markdown_path_punctuation(token.trim_end_matches('.'));
    if token.is_empty() || token.contains("://") || token.ends_with('/') {
        return None;
    }

    let path = PathBuf::from(token.trim_start_matches("./"));
    let relative = if path.is_absolute() {
        path.strip_prefix(project_root).ok()?.to_path_buf()
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    let plan_relative = plan_path
        .strip_prefix(project_root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| plan_path.file_name().map(PathBuf::from));
    if plan_relative.as_ref() == Some(&relative) {
        return None;
    }

    Some(relative)
}

pub(crate) fn trim_markdown_path_punctuation(value: &str) -> &str {
    value.trim_matches(|character: char| {
        matches!(
            character,
            '`' | '*'
                | '"'
                | '\''
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | ','
                | ';'
                | ':'
        )
    })
}

pub(crate) fn looks_like_project_file_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if file_name.starts_with('.') && file_name.len() > 1 {
        return true;
    }
    file_name.contains('.')
}

pub(crate) fn missing_expected_verified_plan_files(
    expected_files: &[PathBuf],
    project_root: &Path,
    validated_actions: &[ValidatedModelToolAction],
) -> Vec<PathBuf> {
    expected_files
        .iter()
        .filter(|relative| {
            let already_exists = project_root.join(relative).is_file();
            let will_create = validated_actions
                .iter()
                .any(|action| action_creates_expected_file(action, project_root, relative));
            !already_exists && !will_create
        })
        .cloned()
        .collect()
}

pub(crate) fn action_creates_expected_file(
    action: &ValidatedModelToolAction,
    project_root: &Path,
    expected_relative: &Path,
) -> bool {
    let ActionRequest::CreateFile(create_file) = &action.request else {
        return false;
    };
    let target_path = &create_file.target_path;
    if target_path.is_absolute() {
        return target_path
            .strip_prefix(project_root)
            .is_ok_and(|relative| relative == expected_relative)
            || target_path.ends_with(expected_relative);
    }
    target_path == expected_relative || target_path.ends_with(expected_relative)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedMemoryNeed {
    plan: bool,
}

impl VerifiedMemoryNeed {
    fn from_input(input: &str) -> Self {
        let lower = input.to_ascii_lowercase();
        let reference = contains_any(
            &lower,
            &[
                "that ",
                "this ",
                "the folder",
                "the directory",
                "the plan",
                "the project",
                "same folder",
                "same directory",
                "inside the folder you created",
                "folder you created",
                "rest of the project",
                "go ahead and make the files",
                "make the files",
                "implement the plan",
                "where is",
                "where did you put",
                "what path",
                "path did you create",
                "dont see",
                "don't see",
                "continue",
                "next step",
                "run it",
                "execute it",
            ],
        );
        let plan = reference
            && contains_any(
                &lower,
                &[
                    "plan",
                    "implement",
                    "execute",
                    "run it",
                    "continue",
                    "next step",
                    "project",
                    "make the files",
                    "rest of the project",
                ],
            );

        Self { plan }
    }
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn compact_prompt_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        return line.to_string();
    }

    let suffix = "...";
    let max_content = max_bytes.saturating_sub(suffix.len());
    let mut end = max_content.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &line[..end], suffix)
}

#[cfg(test)]
mod tests {
    use super::{
        expected_files_from_verified_plan, is_model_first_verified_plan_implementation_request,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn expected_files_include_bold_markdown_paths_and_dotfiles() {
        let project_root = Path::new("/Users/yuval/Desktop/ElgarDesktopReactSmoke");
        let plan_path = project_root.join("project_plan.md");
        let plan = r#"
# React TypeScript Tailwind Project Plan

## Folder structure
```
ElgarDesktopReactSmoke/
├─ public/
│  └─ index.html
├─ src/
│  ├─ app/
│  │  └─ page.tsx
│  ├─ components/
│  │  └─ Button.tsx
│  ├─ styles/
│  │  └─ globals.css
│  ├─ main.tsx
├─ .gitignore
├─ package.json
├─ tsconfig.json
├─ tailwind.config.js
├─ postcss.config.js
├─ next.config.js
├─ README.md
```

## Files to create
1. **public/index.html** - HTML skeleton.
2. **src/main.tsx** - React entry.
3. **src/styles/globals.css** - Tailwind imports.
4. **src/components/Button.tsx** - button component.
5. **package.json** - dependencies.
6. **tsconfig.json** - TypeScript config.
7. **tailwind.config.js** - Tailwind config.
8. **postcss.config.js** - PostCSS config.
9. **next.config.js** - optional config.
10. **README.md** - usage notes.
11. **.gitignore** - ignored files.
"#;

        let expected = expected_files_from_verified_plan(plan, project_root, &plan_path);

        for relative in [
            "public/index.html",
            "src/main.tsx",
            "src/styles/globals.css",
            "src/components/Button.tsx",
            "package.json",
            "tsconfig.json",
            "tailwind.config.js",
            "postcss.config.js",
            "next.config.js",
            "README.md",
            ".gitignore",
        ] {
            assert!(
                expected.contains(&PathBuf::from(relative)),
                "missing expected file {relative}; got {expected:?}"
            );
        }
        assert!(!expected.contains(&PathBuf::from("project_plan.md")));
    }

    #[test]
    fn expected_files_include_nested_tree_paths_without_bare_duplicates() {
        let project_root = Path::new("/tmp/Demo");
        let plan_path = project_root.join("plan.txt");
        let plan = r#"
# Demo Plan

Demo/
├── package.json
├── ts-demo/
│   ├── src/
│   │   └── index.ts    # Main TS file
│   ├── package.json
│   └── tsconfig.json
├── py-demo/
│   ├── src/
│   │   └── main.py    # Main Python file
│   └── requirements.txt
└── README.md

Key files: src/index.ts, src/main.py, index.ts, main.py, package.json.
"#;

        let expected = expected_files_from_verified_plan(plan, project_root, &plan_path);

        for relative in [
            "package.json",
            "ts-demo/src/index.ts",
            "ts-demo/package.json",
            "ts-demo/tsconfig.json",
            "py-demo/src/main.py",
            "py-demo/requirements.txt",
            "README.md",
        ] {
            assert!(
                expected.contains(&PathBuf::from(relative)),
                "missing expected file {relative}; got {expected:?}"
            );
        }
        for duplicate in ["src/index.ts", "src/main.py", "index.ts", "main.py"] {
            assert!(!expected.contains(&PathBuf::from(duplicate)));
        }
    }

    #[test]
    fn expected_files_include_extensionless_tree_files_without_code_block_indent_nesting() {
        let project_root = Path::new("/tmp/Demo");
        let plan_path = project_root.join("plan.md");
        let plan = r#"
```text
    Demo/
    ├── Dockerfile
    ├── Makefile
    ├── cmd/
    │   └── main.go
    └── README
```
"#;

        let expected = expected_files_from_verified_plan(plan, project_root, &plan_path);

        for relative in ["Dockerfile", "Makefile", "cmd/main.go", "README"] {
            assert!(
                expected.contains(&PathBuf::from(relative)),
                "missing expected file {relative}; got {expected:?}"
            );
        }
        assert!(!expected.contains(&PathBuf::from("cmd/README")));
    }

    #[test]
    fn implementation_request_ignores_explicit_do_not_implement_plan_prompt() {
        assert!(!is_model_first_verified_plan_implementation_request(
            "create a plan for a React TypeScript Tailwind project, but do not implement yet"
        ));
        assert!(!is_model_first_verified_plan_implementation_request(
            "create a plan and don't create the files yet"
        ));
        assert!(is_model_first_verified_plan_implementation_request(
            "okay execute it"
        ));
    }
}
