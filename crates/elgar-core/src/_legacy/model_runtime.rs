use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::action::{
    ActionRequest, CreateDirectoryAction, CreateFileAction, DeleteFileAction, MoveFileAction,
    OverwriteFileAction, PatchFileAction, ShellCommandAction, SHELL_COMMAND_MAX_TIMEOUT_SECONDS,
};
use crate::provider::ChatToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelToolName {
    AskGuidance,
    CreateFiles,
    CreateFile,
    CreateDirectory,
    OverwriteFile,
    PatchFile,
    DeleteFile,
    MoveFile,
    ShellCommand,
}

impl ModelToolName {
    fn label(self) -> &'static str {
        match self {
            Self::AskGuidance => "ask_guidance",
            Self::CreateFiles => "create_files",
            Self::CreateFile => "create_file",
            Self::CreateDirectory => "create_directory",
            Self::OverwriteFile => "overwrite_file",
            Self::PatchFile => "patch_file",
            Self::DeleteFile => "delete_file",
            Self::MoveFile => "move_file",
            Self::ShellCommand => "shell_command",
        }
    }
}

pub fn elgar_model_tool_definitions() -> Vec<ChatToolDefinition> {
    vec![
        model_tool_definition(
            ModelToolName::AskGuidance,
            "Ask the user one concise clarification question when target, scope, verified memory, or safe next step is ambiguous. This never creates an action or mutates files.",
            object_parameters(
                &[
                    (
                        "question",
                        string_property("One concise question for the user."),
                    ),
                    (
                        "reason",
                        string_property("Optional short reason why guidance is needed."),
                    ),
                ],
                &["question"],
            ),
        ),
        model_tool_definition(
            ModelToolName::CreateFiles,
            "Draft creation of multiple new directories and files in one validated batch. Prefer this for small project scaffolds and verified-plan execution when several expected paths are missing.",
            object_parameters(
                &[
                    (
                        "directories",
                        array_property(
                            "Optional directory paths to create before files.",
                            string_property("Directory path to create."),
                        ),
                    ),
                    (
                        "files",
                        array_property(
                            "Files to create.",
                            object_parameters(
                                &[
                                    (
                                        "target_path",
                                        string_property("Path for the new file."),
                                    ),
                                    ("contents", string_property("Full contents to write.")),
                                ],
                                &["target_path", "contents"],
                            ),
                        ),
                    ),
                ],
                &["files"],
            ),
        ),
        model_tool_definition(
            ModelToolName::CreateFile,
            "Draft actual creation of a new file under an allowed project path. Use for project/file creation instead of prose-only file contents.",
            object_parameters(
                &[
                    (
                        "target_path",
                        string_property(
                            "Path for the new file. Use the user's explicit Desktop path/name when provided; otherwise use a project-relative path.",
                        ),
                    ),
                    ("contents", string_property("Full contents to write.")),
                ],
                &["target_path", "contents"],
            ),
        ),
        model_tool_definition(
            ModelToolName::CreateDirectory,
            "Draft actual creation of a new directory under an allowed project path. Use for project/folder creation instead of prose-only instructions.",
            object_parameters(
                &[(
                    "target_path",
                    string_property(
                        "Path for the new directory. Use the user's explicit Desktop path/name when provided; otherwise use a project-relative path.",
                    ),
                )],
                &["target_path"],
            ),
        ),
        model_tool_definition(
            ModelToolName::OverwriteFile,
            "Draft full replacement of an existing file.",
            object_parameters(
                &[
                    (
                        "target_path",
                        string_property("Project-relative path for the file to overwrite."),
                    ),
                    ("contents", string_property("Replacement file contents.")),
                ],
                &["target_path", "contents"],
            ),
        ),
        model_tool_definition(
            ModelToolName::PatchFile,
            "Draft a simple find-and-replace patch for an existing file.",
            object_parameters(
                &[
                    (
                        "target_path",
                        string_property("Project-relative path for the file to patch."),
                    ),
                    (
                        "find",
                        string_property("Non-empty exact text to replace in the file."),
                    ),
                    ("replace", string_property("Replacement text.")),
                ],
                &["target_path", "find", "replace"],
            ),
        ),
        model_tool_definition(
            ModelToolName::DeleteFile,
            "Draft deletion of a file under an allowed project path.",
            object_parameters(
                &[(
                    "target_path",
                    string_property("Project-relative path for the file to delete."),
                )],
                &["target_path"],
            ),
        ),
        model_tool_definition(
            ModelToolName::MoveFile,
            "Draft moving or renaming a file under allowed project paths.",
            object_parameters(
                &[
                    (
                        "source_path",
                        string_property("Project-relative path for the existing file."),
                    ),
                    (
                        "target_path",
                        string_property("Project-relative destination path."),
                    ),
                ],
                &["source_path", "target_path"],
            ),
        ),
        model_tool_definition(
            ModelToolName::ShellCommand,
            "Draft a bounded shell command for explicit review before execution. Use this for local inspection commands such as cat, ls, pwd, test, build, lint, compile, or dependency install. Do not use this for long-running dev servers/watchers such as npm run dev, next dev, vite --host, or python -m http.server; ask for guidance or use a future background process mode instead. For read-only inspection commands, leave expected_file and expected_directory empty; stdout and exit status are the proof. For compile, test, lint, or verification commands, rely on exit status and optional expected_effect; do not use expected_file for generated caches or bytecode artifacts.",
            object_parameters(
                &[
                    (
                        "command",
                        string_property("Non-empty shell command to run."),
                    ),
                    (
                        "cwd",
                        string_property("Project-relative working directory."),
                    ),
                    (
                        "timeout_seconds",
                        integer_property(
                            "Optional unsigned timeout in seconds.",
                            Some(SHELL_COMMAND_MAX_TIMEOUT_SECONDS),
                        ),
                    ),
                    (
                        "expected_effect",
                        string_property(
                            "Optional expected command effect. Use this for compile/test/lint verification when exit status is the proof.",
                        ),
                    ),
                    (
                        "risk_notes",
                        string_property("Optional risk notes for reviewer context."),
                    ),
                    (
                        "expected_file",
                        string_property(
                            "Optional durable project-relative file expected after execution. Leave empty for read-only inspection commands like cat or ls. Do not use for generated caches, Python bytecode, or other unstable verification artifacts.",
                        ),
                    ),
                    (
                        "expected_directory",
                        string_property(
                            "Optional project-relative directory expected after execution.",
                        ),
                    ),
                ],
                &["command", "cwd"],
            ),
        ),
    ]
}

pub fn elgar_model_tool_definitions_for(tool_names: &[ModelToolName]) -> Vec<ChatToolDefinition> {
    let labels = tool_names
        .iter()
        .map(|name| name.label())
        .collect::<Vec<_>>();
    elgar_model_tool_definitions()
        .into_iter()
        .filter(|definition| labels.contains(&definition.function.name.as_str()))
        .collect()
}

fn model_tool_definition(
    name: ModelToolName,
    description: &'static str,
    parameters: Value,
) -> ChatToolDefinition {
    ChatToolDefinition::function(name.label(), description, parameters)
}

fn object_parameters(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let properties = properties
        .iter()
        .map(|(name, schema)| ((*name).to_string(), schema.clone()))
        .collect::<Map<_, _>>();

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn string_property(description: &'static str) -> Value {
    json!({
        "type": "string",
        "description": description
    })
}

fn integer_property(description: &'static str, maximum: Option<u64>) -> Value {
    let mut schema = json!({
        "type": "integer",
        "minimum": 0,
        "description": description
    });
    if let Some(maximum) = maximum {
        schema["maximum"] = json!(maximum);
    }
    schema
}

fn array_property(description: &'static str, items: Value) -> Value {
    json!({
        "type": "array",
        "description": description,
        "items": items
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawModelToolName {
    Known(ModelToolName),
    Unknown(String),
}

impl RawModelToolName {
    fn known(&self) -> Result<ModelToolName, ModelToolValidationError> {
        match self {
            Self::Known(name) => Ok(*name),
            Self::Unknown(name) => Err(ModelToolValidationError::unknown_tool_name(name.clone())),
        }
    }

    pub fn raw_label(&self) -> String {
        match self {
            Self::Known(name) => name.label().to_string(),
            Self::Unknown(name) => name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawModelToolCall {
    pub id: String,
    pub name: RawModelToolName,
    pub arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedModelToolAction {
    pub tool_call_id: String,
    pub request: ActionRequest,
    pub summary: String,
    pub target_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedModelGuidanceRequest {
    pub tool_call_id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ValidatedModelToolOutput {
    Action(ValidatedModelToolAction),
    Guidance(ValidatedModelGuidanceRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelToolValidationErrorKind {
    MultipleToolCalls,
    UnknownToolName,
    MissingArgument,
    MalformedArgument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelToolValidationError {
    pub kind: ModelToolValidationErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
}

impl ModelToolValidationError {
    fn multiple_tool_calls(count: usize) -> Self {
        Self {
            kind: ModelToolValidationErrorKind::MultipleToolCalls,
            message: format!("expected at most one model tool call draft, got {count}"),
            tool_call_id: None,
            tool_name: None,
            argument: None,
        }
    }

    fn unknown_tool_name(tool_name: String) -> Self {
        Self {
            kind: ModelToolValidationErrorKind::UnknownToolName,
            message: format!("unknown model tool name `{tool_name}`"),
            tool_call_id: None,
            tool_name: Some(tool_name),
            argument: None,
        }
    }

    fn missing_argument(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        argument: impl Into<String>,
    ) -> Self {
        let tool_name = tool_name.into();
        let argument = argument.into();
        Self {
            kind: ModelToolValidationErrorKind::MissingArgument,
            message: format!("model tool `{tool_name}` is missing required argument `{argument}`"),
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name),
            argument: Some(argument),
        }
    }

    fn malformed_argument(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        argument: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        let tool_name = tool_name.into();
        let argument = argument.into();
        Self {
            kind: ModelToolValidationErrorKind::MalformedArgument,
            message: format!(
                "model tool `{tool_name}` argument `{argument}` must be {}",
                expected.into()
            ),
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name),
            argument: Some(argument),
        }
    }
}

pub fn validate_exactly_one_model_tool_call(
    tool_calls: &[RawModelToolCall],
) -> Result<Option<ValidatedModelToolAction>, ModelToolValidationError> {
    match tool_calls {
        [] => Ok(None),
        [tool_call] => {
            let outputs = validate_model_tool_call(tool_call)?;
            match outputs.as_slice() {
                [ValidatedModelToolOutput::Action(action)] => Ok(Some(action.clone())),
                [ValidatedModelToolOutput::Guidance(_)] => Ok(None),
                outputs => Err(ModelToolValidationError::multiple_tool_calls(outputs.len())),
            }
        }
        calls => Err(ModelToolValidationError::multiple_tool_calls(calls.len())),
    }
}

pub fn validate_model_tool_calls(
    tool_calls: &[RawModelToolCall],
) -> Result<Vec<ValidatedModelToolAction>, ModelToolValidationError> {
    validate_model_tool_outputs(tool_calls).map(|outputs| {
        outputs
            .into_iter()
            .filter_map(|output| match output {
                ValidatedModelToolOutput::Action(action) => Some(action),
                ValidatedModelToolOutput::Guidance(_) => None,
            })
            .collect()
    })
}

pub fn validate_model_tool_outputs(
    tool_calls: &[RawModelToolCall],
) -> Result<Vec<ValidatedModelToolOutput>, ModelToolValidationError> {
    let mut outputs = Vec::new();
    for tool_call in tool_calls {
        outputs.extend(validate_model_tool_call(tool_call)?);
    }
    Ok(outputs)
}

pub fn validate_provider_prose_only(
) -> Result<Option<ValidatedModelToolAction>, ModelToolValidationError> {
    validate_exactly_one_model_tool_call(&[])
}

fn validate_model_tool_call(
    tool_call: &RawModelToolCall,
) -> Result<Vec<ValidatedModelToolOutput>, ModelToolValidationError> {
    let tool_name = tool_call.name.known().map_err(|mut error| {
        error.tool_call_id = Some(tool_call.id.clone());
        error
    })?;
    let arguments = arguments_object(tool_call)?;
    if tool_name == ModelToolName::AskGuidance {
        return Ok(vec![ValidatedModelToolOutput::Guidance(
            ValidatedModelGuidanceRequest {
                tool_call_id: tool_call.id.clone(),
                question: required_non_empty_string(tool_call, arguments, tool_name, "question")?,
                reason: optional_non_empty_string(tool_call, arguments, tool_name, "reason")?,
            },
        )]);
    }
    if tool_name == ModelToolName::CreateFiles {
        return validate_create_files(tool_call, arguments, tool_name);
    }

    let request = match tool_name {
        ModelToolName::AskGuidance => unreachable!("ask_guidance returned before action parsing"),
        ModelToolName::CreateFiles => {
            unreachable!("create_files returned before single action parsing")
        }
        ModelToolName::CreateFile => ActionRequest::CreateFile(CreateFileAction {
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
            contents: required_string(tool_call, arguments, tool_name, "contents")?,
        }),
        ModelToolName::CreateDirectory => ActionRequest::CreateDirectory(CreateDirectoryAction {
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
        }),
        ModelToolName::OverwriteFile => ActionRequest::OverwriteFile(OverwriteFileAction {
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
            contents: required_string(tool_call, arguments, tool_name, "contents")?,
        }),
        ModelToolName::PatchFile => ActionRequest::PatchFile(PatchFileAction {
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
            find: required_non_empty_string(tool_call, arguments, tool_name, "find")?,
            replace: required_string(tool_call, arguments, tool_name, "replace")?,
        }),
        ModelToolName::DeleteFile => ActionRequest::DeleteFile(DeleteFileAction {
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
        }),
        ModelToolName::MoveFile => ActionRequest::MoveFile(MoveFileAction {
            source_path: required_path(tool_call, arguments, tool_name, "source_path")?,
            target_path: required_path(tool_call, arguments, tool_name, "target_path")?,
        }),
        ModelToolName::ShellCommand => validate_shell_command(tool_call, arguments, tool_name)?,
    };
    let target_label = request.approval_target();
    let summary = tool_call
        .assistant_summary
        .clone()
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or_else(|| format!("Drafted {} action for {target_label}", tool_name.label()));

    Ok(vec![ValidatedModelToolOutput::Action(
        ValidatedModelToolAction {
            tool_call_id: tool_call.id.clone(),
            request,
            summary,
            target_label,
        },
    )])
}

fn validate_create_files(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
) -> Result<Vec<ValidatedModelToolOutput>, ModelToolValidationError> {
    let mut outputs = Vec::new();

    if let Some(directories) = optional_array(tool_call, arguments, tool_name, "directories")? {
        for (index, value) in directories.iter().enumerate() {
            let Some(path) = value.as_str().filter(|value| !value.trim().is_empty()) else {
                return Err(ModelToolValidationError::malformed_argument(
                    tool_call.id.clone(),
                    tool_name.label(),
                    "directories",
                    "an array of non-empty path strings",
                ));
            };
            if path_contains_ellipsis_placeholder(path) {
                return Err(ModelToolValidationError::malformed_argument(
                    tool_call.id.clone(),
                    tool_name.label(),
                    "directories",
                    "complete paths without ellipsis placeholders",
                ));
            }
            let request = ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: PathBuf::from(path),
            });
            let target_label = request.approval_target();
            outputs.push(ValidatedModelToolOutput::Action(ValidatedModelToolAction {
                tool_call_id: tool_call.id.clone(),
                request,
                summary: format!(
                    "Drafted create_files directory action {index} for {target_label}"
                ),
                target_label,
            }));
        }
    }

    let files = required_array(tool_call, arguments, tool_name, "files")?;
    if files.is_empty() {
        return Err(ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            "files",
            "a non-empty array of file objects",
        ));
    }
    for (index, value) in files.iter().enumerate() {
        let Some(file) = value.as_object() else {
            return Err(ModelToolValidationError::malformed_argument(
                tool_call.id.clone(),
                tool_name.label(),
                "files",
                "an array of file objects",
            ));
        };
        let target_path = string_field_from_object(tool_call, tool_name, file, "target_path")?;
        if path_contains_ellipsis_placeholder(&target_path) {
            return Err(ModelToolValidationError::malformed_argument(
                tool_call.id.clone(),
                tool_name.label(),
                "target_path",
                "a complete path without ellipsis placeholders",
            ));
        }
        let contents = string_field_from_object(tool_call, tool_name, file, "contents")?;
        let request = ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from(target_path),
            contents,
        });
        let target_label = request.approval_target();
        outputs.push(ValidatedModelToolOutput::Action(ValidatedModelToolAction {
            tool_call_id: tool_call.id.clone(),
            request,
            summary: format!("Drafted create_files file action {index} for {target_label}"),
            target_label,
        }));
    }

    Ok(outputs)
}

fn validate_shell_command(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
) -> Result<ActionRequest, ModelToolValidationError> {
    let command = required_non_empty_string(tool_call, arguments, tool_name, "command")?;
    if command_contains_long_running_server(&command) {
        return Err(ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            "command",
            "a bounded command that exits; long-running dev servers/watchers are not supported by shell_command yet",
        ));
    }
    let cwd = required_path(tool_call, arguments, tool_name, "cwd")?;
    let mut action = ShellCommandAction::new(command, cwd);

    if let Some(timeout_seconds) = optional_u64(tool_call, arguments, tool_name, "timeout_seconds")?
    {
        action.timeout_seconds = timeout_seconds.min(SHELL_COMMAND_MAX_TIMEOUT_SECONDS);
    }
    if let Some(expected_effect) =
        optional_string(tool_call, arguments, tool_name, "expected_effect")?
    {
        action.expected_effect = expected_effect;
    }
    if let Some(risk_notes) = optional_string(tool_call, arguments, tool_name, "risk_notes")? {
        action.risk_notes = risk_notes;
    }
    if let Some(expected_file) = optional_shell_path(arguments, "expected_file") {
        action.expected_file = Some(expected_file);
    }
    if let Some(expected_directory) = optional_shell_path(arguments, "expected_directory") {
        action.expected_directory = Some(expected_directory);
    }

    Ok(ActionRequest::ShellCommand(action))
}

fn optional_shell_path(arguments: &Map<String, Value>, key: &str) -> Option<PathBuf> {
    let value = arguments.get(key)?.as_str()?.trim();
    if value.is_empty() || path_contains_ellipsis_placeholder(value) {
        return None;
    }
    Some(PathBuf::from(value))
}

fn command_contains_long_running_server(command: &str) -> bool {
    command
        .split([';', '\n'])
        .flat_map(|segment| segment.split("&&"))
        .flat_map(|segment| segment.split("||"))
        .any(segment_is_long_running_server)
}

fn segment_is_long_running_server(segment: &str) -> bool {
    let tokens = shell_words(segment);
    let words = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        ["npm", "run", "dev", ..]
        | ["npm", "start", ..]
        | ["npm", "run", "start", ..]
        | ["pnpm", "dev", ..]
        | ["pnpm", "run", "dev", ..]
        | ["pnpm", "start", ..]
        | ["yarn", "dev", ..]
        | ["yarn", "start", ..]
        | ["yarn", "run", "dev", ..]
        | ["bun", "dev", ..]
        | ["bun", "run", "dev", ..]
        | ["next", "dev", ..]
        | ["vite", ..]
        | ["python", "-m", "http.server", ..]
        | ["python3", "-m", "http.server", ..]
        | ["python", "-m", "uvicorn", ..]
        | ["python3", "-m", "uvicorn", ..]
        | ["uvicorn", ..]
        | ["flask", "run", ..]
        | ["cargo", "watch", ..] => true,
        [first, ..] if first.ends_with("/next") && words.get(1) == Some(&"dev") => true,
        [first, ..] if first.ends_with("/vite") => true,
        _ => false,
    }
}

fn shell_words(segment: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escape = false;

    for character in segment.chars() {
        if escape {
            current.push(character);
            escape = false;
            continue;
        }
        if character == '\\' {
            escape = true;
            continue;
        }
        if matches!(quote, Some(active) if active == character) {
            quote = None;
            continue;
        }
        if quote.is_none() && (character == '\'' || character == '"') {
            quote = Some(character);
            continue;
        }
        if quote.is_none() && character.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(character);
    }

    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn arguments_object(
    tool_call: &RawModelToolCall,
) -> Result<&Map<String, Value>, ModelToolValidationError> {
    tool_call.arguments.as_object().ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_call.name.raw_label(),
            "arguments",
            "a JSON object",
        )
    })
}

fn required_path(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<PathBuf, ModelToolValidationError> {
    let value = required_non_empty_string(tool_call, arguments, tool_name, key)?;
    if path_contains_ellipsis_placeholder(&value) {
        return Err(ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "a complete path without ellipsis placeholders",
        ));
    }
    Ok(PathBuf::from(value))
}

fn path_contains_ellipsis_placeholder(value: &str) -> bool {
    value
        .split(['/', '\\'])
        .any(|component| component.contains("...") || component.contains('…'))
}

fn required_string(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<String, ModelToolValidationError> {
    let Some(value) = arguments.get(key) else {
        return Err(ModelToolValidationError::missing_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
        ));
    };
    value.as_str().map(ToString::to_string).ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "a string",
        )
    })
}

fn required_non_empty_string(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<String, ModelToolValidationError> {
    let value = required_string(tool_call, arguments, tool_name, key)?;
    if value.trim().is_empty() {
        return Err(ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "a non-empty string",
        ));
    }
    Ok(value)
}

fn optional_string(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<Option<String>, ModelToolValidationError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| {
            ModelToolValidationError::malformed_argument(
                tool_call.id.clone(),
                tool_name.label(),
                key,
                "a string",
            )
        })
}

fn optional_non_empty_string(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<Option<String>, ModelToolValidationError> {
    let Some(value) = optional_string(tool_call, arguments, tool_name, key)? else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Err(ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "a non-empty string",
        ));
    }
    Ok(Some(value))
}

fn optional_u64(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<Option<u64>, ModelToolValidationError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    value.as_u64().map(Some).ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "an unsigned integer",
        )
    })
}

fn required_array<'a>(
    tool_call: &RawModelToolCall,
    arguments: &'a Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<&'a Vec<Value>, ModelToolValidationError> {
    let Some(value) = arguments.get(key) else {
        return Err(ModelToolValidationError::missing_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
        ));
    };
    value.as_array().ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "an array",
        )
    })
}

fn optional_array<'a>(
    tool_call: &RawModelToolCall,
    arguments: &'a Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<Option<&'a Vec<Value>>, ModelToolValidationError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    value.as_array().map(Some).ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "an array",
        )
    })
}

fn string_field_from_object(
    tool_call: &RawModelToolCall,
    tool_name: ModelToolName,
    object: &Map<String, Value>,
    key: &str,
) -> Result<String, ModelToolValidationError> {
    let Some(value) = object.get(key) else {
        return Err(ModelToolValidationError::missing_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
        ));
    };
    value.as_str().map(ToString::to_string).ok_or_else(|| {
        ModelToolValidationError::malformed_argument(
            tool_call.id.clone(),
            tool_name.label(),
            key,
            "a string",
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        elgar_model_tool_definitions, validate_exactly_one_model_tool_call,
        validate_model_tool_calls, validate_model_tool_outputs, validate_provider_prose_only,
        ModelToolName, ModelToolValidationErrorKind, RawModelToolCall, RawModelToolName,
        ValidatedModelToolOutput,
    };
    use crate::action::{
        ActionRequest, SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS, SHELL_COMMAND_MAX_TIMEOUT_SECONDS,
    };
    use crate::provider::ChatToolDefinition;

    #[test]
    fn model_tool_name_serde_names_roundtrip() {
        let value = serde_json::to_value([
            ModelToolName::AskGuidance,
            ModelToolName::CreateFiles,
            ModelToolName::CreateFile,
            ModelToolName::CreateDirectory,
            ModelToolName::OverwriteFile,
            ModelToolName::PatchFile,
            ModelToolName::DeleteFile,
            ModelToolName::MoveFile,
            ModelToolName::ShellCommand,
        ])
        .expect("serialize model tool names");

        assert_eq!(
            value,
            json!([
                "ask_guidance",
                "create_files",
                "create_file",
                "create_directory",
                "overwrite_file",
                "patch_file",
                "delete_file",
                "move_file",
                "shell_command"
            ])
        );

        let names: Vec<ModelToolName> =
            serde_json::from_value(value).expect("deserialize model tool names");
        assert_eq!(
            names,
            vec![
                ModelToolName::AskGuidance,
                ModelToolName::CreateFiles,
                ModelToolName::CreateFile,
                ModelToolName::CreateDirectory,
                ModelToolName::OverwriteFile,
                ModelToolName::PatchFile,
                ModelToolName::DeleteFile,
                ModelToolName::MoveFile,
                ModelToolName::ShellCommand
            ]
        );
    }

    #[test]
    fn tool_definition_names_match_model_tool_name_serde_names() {
        let definitions = elgar_model_tool_definitions();
        let actual = definitions
            .iter()
            .map(|definition| definition.function.name.as_str())
            .collect::<Vec<_>>();
        let expected_value = serde_json::to_value([
            ModelToolName::AskGuidance,
            ModelToolName::CreateFiles,
            ModelToolName::CreateFile,
            ModelToolName::CreateDirectory,
            ModelToolName::OverwriteFile,
            ModelToolName::PatchFile,
            ModelToolName::DeleteFile,
            ModelToolName::MoveFile,
            ModelToolName::ShellCommand,
        ])
        .expect("serialize model tool names");
        let expected = expected_value
            .as_array()
            .expect("tool names array")
            .iter()
            .map(|value| value.as_str().expect("tool name string"))
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn create_files_tool_expands_to_directory_and_file_actions() {
        let outputs = validate_model_tool_outputs(&[raw_call(
            "batch-1",
            RawModelToolName::Known(ModelToolName::CreateFiles),
            json!({
                "directories": ["demo/src"],
                "files": [
                    {
                        "target_path": "demo/README.md",
                        "contents": "# Demo\n"
                    },
                    {
                        "target_path": "demo/src/main.py",
                        "contents": "print('demo')\n"
                    }
                ]
            }),
        )])
        .expect("validate create_files");

        assert_eq!(outputs.len(), 3);
        assert!(matches!(
            &outputs[0],
            ValidatedModelToolOutput::Action(action)
                if matches!(action.request, ActionRequest::CreateDirectory(_))
                    && action.tool_call_id == "batch-1"
        ));
        assert!(matches!(
            &outputs[1],
            ValidatedModelToolOutput::Action(action)
                if matches!(action.request, ActionRequest::CreateFile(_))
                    && action.tool_call_id == "batch-1"
        ));
        assert!(matches!(
            &outputs[2],
            ValidatedModelToolOutput::Action(action)
                if matches!(action.request, ActionRequest::CreateFile(_))
                    && action.tool_call_id == "batch-1"
        ));
    }

    #[test]
    fn create_file_tool_schema_requires_target_path_and_contents() {
        let schema = tool_parameters("create_file");

        assert_eq!(required_names(&schema), vec!["target_path", "contents"]);
        assert!(schema["properties"].get("target_path").is_some());
        assert!(schema["properties"].get("contents").is_some());
    }

    #[test]
    fn ask_guidance_tool_schema_requires_question_only() {
        let schema = tool_parameters("ask_guidance");

        assert_eq!(required_names(&schema), vec!["question"]);
        assert!(schema["properties"].get("question").is_some());
        assert!(schema["properties"].get("reason").is_some());
    }

    #[test]
    fn shell_command_tool_schema_includes_required_and_optional_fields() {
        let schema = tool_parameters("shell_command");

        assert_eq!(required_names(&schema), vec!["command", "cwd"]);
        assert!(schema["properties"].get("command").is_some());
        assert!(schema["properties"].get("cwd").is_some());
        assert!(schema["properties"].get("timeout_seconds").is_some());
        assert_eq!(schema["properties"]["timeout_seconds"]["maximum"], 900);
        assert!(schema["properties"].get("expected_effect").is_some());
        assert!(schema["properties"].get("risk_notes").is_some());
        assert!(schema["properties"].get("expected_file").is_some());
        assert!(schema["properties"].get("expected_directory").is_some());
    }

    #[test]
    fn shell_command_tool_schema_steers_verification_away_from_cache_files() {
        let definition = tool_definition("shell_command");
        assert!(definition
            .function
            .description
            .contains("compile, test, lint, or verification"));

        let schema = definition.function.parameters;
        assert!(schema["properties"]["expected_effect"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("exit status is the proof")));
        assert!(schema["properties"]["expected_file"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("Do not use for generated caches")));
        assert!(schema["properties"]["expected_file"]["description"]
            .as_str()
            .is_some_and(
                |description| description.contains("Leave empty for read-only inspection commands")
            ));
    }

    #[test]
    fn valid_create_file_validates_to_action_request() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": "hello.py",
                "contents": "print('hello')\n"
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate create_file")
            .expect("one validated action");

        assert_eq!(validated.tool_call_id, "tool-1");
        assert_eq!(validated.summary, "Drafted create_file action for hello.py");
        assert_eq!(validated.target_label, "hello.py");
        let ActionRequest::CreateFile(action) = validated.request else {
            panic!("expected CreateFile");
        };
        assert_eq!(action.target_path, PathBuf::from("hello.py"));
        assert_eq!(action.contents, "print('hello')\n");
    }

    #[test]
    fn valid_ask_guidance_validates_without_action_request() {
        let draft = raw_call(
            "tool-guidance",
            RawModelToolName::Known(ModelToolName::AskGuidance),
            json!({
                "question": "Which folder should I use?",
                "reason": "No verified folder is available."
            }),
        );

        let outputs = validate_model_tool_outputs(&[draft]).expect("validate ask_guidance");

        assert_eq!(outputs.len(), 1);
        let ValidatedModelToolOutput::Guidance(guidance) = &outputs[0] else {
            panic!("expected guidance");
        };
        assert_eq!(guidance.tool_call_id, "tool-guidance");
        assert_eq!(guidance.question, "Which folder should I use?");
        assert_eq!(
            guidance.reason.as_deref(),
            Some("No verified folder is available.")
        );
    }

    #[test]
    fn valid_create_directory_validates_to_action_request() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({
                "target_path": "src/generated"
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate create_directory")
            .expect("one validated action");

        let ActionRequest::CreateDirectory(action) = validated.request else {
            panic!("expected CreateDirectory");
        };
        assert_eq!(validated.target_label, "src/generated");
        assert_eq!(action.target_path, PathBuf::from("src/generated"));
    }

    #[test]
    fn path_arguments_reject_ellipsis_placeholders() {
        let error = validate_model_tool_calls(&[raw_call(
            "call-truncated",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "/Users/yuval/next-tailwind-...." }),
        )])
        .expect_err("truncated display paths must not become filesystem targets");

        assert_eq!(error.kind, ModelToolValidationErrorKind::MalformedArgument);
        assert_eq!(error.argument.as_deref(), Some("target_path"));
        assert!(error.message.contains("complete path"));
    }

    #[test]
    fn valid_shell_command_validates_to_action_request_with_defaults() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({
                "command": "cargo test -p elgar-core",
                "cwd": ".",
                "timeout_seconds": 120,
                "expected_effect": "Run core tests.",
                "risk_notes": "May run local test binaries.",
                "expected_file": "target/debug/deps/elgar_core",
                "expected_directory": "target"
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate shell_command")
            .expect("one validated action");

        let ActionRequest::ShellCommand(action) = validated.request else {
            panic!("expected ShellCommand");
        };
        assert_eq!(validated.target_label, "cargo test -p elgar-core");
        assert_eq!(action.command, "cargo test -p elgar-core");
        assert_eq!(action.cwd, PathBuf::from("."));
        assert_eq!(action.timeout_seconds, 120);
        assert_eq!(action.expected_effect, "Run core tests.");
        assert_eq!(action.risk_notes, "May run local test binaries.");
        assert_eq!(
            action.expected_file,
            Some(PathBuf::from("target/debug/deps/elgar_core"))
        );
        assert_eq!(action.expected_directory, Some(PathBuf::from("target")));
    }

    #[test]
    fn shell_command_omitted_optional_fields_use_existing_defaults() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({
                "command": "cargo test",
                "cwd": "."
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate shell_command")
            .expect("one validated action");

        let ActionRequest::ShellCommand(action) = validated.request else {
            panic!("expected ShellCommand");
        };
        assert_eq!(
            action.timeout_seconds,
            SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS
        );
        assert!(action.expected_effect.contains("Run `cargo test`"));
        assert!(action.risk_notes.contains("Shell commands are high risk"));
        assert_eq!(action.expected_file, None);
        assert_eq!(action.expected_directory, None);
    }

    #[test]
    fn shell_command_ignores_malformed_optional_expected_paths() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({
                "command": "cat package.json",
                "cwd": ".",
                "expected_file": ["package.json"],
                "expected_directory": {"path": "."}
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("malformed optional shell verification hints should not reject command")
            .expect("one validated action");

        let ActionRequest::ShellCommand(action) = validated.request else {
            panic!("expected ShellCommand");
        };
        assert_eq!(action.command, "cat package.json");
        assert_eq!(action.expected_file, None);
        assert_eq!(action.expected_directory, None);
    }

    #[test]
    fn shell_command_timeout_is_capped_to_runtime_maximum() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({
                "command": "sleep 999",
                "cwd": ".",
                "timeout_seconds": 999_999
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate shell_command")
            .expect("one validated action");

        let ActionRequest::ShellCommand(action) = validated.request else {
            panic!("expected ShellCommand");
        };
        assert_eq!(action.timeout_seconds, SHELL_COMMAND_MAX_TIMEOUT_SECONDS);
    }

    #[test]
    fn shell_command_rejects_long_running_dev_server_commands() {
        let error = validate_exactly_one_model_tool_call(&[raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({
                "command": "npm install && npm run dev",
                "cwd": "Nextjs-1",
                "timeout_seconds": 300
            }),
        )])
        .expect_err("dev server commands must not become bounded shell actions");

        assert_eq!(error.kind, ModelToolValidationErrorKind::MalformedArgument);
        assert_eq!(error.argument.as_deref(), Some("command"));
        assert!(error.message.contains("bounded command"));
        assert!(error.message.contains("long-running dev servers"));
    }

    #[test]
    fn shell_command_accepts_bounded_install_build_and_test_commands() {
        for command in [
            "npm install",
            "npm run build",
            "npm test",
            "cargo test -p elgar-core",
        ] {
            let validated = validate_exactly_one_model_tool_call(&[raw_call(
                "tool-1",
                RawModelToolName::Known(ModelToolName::ShellCommand),
                json!({
                    "command": command,
                    "cwd": "."
                }),
            )])
            .unwrap_or_else(|error| panic!("{command} should validate: {error:?}"))
            .expect("one action");

            assert!(matches!(validated.request, ActionRequest::ShellCommand(_)));
        }
    }

    #[test]
    fn unknown_tool_rejects_safely() {
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Unknown("make_magic".to_string()),
            json!({}),
        );

        let error = validate_exactly_one_model_tool_call(&[draft]).expect_err("unknown tool");

        assert_eq!(error.kind, ModelToolValidationErrorKind::UnknownToolName);
        assert_eq!(error.tool_call_id, Some("tool-1".to_string()));
        assert_eq!(error.tool_name, Some("make_magic".to_string()));
    }

    #[test]
    fn malformed_or_missing_args_reject_safely() {
        let missing = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "contents": "hello"
            }),
        );
        let malformed = raw_call(
            "tool-2",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({
                "target_path": 42
            }),
        );

        let missing_error =
            validate_exactly_one_model_tool_call(&[missing]).expect_err("missing target_path");
        let malformed_error =
            validate_exactly_one_model_tool_call(&[malformed]).expect_err("malformed target_path");

        assert_eq!(
            missing_error.kind,
            ModelToolValidationErrorKind::MissingArgument
        );
        assert_eq!(missing_error.argument, Some("target_path".to_string()));
        assert_eq!(
            malformed_error.kind,
            ModelToolValidationErrorKind::MalformedArgument
        );
        assert_eq!(malformed_error.argument, Some("target_path".to_string()));
    }

    #[test]
    fn multiple_tool_calls_reject_safely() {
        let drafts = [
            raw_call(
                "tool-1",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "one" }),
            ),
            raw_call(
                "tool-2",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "two" }),
            ),
        ];

        let error = validate_exactly_one_model_tool_call(&drafts).expect_err("multiple calls");

        assert_eq!(error.kind, ModelToolValidationErrorKind::MultipleToolCalls);
    }

    #[test]
    fn multiple_tool_calls_validate_as_batch() {
        let drafts = [
            raw_call(
                "tool-1",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app" }),
            ),
            raw_call(
                "tool-2",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/index.html", "contents": "<div></div>" }),
            ),
        ];

        let validated = validate_model_tool_calls(&drafts).expect("validate batch");

        assert_eq!(validated.len(), 2);
        assert!(matches!(
            validated[0].request,
            ActionRequest::CreateDirectory(_)
        ));
        assert!(matches!(validated[1].request, ActionRequest::CreateFile(_)));
    }

    #[test]
    fn empty_tool_call_list_returns_no_action() {
        let validated =
            validate_exactly_one_model_tool_call(&[]).expect("empty calls should be valid");

        assert_eq!(validated, None);
        assert_eq!(validate_provider_prose_only().unwrap(), None);
    }

    #[test]
    fn validation_does_not_create_files() {
        let target = std::env::temp_dir().join(format!(
            "elgar-model-runtime-{}-no-mutation.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&target);
        let draft = raw_call(
            "tool-1",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": target,
                "contents": "validation only"
            }),
        );

        let validated = validate_exactly_one_model_tool_call(&[draft])
            .expect("validate create_file")
            .expect("one validated action");

        assert!(matches!(validated.request, ActionRequest::CreateFile(_)));
        assert!(!target.exists());
    }

    fn raw_call(
        id: &str,
        name: RawModelToolName,
        arguments: serde_json::Value,
    ) -> RawModelToolCall {
        RawModelToolCall {
            id: id.to_string(),
            name,
            arguments,
            assistant_summary: None,
        }
    }

    fn tool_parameters(name: &str) -> serde_json::Value {
        tool_definition(name).function.parameters
    }

    fn tool_definition(name: &str) -> ChatToolDefinition {
        elgar_model_tool_definitions()
            .into_iter()
            .find(|definition| definition.function.name == name)
            .unwrap_or_else(|| panic!("missing tool definition {name}"))
    }

    fn required_names(schema: &serde_json::Value) -> Vec<&str> {
        schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .map(|value| value.as_str().expect("required string"))
            .collect()
    }
}
