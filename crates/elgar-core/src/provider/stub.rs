use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    provider::types::{
        ChatToolDefinition, ControllerProvider, ProviderError, ProviderRequestMetadata,
    },
};

/// Deterministic provider stub for no-model controller tests.
///
/// This stub never performs network calls, filesystem writes, shell commands,
/// action transitions, or any other side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStub {
    pub provider: String,
    pub model: Option<String>,
}

impl ProviderStub {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn ask(&self, prompt: &str) -> ProviderStubResponse {
        let visible_prompt = visible_user_prompt(prompt);
        ProviderStubResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_id: "stub-request-1".to_string(),
            output: ProviderOutput::new(format!(
                "stub provider response (no-network) to: {}. No live provider call was made. For explicit LM Studio TUI smoke, set ELGAR_LM_STUDIO_MODEL and run `cargo run -p elgar-cli -- tui-controller-smoke \"Say hello in one sentence.\"`.",
                visible_prompt
            )),
        }
    }

    pub fn ask_with_tools(&self, prompt: &str) -> ProviderStubResponse {
        let visible_prompt = visible_user_prompt(prompt);
        let output = deterministic_stub_tool_output(visible_prompt).unwrap_or_else(|| {
            ProviderOutput::new(format!(
                "stub provider response (no-network) to: {}. No live provider call was made. For explicit LM Studio TUI smoke, set ELGAR_LM_STUDIO_MODEL and run `cargo run -p elgar-cli -- tui-controller-smoke \"Say hello in one sentence.\"`.",
                visible_prompt
            ))
        });

        ProviderStubResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_id: "stub-request-1".to_string(),
            output,
        }
    }
}

impl ControllerProvider for ProviderStub {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(self.provider.clone(), self.model.clone(), "stub-request-1")
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(self.ask(prompt).output)
    }

    fn chat_with_tools_with_metadata(
        &self,
        prompt: &str,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        Ok(self.ask_with_tools(prompt).output)
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<crate::provider::ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        let context = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        if messages
            .iter()
            .any(|message| matches!(message.role, crate::provider::ChatRole::Tool))
        {
            return Ok(ProviderOutput::new(stub_done_text(&context)));
        }

        let prompt = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, crate::provider::ChatRole::User))
            .map(|message| message.content.as_str())
            .unwrap_or_default();
        let visible_prompt = visible_user_prompt(prompt);
        let output = deterministic_stub_followup_tool_output(visible_prompt, &context)
            .or_else(|| deterministic_stub_tool_output(visible_prompt))
            .unwrap_or_else(|| {
                ProviderOutput::new(format!(
                    "stub provider response (no-network) to: {}. No live provider call was made. For explicit LM Studio TUI smoke, set ELGAR_LM_STUDIO_MODEL and run `cargo run -p elgar-cli -- tui-controller-smoke \"Say hello in one sentence.\"`.",
                    visible_prompt
                ))
            });
        Ok(output)
    }
}

fn stub_done_text(context: &str) -> String {
    if context.contains("python-ts-project-plan.md")
        || context.contains("Python + TypeScript Project Plan")
    {
        return stub_python_ts_plan_summary();
    }

    "Done.".to_string()
}

impl Default for ProviderStub {
    fn default() -> Self {
        Self::new("stub-provider")
    }
}

fn visible_user_prompt(prompt: &str) -> &str {
    prompt
        .rsplit_once("User request:\n")
        .map(|(_context, request)| request.trim())
        .unwrap_or_else(|| prompt.trim())
}

fn deterministic_stub_tool_output(prompt: &str) -> Option<ProviderOutput> {
    let normalized = prompt.trim();
    let lower = normalized.to_ascii_lowercase();

    if asks_to_read_existing_plan(&lower) {
        return Some(ProviderOutput::new(
            "The latest plan is the project plan I created in the last verified folder.",
        ));
    }

    if contains_any(&lower, &["that folder", "that directory"]) {
        return Some(stub_guidance_output(
            "Which folder should I use for the project?",
            "The request refers to a folder that is not identified in the request.",
        ));
    }

    if let Some(output) = deterministic_stub_plan_tool_output(&lower) {
        return Some(output);
    }

    if let Some(target_path) = extract_create_file_target(normalized, &lower) {
        return Some(
            ProviderOutput::new(format!("Creating {target_path}.")).with_tool_calls(vec![
                RawModelToolCall {
                    id: "stub-tool-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: serde_json::json!({
                        "target_path": target_path,
                        "contents": ""
                    }),
                    assistant_summary: Some(format!("create {target_path}")),
                },
            ]),
        );
    }

    if is_compound_folder_project_request(&lower) {
        if let Some(output) = deterministic_stub_project_tool_output(normalized, &lower) {
            return Some(output);
        }
    }

    if let Some(target_path) = extract_create_directory_target(normalized, &lower) {
        return Some(
            ProviderOutput::new(format!("Creating {target_path}.")).with_tool_calls(vec![
                RawModelToolCall {
                    id: "stub-tool-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: serde_json::json!({
                        "target_path": target_path
                    }),
                    assistant_summary: Some(format!("create directory {target_path}")),
                },
            ]),
        );
    }

    if let Some(output) = deterministic_stub_project_tool_output(normalized, &lower) {
        return Some(output);
    }

    if let Some(command) = extract_shell_command(normalized) {
        return Some(
            ProviderOutput::new("Drafting shell command for review.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "stub-tool-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: serde_json::json!({
                        "command": command,
                        "cwd": "."
                    }),
                    assistant_summary: Some("run shell command".to_string()),
                },
            ]),
        );
    }

    None
}

fn is_compound_folder_project_request(lower: &str) -> bool {
    contains_any(
        lower,
        &["inside the folder", "in that folder", "inside that folder"],
    ) && contains_any(lower, &["project", "app", "calculator"])
        && contains_any(lower, &["create", "implement", "make", "build", "scaffold"])
}

fn stub_guidance_output(question: &str, reason: &str) -> ProviderOutput {
    ProviderOutput::new("").with_tool_calls(vec![RawModelToolCall {
        id: "stub-guidance-call-1".to_string(),
        name: RawModelToolName::Known(ModelToolName::AskGuidance),
        arguments: serde_json::json!({
            "question": question,
            "reason": reason
        }),
        assistant_summary: None,
    }])
}

fn deterministic_stub_plan_tool_output(lower: &str) -> Option<ProviderOutput> {
    if !contains_any(lower, &["create a plan", "write a plan", "draft a plan"]) {
        return None;
    }

    let (file_name, title) =
        if lower.contains("python") && (lower.contains(" ts") || lower.contains("typescript")) {
            (
                "python-ts-project-plan.md",
                "Python + TypeScript Project Plan",
            )
        } else if lower.contains("react ts") || lower.contains("react typescript") {
            ("react-ts-project-plan.md", "React TS Project Plan")
        } else if lower.contains("react") {
            ("react-project-plan.md", "React Project Plan")
        } else if lower.contains("python") {
            ("python-project-plan.md", "Python Project Plan")
        } else {
            ("project-plan.md", "Project Plan")
        };
    let contents = format!(
        "# {title}\n\n- Create local project files with model tool calls.\n- Defer package installation.\n"
    );

    Some(
        ProviderOutput::new(format!("Creating {file_name}.")).with_tool_calls(vec![
            RawModelToolCall {
                id: "stub-tool-call-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": file_name,
                    "contents": contents
                }),
                assistant_summary: Some(format!("create {file_name}")),
            },
        ]),
    )
}

fn deterministic_stub_followup_tool_output(prompt: &str, context: &str) -> Option<ProviderOutput> {
    let lower_prompt = prompt.trim().to_ascii_lowercase();
    let lower_context = context.to_ascii_lowercase();
    let short_choice = matches!(
        lower_prompt.trim_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace()),
        "your choice" | "up to you" | "whatever you want" | "whatever you think" | "you choose"
    );

    if asks_to_read_existing_plan(&lower_prompt)
        || (short_choice && lower_context.contains("latest verified plan:"))
    {
        return Some(ProviderOutput::new(stub_plan_summary(&lower_context)));
    }

    if short_choice
        && lower_context.contains("create a plan")
        && lower_context.contains("python")
        && (lower_context.contains(" ts") || lower_context.contains("typescript"))
    {
        return deterministic_stub_plan_tool_output("create a plan for a python and ts project");
    }

    if lower_prompt.contains("plan i asked") && lower_context.contains("python + typescript") {
        return Some(ProviderOutput::new(
            "The latest plan is a Python + TypeScript project plan.",
        ));
    }

    None
}

fn asks_to_read_existing_plan(lower: &str) -> bool {
    contains_any(
        lower,
        &[
            "what's the plan",
            "whats the plan",
            "read the plan",
            "show me the plan",
            "tell me the plan",
        ],
    )
}

fn stub_plan_summary(lower_context: &str) -> String {
    if lower_context.contains("python-ts-project-plan.md")
        || lower_context.contains("python + typescript project plan")
    {
        return stub_python_ts_plan_summary();
    }

    "The latest plan is the project plan I created in the last verified folder.".to_string()
}

fn stub_python_ts_plan_summary() -> String {
    "The latest plan is a Python + TypeScript project plan: create local project files with model tool calls, then defer package installation until you ask to implement it.".to_string()
}

fn deterministic_stub_project_tool_output(original: &str, lower: &str) -> Option<ProviderOutput> {
    let plan_followup = contains_any(
        lower,
        &[
            "the plan",
            "you planned",
            "we planned",
            "rest of the project",
            "implement the plan",
        ],
    );
    if !plan_followup && !contains_any(lower, &["project", "app", "calculator"]) {
        return None;
    }
    if !contains_any(lower, &["create", "implement", "make", "build", "scaffold"]) {
        return None;
    }

    let project_root = (!plan_followup)
        .then(|| extract_stub_project_root(original, lower))
        .flatten();

    let calls = if lower.contains("next") && lower.contains("tailwind") {
        stub_next_tailwind_project_tool_calls(project_root.as_deref())
    } else if lower.contains("calculator") || lower.contains("python") {
        stub_python_calculator_project_tool_calls(project_root.as_deref())
    } else {
        stub_react_project_tool_calls(project_root.as_deref())
    };

    Some(ProviderOutput::new("Creating project files.").with_tool_calls(calls))
}

fn stub_python_calculator_project_tool_calls(project_root: Option<&str>) -> Vec<RawModelToolCall> {
    let root = project_root.unwrap_or("");
    vec![
        stub_create_directory_call("stub-tool-call-1", root),
        stub_create_file_call(
            "stub-tool-call-2",
            &join_stub_path(root, "calculator.py"),
            "import tkinter as tk\n\nroot = tk.Tk()\nroot.title('Calculator')\ntk.Label(root, text='Calculator').pack()\nroot.mainloop()\n",
        ),
        stub_create_file_call(
            "stub-tool-call-3",
            &join_stub_path(root, "README.md"),
            "# Calculator\n\nA tiny local Python calculator UI starter.\n",
        ),
    ]
    .into_iter()
    .filter(|call| !call.arguments["target_path"].as_str().unwrap_or("").is_empty())
    .collect()
}

fn stub_react_project_tool_calls(project_root: Option<&str>) -> Vec<RawModelToolCall> {
    let root = project_root.unwrap_or("");
    vec![
        stub_create_directory_call("stub-tool-call-1", root),
        stub_create_directory_call("stub-tool-call-2", &join_stub_path(root, "src")),
        stub_create_file_call(
            "stub-tool-call-3",
            &join_stub_path(root, "package.json"),
            "{\"scripts\":{\"dev\":\"vite\"},\"dependencies\":{},\"devDependencies\":{}}\n",
        ),
        stub_create_file_call(
            "stub-tool-call-4",
            &join_stub_path(root, "src/App.tsx"),
            "export function App() { return <main>Demo</main>; }\n",
        ),
        stub_create_file_call(
            "stub-tool-call-5",
            &join_stub_path(root, "src/main.tsx"),
            "import { App } from './App';\nvoid App;\n",
        ),
        stub_create_file_call(
            "stub-tool-call-6",
            &join_stub_path(root, "README.md"),
            "# Demo Project\n\nCreated with model-first tool calls.\n",
        ),
    ]
    .into_iter()
    .filter(|call| {
        !call.arguments["target_path"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    })
    .collect()
}

fn stub_next_tailwind_project_tool_calls(project_root: Option<&str>) -> Vec<RawModelToolCall> {
    let root = project_root.unwrap_or("");
    vec![
        stub_create_directory_call("stub-tool-call-1", root),
        stub_create_directory_call("stub-tool-call-2", &join_stub_path(root, "app")),
        stub_create_file_call(
            "stub-tool-call-3",
            &join_stub_path(root, "package.json"),
            r#"{"private":true,"scripts":{"dev":"next dev","build":"next build","start":"next start","lint":"next lint"},"dependencies":{"next":"latest","react":"latest","react-dom":"latest"},"devDependencies":{"@types/node":"latest","@types/react":"latest","@types/react-dom":"latest","autoprefixer":"latest","postcss":"latest","tailwindcss":"latest","typescript":"latest"}}
"#,
        ),
        stub_create_file_call(
            "stub-tool-call-4",
            &join_stub_path(root, "tsconfig.json"),
            r#"{"compilerOptions":{"target":"es5","lib":["dom","dom.iterable","esnext"],"allowJs":true,"skipLibCheck":true,"strict":true,"noEmit":true,"esModuleInterop":true,"module":"esnext","moduleResolution":"bundler","resolveJsonModule":true,"isolatedModules":true,"jsx":"preserve","incremental":true,"plugins":[{"name":"next"}]},"include":["next-env.d.ts","**/*.ts","**/*.tsx",".next/types/**/*.ts"],"exclude":["node_modules"]}
"#,
        ),
        stub_create_file_call(
            "stub-tool-call-5",
            &join_stub_path(root, "next-env.d.ts"),
            "/// <reference types=\"next\" />\n/// <reference types=\"next/image-types/global\" />\n\n// This file is generated for the local Next.js TypeScript scaffold.\n",
        ),
        stub_create_file_call(
            "stub-tool-call-6",
            &join_stub_path(root, "next.config.ts"),
            "import type { NextConfig } from 'next';\n\nconst nextConfig: NextConfig = {};\n\nexport default nextConfig;\n",
        ),
        stub_create_file_call(
            "stub-tool-call-7",
            &join_stub_path(root, "postcss.config.js"),
            "module.exports = {\n  plugins: {\n    tailwindcss: {},\n    autoprefixer: {},\n  },\n};\n",
        ),
        stub_create_file_call(
            "stub-tool-call-8",
            &join_stub_path(root, "tailwind.config.ts"),
            "import type { Config } from 'tailwindcss';\n\nconst config: Config = {\n  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}'],\n  theme: { extend: {} },\n  plugins: [],\n};\n\nexport default config;\n",
        ),
        stub_create_file_call(
            "stub-tool-call-9",
            &join_stub_path(root, "app/layout.tsx"),
            "import type { ReactNode } from 'react';\nimport './globals.css';\n\nexport const metadata = {\n  title: 'Elgar Next Tailwind Demo',\n  description: 'A simple Next.js TypeScript Tailwind starter.',\n};\n\nexport default function RootLayout({ children }: { children: ReactNode }) {\n  return (\n    <html lang=\"en\">\n      <body>{children}</body>\n    </html>\n  );\n}\n",
        ),
        stub_create_file_call(
            "stub-tool-call-10",
            &join_stub_path(root, "app/page.tsx"),
            "export default function Home() {\n  return (\n    <main className=\"min-h-screen bg-slate-950 px-6 py-16 text-white\">\n      <section className=\"mx-auto max-w-3xl\">\n        <p className=\"text-sm font-medium uppercase tracking-wide text-cyan-300\">Elgar Demo</p>\n        <h1 className=\"mt-4 text-4xl font-semibold\">Next.js + TypeScript + Tailwind</h1>\n        <p className=\"mt-4 text-slate-300\">A simple starter project created by Elgar.</p>\n      </section>\n    </main>\n  );\n}\n",
        ),
        stub_create_file_call(
            "stub-tool-call-11",
            &join_stub_path(root, "app/globals.css"),
            "@tailwind base;\n@tailwind components;\n@tailwind utilities;\n\n:root {\n  color-scheme: dark;\n}\n\nbody {\n  margin: 0;\n}\n",
        ),
        stub_create_file_call(
            "stub-tool-call-12",
            &join_stub_path(root, "README.md"),
            "# Next.js TypeScript Tailwind Project\n\nA simple Next.js starter with TypeScript and Tailwind CSS.\n\n## Run\n\n```bash\nnpm install\nnpm run dev\n```\n",
        ),
    ]
    .into_iter()
    .filter(|call| {
        !call.arguments["target_path"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    })
    .collect()
}

fn stub_create_directory_call(id: &str, target_path: &str) -> RawModelToolCall {
    RawModelToolCall {
        id: id.to_string(),
        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
        arguments: serde_json::json!({
            "target_path": target_path
        }),
        assistant_summary: Some(format!("create directory {target_path}")),
    }
}

fn stub_create_file_call(id: &str, target_path: &str, contents: &str) -> RawModelToolCall {
    RawModelToolCall {
        id: id.to_string(),
        name: RawModelToolName::Known(ModelToolName::CreateFile),
        arguments: serde_json::json!({
            "target_path": target_path,
            "contents": contents
        }),
        assistant_summary: Some(format!("create {target_path}")),
    }
}

fn extract_stub_project_root(original: &str, lower: &str) -> Option<String> {
    let folder_name = extract_stub_named_value(original, lower)
        .or_else(|| extract_stub_after_marker(original, lower, "project called "))
        .or_else(|| extract_stub_after_marker(original, lower, "project named "))
        .or_else(|| extract_stub_after_marker(original, lower, "app called "))
        .or_else(|| extract_stub_after_marker(original, lower, "app named "))
        .unwrap_or_else(|| "project".to_string());

    if lower.contains("desktop") {
        if let Some(home) = std::env::var_os("HOME") {
            return Some(
                std::path::PathBuf::from(home)
                    .join("Desktop")
                    .join(folder_name)
                    .display()
                    .to_string(),
            );
        }
    }

    Some(folder_name)
}

fn extract_stub_named_value(original: &str, lower: &str) -> Option<String> {
    extract_stub_after_marker(original, lower, "name it ")
        .or_else(|| extract_stub_after_marker(original, lower, "call the folder "))
        .or_else(|| extract_stub_after_marker(original, lower, "called "))
        .or_else(|| extract_stub_after_marker(original, lower, "named "))
}

fn extract_stub_after_marker(original: &str, lower: &str, marker: &str) -> Option<String> {
    let start = lower.find(marker)? + marker.len();
    let rest = original.get(start..)?;
    clean_stub_target_token(rest)
}

fn clean_stub_target_token(value: &str) -> Option<String> {
    let value = value
        .split(',')
        .next()
        .unwrap_or(value)
        .split(" inside ")
        .next()
        .unwrap_or(value)
        .split(" with ")
        .next()
        .unwrap_or(value)
        .trim()
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | '"' | '\''));
    (!value.is_empty()).then(|| value.to_string())
}

fn join_stub_path(root: &str, child: &str) -> String {
    if root.is_empty() {
        return child.to_string();
    }

    std::path::Path::new(root).join(child).display().to_string()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn extract_create_file_target(original: &str, lower: &str) -> Option<String> {
    let marker = ["create file ", "write file "]
        .into_iter()
        .find(|marker| lower.starts_with(marker))?;
    clean_stub_target(&original[marker.len()..])
}

fn extract_create_directory_target(original: &str, lower: &str) -> Option<String> {
    if let Some(folder_name) = extract_stub_home_named_folder(original, lower) {
        return Some(folder_name);
    }

    for marker in [
        "create a folder at ",
        "create folder at ",
        "create a directory at ",
        "create directory at ",
        "create a folder called ",
        "create folder called ",
        "create a directory called ",
        "create directory called ",
        "create a folder named ",
        "create folder named ",
        "create a directory named ",
        "create directory named ",
        "create a folder ",
        "create folder ",
        "create a directory ",
        "create directory ",
    ] {
        if lower.starts_with(marker) {
            return clean_stub_target(&original[marker.len()..]);
        }
    }

    None
}

fn clean_stub_target(value: &str) -> Option<String> {
    let value = value
        .trim()
        .split(" on the desktop")
        .next()
        .unwrap_or(value)
        .split(" in the desktop")
        .next()
        .unwrap_or(value)
        .split(" on desktop")
        .next()
        .unwrap_or(value)
        .split(" in desktop")
        .next()
        .unwrap_or(value)
        .split(" in this repo")
        .next()
        .unwrap_or(value)
        .split(" under this repo")
        .next()
        .unwrap_or(value)
        .split(" inside this repo")
        .next()
        .unwrap_or(value)
        .split(" in the repo")
        .next()
        .unwrap_or(value)
        .split(" in ~/")
        .next()
        .unwrap_or(value)
        .split(" in ~")
        .next()
        .unwrap_or(value)
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | '"' | '\''));
    (!value.is_empty()).then(|| value.to_string())
}

fn extract_stub_home_named_folder(original: &str, lower: &str) -> Option<String> {
    if !contains_any(
        lower,
        &[
            "create a folder",
            "create folder",
            "create a directory",
            "create directory",
        ],
    ) || !contains_any(lower, &["in ~", "inside ~", "under ~", "~/"])
    {
        return None;
    }

    let name = extract_stub_after_marker(original, lower, "call it ")
        .or_else(|| extract_stub_after_marker(original, lower, "called "))
        .or_else(|| extract_stub_after_marker(original, lower, "name it "))
        .or_else(|| extract_stub_after_marker(original, lower, "named "))?;

    Some(format!("~/{name}"))
}

fn extract_shell_command(original: &str) -> Option<String> {
    for prefix in [
        "run shell command ",
        "run command ",
        "run ",
        "shell command ",
    ] {
        if let Some(command) = strip_ascii_case_prefix(original, prefix) {
            let command = command.trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }

    None
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        .then(|| &value[prefix.len()..])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStubResponse {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
    pub output: ProviderOutput,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_tool_target(prompt: &str) -> String {
        let output = ProviderStub::default().ask_with_tools(prompt).output;
        output.tool_calls[0].arguments["target_path"]
            .as_str()
            .expect("tool target")
            .to_string()
    }

    #[test]
    fn stub_folder_create_does_not_treat_project_substring_as_project_request() {
        assert_eq!(
            first_tool_target("i want you to create a folder in ~/ call it myfirstproject"),
            "~/myfirstproject"
        );
    }

    #[test]
    fn stub_desktop_folder_create_strips_location_from_folder_name() {
        assert_eq!(
            first_tool_target("create a folder called test in the desktop"),
            "test"
        );
    }

    #[test]
    fn stub_repo_folder_create_strips_repo_location_from_folder_name() {
        assert_eq!(
            first_tool_target("create a folder called demo in this repo"),
            "demo"
        );
    }

    #[test]
    fn stub_next_tailwind_project_creates_framework_files() {
        let output = ProviderStub::default()
            .ask_with_tools("create a TS Next.js and Tailwind simple project called demo")
            .output;
        let targets = output
            .tool_calls
            .iter()
            .map(|call| call.arguments["target_path"].as_str().unwrap_or(""))
            .collect::<Vec<_>>();

        for expected in [
            "demo/package.json",
            "demo/tsconfig.json",
            "demo/next-env.d.ts",
            "demo/next.config.ts",
            "demo/postcss.config.js",
            "demo/tailwind.config.ts",
            "demo/app/layout.tsx",
            "demo/app/page.tsx",
            "demo/app/globals.css",
            "demo/README.md",
        ] {
            assert!(targets.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn stub_project_request_accepts_call_the_folder_name() {
        let output = ProviderStub::default()
            .ask_with_tools("create a react project using tailwind and TS, call the folder TEST")
            .output;
        let targets = output
            .tool_calls
            .iter()
            .map(|call| call.arguments["target_path"].as_str().unwrap_or(""))
            .collect::<Vec<_>>();

        assert!(targets.contains(&"TEST/package.json"));
        assert!(targets.contains(&"TEST/src/App.tsx"));
    }

    #[test]
    fn stub_messages_use_recent_context_for_short_plan_followup() {
        let output = ProviderStub::default()
            .chat_messages_with_tools_with_metadata(
                vec![
                    crate::provider::ChatMessage::system(
                        "Recent conversation context:\nUser: create a plan for a python and ts project in the last folder you created\nVerified filesystem context:\n- latest verified folder: helloworld",
                    ),
                    crate::provider::ChatMessage::user("your choice"),
                ],
                &ProviderRequestMetadata::new("stub-provider", None, "stub-request-1"),
                Vec::new(),
            )
            .unwrap();

        assert_eq!(
            output.tool_calls[0].arguments["target_path"].as_str(),
            Some("python-ts-project-plan.md")
        );
    }

    #[test]
    fn stub_short_followup_does_not_recreate_existing_plan() {
        let output = ProviderStub::default()
            .chat_messages_with_tools_with_metadata(
                vec![
                    crate::provider::ChatMessage::system(
                        "Recent conversation context:\nUser: create a plan for a python and ts project in the last folder you created\nVerified filesystem context:\n- latest verified folder: helloworld\n- latest verified plan: helloworld/python-ts-project-plan.md",
                    ),
                    crate::provider::ChatMessage::user("your choice"),
                ],
                &ProviderRequestMetadata::new("stub-provider", None, "stub-request-1"),
                Vec::new(),
            )
            .unwrap();

        assert!(output.tool_calls.is_empty());
        assert!(output.text.contains("latest plan"));
    }

    #[test]
    fn stub_read_plan_question_does_not_implement_project() {
        let output = ProviderStub::default()
            .ask_with_tools("whats the plan i asked you to create??")
            .output;

        assert!(output.tool_calls.is_empty());
        assert!(output.text.contains("latest plan"));
    }
}
