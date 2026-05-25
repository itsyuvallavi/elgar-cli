use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::action::{
    ActionRequest, CreateDirectoryAction, CreateFileAction, DeleteFileAction, MoveFileAction,
    OverwriteFileAction, PatchFileAction, ShellCommandAction,
};
use crate::provider::ChatToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelToolName {
    AskGuidance,
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
            "Draft a shell command for explicit review before execution.",
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
                        integer_property("Optional unsigned timeout in seconds."),
                    ),
                    (
                        "expected_effect",
                        string_property("Optional expected command effect."),
                    ),
                    (
                        "risk_notes",
                        string_property("Optional risk notes for reviewer context."),
                    ),
                    (
                        "expected_file",
                        string_property("Optional project-relative file expected after execution."),
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

fn integer_property(description: &'static str) -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "description": description
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
        [tool_call] => match validate_model_tool_call(tool_call)? {
            ValidatedModelToolOutput::Action(action) => Ok(Some(action)),
            ValidatedModelToolOutput::Guidance(_) => Ok(None),
        },
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
    tool_calls.iter().map(validate_model_tool_call).collect()
}

pub fn validate_provider_prose_only(
) -> Result<Option<ValidatedModelToolAction>, ModelToolValidationError> {
    validate_exactly_one_model_tool_call(&[])
}

fn validate_model_tool_call(
    tool_call: &RawModelToolCall,
) -> Result<ValidatedModelToolOutput, ModelToolValidationError> {
    let tool_name = tool_call.name.known().map_err(|mut error| {
        error.tool_call_id = Some(tool_call.id.clone());
        error
    })?;
    let arguments = arguments_object(tool_call)?;
    if tool_name == ModelToolName::AskGuidance {
        return Ok(ValidatedModelToolOutput::Guidance(
            ValidatedModelGuidanceRequest {
                tool_call_id: tool_call.id.clone(),
                question: required_non_empty_string(tool_call, arguments, tool_name, "question")?,
                reason: optional_non_empty_string(tool_call, arguments, tool_name, "reason")?,
            },
        ));
    }

    let request = match tool_name {
        ModelToolName::AskGuidance => unreachable!("ask_guidance returned before action parsing"),
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

    Ok(ValidatedModelToolOutput::Action(ValidatedModelToolAction {
        tool_call_id: tool_call.id.clone(),
        request,
        summary,
        target_label,
    }))
}

fn validate_shell_command(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
) -> Result<ActionRequest, ModelToolValidationError> {
    let command = required_non_empty_string(tool_call, arguments, tool_name, "command")?;
    let cwd = required_path(tool_call, arguments, tool_name, "cwd")?;
    let mut action = ShellCommandAction::new(command, cwd);

    if let Some(timeout_seconds) = optional_u64(tool_call, arguments, tool_name, "timeout_seconds")?
    {
        action.timeout_seconds = timeout_seconds;
    }
    if let Some(expected_effect) =
        optional_string(tool_call, arguments, tool_name, "expected_effect")?
    {
        action.expected_effect = expected_effect;
    }
    if let Some(risk_notes) = optional_string(tool_call, arguments, tool_name, "risk_notes")? {
        action.risk_notes = risk_notes;
    }
    if let Some(expected_file) = optional_path(tool_call, arguments, tool_name, "expected_file")? {
        action.expected_file = Some(expected_file);
    }
    if let Some(expected_directory) =
        optional_path(tool_call, arguments, tool_name, "expected_directory")?
    {
        action.expected_directory = Some(expected_directory);
    }

    Ok(ActionRequest::ShellCommand(action))
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

fn optional_path(
    tool_call: &RawModelToolCall,
    arguments: &Map<String, Value>,
    tool_name: ModelToolName,
    key: &str,
) -> Result<Option<PathBuf>, ModelToolValidationError> {
    optional_non_empty_string(tool_call, arguments, tool_name, key)
        .map(|value| value.map(PathBuf::from))
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
    use crate::action::{ActionRequest, SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS};

    #[test]
    fn model_tool_name_serde_names_roundtrip() {
        let value = serde_json::to_value([
            ModelToolName::AskGuidance,
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
        assert!(schema["properties"].get("expected_effect").is_some());
        assert!(schema["properties"].get("risk_notes").is_some());
        assert!(schema["properties"].get("expected_file").is_some());
        assert!(schema["properties"].get("expected_directory").is_some());
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
        elgar_model_tool_definitions()
            .into_iter()
            .find(|definition| definition.function.name == name)
            .unwrap_or_else(|| panic!("missing tool definition {name}"))
            .function
            .parameters
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
