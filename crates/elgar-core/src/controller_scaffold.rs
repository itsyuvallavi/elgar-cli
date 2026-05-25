use std::path::{Path, PathBuf};

use crate::{
    action::{ActionRequest, CreateDirectoryAction, CreateFileAction},
    legacy_controller_model_first_plan_completion::ModelFirstPlanCompletenessNeed,
    model_runtime::ValidatedModelToolAction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectScaffoldPlan {
    pub(crate) directories: Vec<PathBuf>,
    pub(crate) files: Vec<(PathBuf, String)>,
}

pub(crate) fn build_project_scaffold_plan(
    base_path: &Path,
    plan_contents: &str,
) -> ProjectScaffoldPlan {
    if is_typescript_python_project_plan(plan_contents) {
        build_typescript_python_project_plan(base_path)
    } else if is_react_ts_project_plan(plan_contents) {
        build_react_ts_project_plan(base_path)
    } else {
        build_small_python_project_plan(base_path, plan_contents)
    }
}

pub(crate) fn first_existing_scaffold_target(
    project_plan: &ProjectScaffoldPlan,
) -> Option<PathBuf> {
    project_plan
        .directories
        .iter()
        .chain(project_plan.files.iter().map(|(path, _contents)| path))
        .find(|path| path.try_exists().unwrap_or(true))
        .cloned()
}

/// Build controller-owned create actions only for the verified React/Vite fallback path.
pub(crate) fn controller_owned_verified_plan_scaffold_actions(
    need: &ModelFirstPlanCompletenessNeed,
) -> Option<Vec<ValidatedModelToolAction>> {
    if !is_react_ts_project_plan(&need.plan_contents) {
        return None;
    }

    let project_plan = build_project_scaffold_plan(&need.project_root, &need.plan_contents);
    if project_plan.files.is_empty() {
        return None;
    }

    let mut actions = Vec::new();
    for (index, directory) in project_plan.directories.iter().enumerate() {
        actions.push(ValidatedModelToolAction {
            tool_call_id: format!("controller-owned-scaffold-dir-{}", index + 1),
            request: ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: directory.clone(),
            }),
            summary: format!(
                "Controller-owned verified-plan scaffold directory {}",
                directory.display()
            ),
            target_label: directory.display().to_string(),
        });
    }

    for (index, (path, contents)) in project_plan.files.iter().enumerate() {
        actions.push(ValidatedModelToolAction {
            tool_call_id: format!("controller-owned-scaffold-file-{}", index + 1),
            request: ActionRequest::CreateFile(CreateFileAction {
                target_path: path.clone(),
                contents: contents.clone(),
            }),
            summary: format!(
                "Controller-owned verified-plan scaffold file {}",
                path.display()
            ),
            target_label: path.display().to_string(),
        });
    }

    Some(actions)
}

fn is_typescript_python_project_plan(plan_contents: &str) -> bool {
    let normalized = plan_contents.to_ascii_lowercase();
    let mentions_typescript = mentions_typescript(&normalized)
        || normalized.contains(".ts")
        || normalized.contains("package.json")
        || normalized.contains("tsconfig");
    let mentions_python = normalized.contains("python")
        || normalized.contains(".py")
        || normalized.contains("requirements.txt");

    mentions_typescript && mentions_python
}

fn is_react_ts_project_plan(plan_contents: &str) -> bool {
    let normalized = plan_contents.to_ascii_lowercase();
    normalized.contains("react")
        && (mentions_typescript(&normalized)
            || normalized.contains("react ts")
            || normalized.contains("vite-style react scaffold")
            || normalized.contains("react project plan"))
}

fn mentions_typescript(input: &str) -> bool {
    input.contains(" ts") || input.contains("typescript") || input.contains("type script")
}

fn build_small_python_project_plan(base_path: &Path, plan_contents: &str) -> ProjectScaffoldPlan {
    let title = plan_contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("# "))
        .unwrap_or("Small Python Project")
        .trim();
    let safe_title = if title.is_empty() {
        "Small Python Project"
    } else {
        title
    };

    let src_dir = base_path.join("src");
    let tests_dir = base_path.join("tests");
    let files = vec![
        (src_dir.join("__init__.py"), String::new()),
        (src_dir.join("csv_filter.py"), csv_filter_source()),
        (tests_dir.join("test_csv_filter.py"), csv_filter_test_source()),
        (
            base_path.join("README.md"),
            format!(
                "# {safe_title}\n\nA small Python project scaffold generated from the approved Markdown plan.\n\n## Run tests\n\n```bash\npython -m unittest discover -s tests\n```\n"
            ),
        ),
        (
            base_path.join("pyproject.toml"),
            "[project]\nname = \"elgar-small-python-project\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n\n[tool.pytest.ini_options]\npythonpath = [\".\"]\n".to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir, tests_dir],
        files,
    }
}

fn build_typescript_python_project_plan(base_path: &Path) -> ProjectScaffoldPlan {
    let src_dir = base_path.join("src");
    let python_dir = base_path.join("python");
    let files = vec![
        (base_path.join("package.json"), ts_python_package_json()),
        (base_path.join("tsconfig.json"), ts_python_tsconfig_json()),
        (src_dir.join("main.ts"), ts_python_main_source()),
        (python_dir.join("main.py"), ts_python_python_main_source()),
        (
            base_path.join("requirements.txt"),
            "# Add Python dependencies here.\n".to_string(),
        ),
        (
            base_path.join("README.md"),
            "# TypeScript and Python Project\n\nA local scaffold generated from the verified Markdown plan.\n\n## TypeScript\n\n```bash\nnpm install\nnpm run build\n```\n\n## Python\n\n```bash\npython python/main.py\n```\n"
                .to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir, python_dir],
        files,
    }
}

fn build_react_ts_project_plan(base_path: &Path) -> ProjectScaffoldPlan {
    let src_dir = base_path.join("src");
    let files = vec![
        (base_path.join("package.json"), react_ts_package_json()),
        (base_path.join("index.html"), react_ts_index_html()),
        (base_path.join("tsconfig.json"), react_tsconfig_json()),
        (base_path.join("vite.config.ts"), react_ts_vite_config()),
        (src_dir.join("main.tsx"), react_ts_main_source()),
        (src_dir.join("App.tsx"), react_ts_app_source()),
        (src_dir.join("styles.css"), react_ts_styles_source()),
        (
            base_path.join("README.md"),
            "# React TS Project\n\nA local React TypeScript scaffold generated from the approved Markdown plan.\n\n## Deferred dependency install\n\nPackage installation is deferred. Propose and approve a separate shell command such as `npm install` before downloading dependencies.\n\n## After install\n\n```bash\nnpm run dev\n```\n"
                .to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir],
        files,
    }
}

fn ts_python_package_json() -> String {
    r#"{
  "name": "elgar-typescript-python-project",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "build": "tsc --noEmit",
    "start": "node dist/main.js"
  },
  "devDependencies": {
    "typescript": "latest"
  }
}
"#
    .to_string()
}

fn ts_python_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Node",
    "strict": true,
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src"]
}
"#
    .to_string()
}

fn ts_python_main_source() -> String {
    r#"export function greet(name: string): string {
  return `Hello, ${name}`;
}

console.log(greet("Elgar"));
"#
    .to_string()
}

fn ts_python_python_main_source() -> String {
    r#"from __future__ import annotations


def greet(name: str) -> str:
    return f"Hello, {name}"


if __name__ == "__main__":
    print(greet("Elgar"))
"#
    .to_string()
}

fn react_ts_package_json() -> String {
    r#"{
  "name": "elgar-react-ts-project",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@vitejs/plugin-react": "latest",
    "vite": "latest",
    "typescript": "latest",
    "react": "latest",
    "react-dom": "latest"
  },
  "devDependencies": {}
}
"#
    .to_string()
}

fn react_ts_index_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>React TS Project</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
"#
    .to_string()
}

fn react_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["DOM", "DOM.Iterable", "ES2020"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Node",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx"
  },
  "include": ["src"],
  "references": []
}
"#
    .to_string()
}

fn react_ts_vite_config() -> String {
    r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
});
"#
    .to_string()
}

fn react_ts_main_source() -> String {
    r#"import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
"#
    .to_string()
}

fn react_ts_app_source() -> String {
    r#"export default function App() {
  return (
    <main className="app-shell">
      <section>
        <p className="eyebrow">Elgar scaffold</p>
        <h1>React TS Project</h1>
        <p>
          This project was created from a controller-owned, approved plan.
          Install dependencies only after approving a separate shell command.
        </p>
      </section>
    </main>
  );
}
"#
    .to_string()
}

fn react_ts_styles_source() -> String {
    r#":root {
  color: #1f2937;
  background: #f8fafc;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

.app-shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

section {
  width: min(680px, 100%);
}

.eyebrow {
  color: #0f766e;
  font-size: 0.8rem;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 12px;
  font-size: 2.5rem;
}

p {
  line-height: 1.6;
}
"#
    .to_string()
}

fn csv_filter_source() -> String {
    r#"from __future__ import annotations

import argparse
import csv
from pathlib import Path


def filter_rows(input_path: Path, output_path: Path, column: str, value: str) -> int:
    with input_path.open(newline="") as source:
        reader = csv.DictReader(source)
        rows = [row for row in reader if row.get(column) == value]
        fieldnames = reader.fieldnames or []

    with output_path.open("w", newline="") as destination:
        writer = csv.DictWriter(destination, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)

    return len(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description="Filter a CSV by one column value.")
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("column")
    parser.add_argument("value")
    args = parser.parse_args()

    count = filter_rows(args.input, args.output, args.column, args.value)
    print(f"wrote {count} row(s)")


if __name__ == "__main__":
    main()
"#
    .to_string()
}

fn csv_filter_test_source() -> String {
    r#"from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from src.csv_filter import filter_rows


class CsvFilterTests(unittest.TestCase):
    def test_filters_rows_by_column_value(self) -> None:
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "input.csv"
            output = root / "output.csv"
            source.write_text("name,kind\nalpha,keep\nbeta,drop\n", encoding="utf-8")

            count = filter_rows(source, output, "kind", "keep")

            self.assertEqual(count, 1)
            self.assertEqual(output.read_text(encoding="utf-8"), "name,kind\nalpha,keep\n")


if __name__ == "__main__":
    unittest.main()
"#
    .to_string()
}
