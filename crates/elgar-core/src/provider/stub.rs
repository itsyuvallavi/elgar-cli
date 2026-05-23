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

    if let Some(output) = deterministic_stub_project_tool_output(normalized, &lower) {
        return Some(output);
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

    let (file_name, title) = if lower.contains("react ts") || lower.contains("react typescript") {
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

fn deterministic_stub_project_tool_output(original: &str, lower: &str) -> Option<ProviderOutput> {
    if !contains_any(lower, &["project", "app", "calculator"]) {
        return None;
    }
    if !contains_any(lower, &["create", "implement", "make", "build", "scaffold"]) {
        return None;
    }

    let prior_followup = contains_any(
        lower,
        &[
            "the plan",
            "you planned",
            "we planned",
            "rest of the project",
            "implement the plan",
        ],
    );
    let project_root = (!prior_followup)
        .then(|| extract_stub_project_root(original, lower))
        .flatten();

    let calls = if lower.contains("calculator") || lower.contains("python") {
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
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | '"' | '\''));
    (!value.is_empty()).then(|| value.to_string())
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
