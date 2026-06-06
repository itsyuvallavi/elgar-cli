use serde_json::json;

use super::*;
use crate::{
    action::{Action, CreateFileAction},
    agent_path_utils::normalize_path,
    agent_turn_router::{
        explicit_project_root_token, input_contains_executable_command_shape,
        input_has_run_prefixed_command_shape, local_path_like_token_count,
        looks_like_misrouted_artifact_chat, looks_like_misrouted_artifact_chat_after_retry,
        numbered_artifact_line_count,
    },
    controller_project_memory::record_verified_project_memory,
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    provider::{ChatToolDefinition, ProviderError, ProviderRequestMetadata},
    session::{ActionRecord, VerifiedFolderReference, VerifiedPlanReference},
    verified_state_answer::{verified_session_state_answer, VerifiedStateAnswerKind},
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct SequenceProvider {
    outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
    messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<ChatMessage>>>>,
}

impl SequenceProvider {
    fn new(outputs: Vec<crate::event::ProviderOutput>) -> Self {
        Self {
            outputs: std::sync::Arc::new(std::sync::Mutex::new(outputs)),
            messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl ControllerProvider for SequenceProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("sequence", None, "request")
    }

    fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
        Err(ProviderError::configuration("unused"))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        self.messages.lock().unwrap().push(messages);
        Ok(self.outputs.lock().unwrap().remove(0))
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        self.messages.lock().unwrap().push(messages);
        Ok(self.outputs.lock().unwrap().remove(0))
    }
}

#[test]
fn agent_prompts_describe_plan_artifact_before_same_turn_execution() {
    assert!(AGENT_SYSTEM_PROMPT
        .contains("create the plan file first, then implement the planned files"));
    assert!(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT
        .contains("same prompt creates plan then executes/implements it"));
    assert!(AGENT_NORMAL_TURN_DECISION_SYSTEM_PROMPT.len() <= 700);
}

#[test]
fn explicit_project_root_rejects_url_and_scheme_tokens() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-root-token-guard",
        std::process::id()
    ));
    let session = Session::new("session", &root, &root);

    for token in [
        "https://nextjs.org/docs/messages/module-not-found",
        "http://example.com/path",
        "file:///tmp/thing",
        "scheme:opaque/value",
    ] {
        assert_eq!(
            explicit_project_root_token(&session, token),
            None,
            "scheme/url token must not become a project root: {token}"
        );
    }

    // A normal relative project path under the root is still accepted.
    assert_eq!(
        explicit_project_root_token(&session, "my-project/api"),
        Some(normalize_path(root.join("my-project/api")))
    );
}

#[test]
fn permissive_agent_turn_executes_tool_call_and_continues() {
    let root = std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![RawModelToolCall {
            id: "call-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
            arguments: json!({ "target_path": "demo" }),
            assistant_summary: None,
        }]),
        crate::event::ProviderOutput::new("Created demo."),
    ]);
    let mut session = Session::new("session", &root, &root);

    let result = run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create a folder demo",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(result.route, Route::AskModel);
    assert!(root.join("demo").is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        crate::action::ActionLifecycleState::Applied
    );
    let messages = provider.messages.lock().unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages[1]
        .iter()
        .any(|message| matches!(message.role, ChatRole::Tool)));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn policy_applied_shell_command_records_verified_expected_effect() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-verified",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let expected_directory = root.join("shell-out");
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "mkdir shell-out",
                    "cwd": root.display().to_string(),
                    "expected_directory": expected_directory.display().to_string()
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create a directory using shell",
        PermissionPolicyMode::FullAccess,
    );

    assert!(expected_directory.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        crate::action::ActionLifecycleState::Applied
    );
    let expected_effect = format!(
        "verified directory exists: {}",
        expected_directory.display()
    );
    let Some(VerifiedActionResult::Shell(shell)) = session.actions()[0].verified_result.as_ref()
    else {
        panic!("expected verified shell result");
    };
    assert_eq!(shell.exit_code, Some(0));
    assert_eq!(
        shell.verified_effect.as_deref(),
        Some(expected_effect.as_str())
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(
                &applied.result,
                VerifiedActionResult::Shell(shell)
                    if shell.verified_effect.as_deref() == Some(expected_effect.as_str())
            )
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Done."
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
                && started.tool_count == Some(0)
    )));
    let messages = provider.messages.lock().unwrap();
    assert!(messages
        .iter()
        .any(
            |request_messages| request_messages.iter().any(|message| matches!(
                message.role,
                ChatRole::Tool
            ) && message
                .content
                .contains("answer the user in normal prose now"))
        ));
    assert!(messages
        .iter()
        .any(
            |request_messages| request_messages.iter().any(|message| matches!(
                message.role,
                ChatRole::System
            ) && message
                .content
                .contains("Do not request or describe any more tool calls"))
        ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn provider_message_after_verified_file_action_stays_suppressed() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-file-provider-suppressed",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![RawModelToolCall {
            id: "create-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "hello.txt",
                "contents": "hello\n"
            }),
            assistant_summary: None,
        }]),
        crate::event::ProviderOutput::new("Created hello.txt."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create hello.txt",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("hello.txt").exists());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Created hello.txt")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn policy_applied_shell_command_fails_when_expected_effect_is_missing() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-missing-effect",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let missing_file = root.join("missing.txt");
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "printf done",
                    "cwd": root.display().to_string(),
                    "expected_file": missing_file.display().to_string()
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create a file using shell",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!missing_file.exists());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        crate::action::ActionLifecycleState::Failed
    );
    assert!(session.actions()[0].verified_result.is_none());
    assert!(session.actions()[0]
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("expected files were not created")));
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn policy_applied_shell_command_records_nonzero_exit_as_verified_result() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-nonzero",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "exit 7",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "run a failing shell command",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        crate::action::ActionLifecycleState::Applied
    );
    assert!(matches!(
        session.actions()[0].verified_result.as_ref(),
        Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(7)
    ));
    assert!(session.actions()[0].failure_reason.is_none());
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn policy_applied_shell_command_resolves_relative_cwd_and_expected_paths() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-relative-paths",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("work")).unwrap();
    let expected_file = root.join("work/out.txt");
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Running command.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "printf ok > out.txt",
                    "cwd": "work",
                    "expected_file": "out.txt"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "run a command in work",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(std::fs::read_to_string(&expected_file).unwrap(), "ok");
    let Some(VerifiedActionResult::Shell(shell)) = session.actions()[0].verified_result.as_ref()
    else {
        panic!("expected verified shell result");
    };
    assert_eq!(shell.cwd, root.join("work").display().to_string());
    assert_eq!(
        shell.verified_effect.as_deref(),
        Some(format!("verified file exists: {}", expected_file.display()).as_str())
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_tool_call_turn_does_not_render_tool_planning_text_as_chat() {
    let root =
        std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool-text", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new(
            "We need to create the folder and write files. Let's implement.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "call-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
            arguments: json!({ "target_path": "demo" }),
            assistant_summary: None,
        }]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create demo",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("demo").is_dir());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("We need to create the folder")
                || message.content.contains("Let's implement")
    )));
    assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("demo")
                )
        )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Done."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_plan_only_request_does_not_retry_as_implementation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-only-no-implementation",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating folder and plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-only-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "ReactPlanOnly" }),
                    assistant_summary: Some("create folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-only-call-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ReactPlanOnly/plan.md",
                        "contents": "# React TS Tailwind Plan\n\n```text\npackage.json\nsrc/main.tsx\n```\n\n## Verification\n- Check package.json and src/main.tsx exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created. I have not implemented it yet."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "create a folder called ReactPlanOnly, then create a plan for a simple React TypeScript Tailwind project inside it. The plan should include all necessary files, but do not implement yet.",
            PermissionPolicyMode::FullAccess,
        );

    assert!(root.join("ReactPlanOnly/plan.md").is_file());
    assert!(!root.join("ReactPlanOnly/package.json").exists());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::Error(error)
            if error
                .message
                .contains("Provider did not return the required filesystem tool calls")
    )));
    assert_eq!(provider.messages.lock().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_records_reasoning_trace_for_review() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-reasoning-trace",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating only the plan file first.")
                .with_thinking("I need to create a plan before implementation and wait.")
                .with_tool_calls(vec![RawModelToolCall {
                    id: "plan-reasoning-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ReasoningPlan/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                }]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create only a project plan",
        PermissionPolicyMode::FullAccess,
    );

    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should be recorded");
    assert_eq!(trace.route.as_deref(), Some("plan_creation"));
    assert!(trace
        .provider_planning
        .iter()
        .any(|line| line.contains("create a plan before implementation")));
    assert!(trace.model_decisions.iter().any(
        |line| line.contains("requested create_file") && line.contains("ReasoningPlan/plan.md")
    ));
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("plan detected") && line.contains("ReasoningPlan/plan.md")));
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Wrote") && line.contains("ReasoningPlan/plan.md")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_batch_skips_implementation_tool_calls_in_same_response() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-batch-guard",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-batch-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanBatch" }),
                    assistant_summary: Some("create project folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanBatch/src" }),
                    assistant_summary: Some("create source folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/plan.md",
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-4".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/README.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                    }),
                    assistant_summary: Some("create readme".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-5".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanBatch/requirements.txt",
                        "contents": ""
                    }),
                    assistant_summary: Some("create requirements".to_string()),
                },
                RawModelToolCall {
                    id: "plan-batch-6".to_string(),
                    name: RawModelToolName::Known(ModelToolName::DeleteFile),
                    arguments: json!({ "target_path": "PlanBatch/requirements.txt" }),
                    assistant_summary: Some("delete requirements".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create a project plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("PlanBatch").is_dir());
    assert!(root.join("PlanBatch/plan.md").is_file());
    assert!(!root.join("PlanBatch/README.md").exists());
    assert!(!root.join("PlanBatch/src").exists());
    assert!(!root.join("PlanBatch/requirements.txt").exists());
    assert!(matches!(
        session.pending_action_selection(),
        PendingActionSelection::None
    ));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message
                    .content
                    .contains("Skipped extra implementation tool calls")
    )));
    assert!(session.project_memory().latest_structured_plan().is_some());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_turn_skips_later_implementation_rounds() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-later-round-guard",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-later-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanLater/PROJECT_PLAN.md",
                        "contents": "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("create plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Continuing.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-later-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "PlanLater/src" }),
                    assistant_summary: Some("create source folder".to_string()),
                },
                RawModelToolCall {
                    id: "plan-later-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::DeleteFile),
                    arguments: json!({ "target_path": "PlanLater/src" }),
                    assistant_summary: Some("delete source folder".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan only."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create only the project plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("PlanLater/PROJECT_PLAN.md").is_file());
    assert!(!root.join("PlanLater/src").exists());
    assert!(matches!(
        session.pending_action_selection(),
        PendingActionSelection::None
    ));
    assert_eq!(session.actions().len(), 1);
    assert_eq!(provider.messages.lock().unwrap().len(), 1);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("Skipped implementation tool calls")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_repairs_plan_without_expected_paths_before_implementation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-empty-path-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("PlanRepair")).unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating incomplete plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-repair-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanRepair/plan.md",
                        "contents": "# Project Plan\n\nThis is a tiny Python CLI app.\n"
                    }),
                    assistant_summary: Some("create incomplete plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-repair-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "PlanRepair/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                    }),
                    assistant_summary: Some("repair plan".to_string()),
                },
                RawModelToolCall {
                    id: "plan-repair-3".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "PlanRepair/src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: Some("create main too early".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan ready."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create only the project plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
            std::fs::read_to_string(root.join("PlanRepair/plan.md")).unwrap(),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
        );
    assert!(!root.join("PlanRepair/src/main.py").exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("repaired plan should be remembered");
    assert!(plan
        .expected_files
        .contains(&root.join("PlanRepair/src/main.py")));
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|message| message.content.contains("no executable expected paths")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_repairs_plan_missing_review_sections_before_implementation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-review-section-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("PlanReviewRepair")).unwrap();
    let repaired_plan = "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n";
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating weak plan.").with_tool_calls(vec![
            RawModelToolCall {
                id: "plan-review-repair-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "PlanReviewRepair/plan.md",
                    "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n"
                }),
                assistant_summary: Some("create weak plan".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Repairing review sections.").with_tool_calls(vec![
            RawModelToolCall {
                id: "plan-review-repair-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                arguments: json!({
                    "target_path": "PlanReviewRepair/plan.md",
                    "contents": repaired_plan
                }),
                assistant_summary: Some("repair plan sections".to_string()),
            },
            RawModelToolCall {
                id: "plan-review-repair-3".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "PlanReviewRepair/src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main too early".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Plan ready."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create only the project plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(root.join("PlanReviewRepair/plan.md")).unwrap(),
        repaired_plan
    );
    assert!(!root.join("PlanReviewRepair/src/main.py").exists());
    let contract = session
        .latest_plan_contract()
        .expect("repaired plan should create a contract");
    assert!(contract.review_draft().is_approvable());
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(
            |message| message.content.contains("missing `Verification` section")
                && message
                    .content
                    .contains("missing `Acceptance Criteria` section")
        ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn plan_creation_reports_needs_revision_when_repair_does_not_converge() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-needs-revision",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("NeedsRevision")).unwrap();
    let mut outputs = vec![crate::event::ProviderOutput::new("Creating weak plan.")
        .with_tool_calls(vec![RawModelToolCall {
            id: "plan-needs-revision-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "NeedsRevision/plan.md",
                "contents": "# Project Plan\n\n```text\nREADME.md\n```\n"
            }),
            assistant_summary: Some("create weak plan".to_string()),
        }])];
    for _ in 1..MAX_AGENT_TOOL_ROUNDS {
        outputs.push(crate::event::ProviderOutput::new("Plan created."));
    }
    let provider = SequenceProvider::new(outputs);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create only the project plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("NeedsRevision/plan.md").is_file());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("The plan needs revision before execution")
                && message.content.contains("missing `Verification` section")
                && message.content.contains("missing `Acceptance Criteria` section")
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| { line.contains("The plan needs revision before execution") })));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execute_plan_skips_redundant_directory_create_when_file_creates_parent() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-redundant-dir",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("demo")).unwrap();
    std::fs::write(
            root.join("demo/PROJECT_PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Executing plan.").with_tool_calls(vec![
            RawModelToolCall {
                id: "redundant-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "demo/src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "redundant-dir-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "demo/src" }),
                assistant_summary: Some("create src".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: root.join("demo/PROJECT_PLAN.md"),
        project_root: root.join("demo"),
        source_action_id: "action-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(root.join("demo/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("demo/src")
                )
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_allows_unrelated_folder_creation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-unrelated-folder",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("demo")).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating folder.").with_tool_calls(vec![
            RawModelToolCall {
                id: "outside-folder-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "other-folder" }),
                assistant_summary: Some("create folder".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: root.join("demo/PROJECT_PLAN.md"),
        project_root: root.join("demo"),
        source_action_id: "action-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create an unrelated folder",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("other-folder").is_dir());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("verified plan is rooted")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_allows_new_independent_plan_creation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-independent-draft",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("existing")).unwrap();
    std::fs::write(
            root.join("existing/plan.md"),
            "# Existing Plan\n\n```text\nmain.py\n```\n\n## Verification\n- Check main.py exists.\n\n## Acceptance Criteria\n- Existing plan remains available.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating the new plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "new-independent-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "new-plan/plan.md",
                        "contents": "# New Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check src/main.py and requirements.txt exist.\n\n## Acceptance Criteria\n- New plan can be executed independently.\n"
                    }),
                    assistant_summary: Some("create new independent plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan created."),
        ]);
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: root.join("existing/plan.md"),
        project_root: root.join("existing"),
        source_action_id: "action-existing-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create a separate plan for a different project",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("new-plan/plan.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("outside that project")
    )));
    let latest = session
        .project_memory()
        .latest_structured_plan()
        .expect("new plan should become latest structured plan");
    assert_eq!(latest.project_root, root.join("new-plan"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_create_file_target_gets_model_repair_without_user_error() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-missing-create-target",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating the file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "missing-create-target-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({ "contents": "# Notes\n" }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Repairing the file path.").with_tool_calls(vec![
            RawModelToolCall {
                id: "missing-create-target-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "notes.md",
                    "contents": "# Notes\n"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "create an md notes file",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(root.join("notes.md")).unwrap(),
        "# Notes\n"
    );
    assert_eq!(session.actions().len(), 1);
    assert_eq!(provider.messages.lock().unwrap().len(), 3);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message
                    .content
                    .contains("I need a concrete target path before I can create the file")
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::Error(error)
            if error.message.contains("model tool")
                || error.message.contains("missing required argument")
                || error.message.contains("Tool error")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_blocks_file_actions_outside_plan_root() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-preflight-outside",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("demo")).unwrap();
    std::fs::create_dir_all(root.join("other")).unwrap();
    let provider = SequenceProvider::new(vec![crate::event::ProviderOutput::new(
        "Creating missing files.",
    )
    .with_tool_calls(vec![RawModelToolCall {
        id: "outside-plan-root-1".to_string(),
        name: RawModelToolName::Known(ModelToolName::CreateFile),
        arguments: json!({
            "target_path": "other/index.tsx",
            "contents": "export default function Home() {}\n"
        }),
        assistant_summary: None,
    }])]);
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: root.join("demo/project-plan.md"),
        project_root: root.join("demo"),
        source_action_id: "action-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue from the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!root.join("other/index.tsx").exists());
    assert!(session.actions().is_empty());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("verified plan is rooted at demo")
                && message.content.contains("outside that project")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_allows_file_actions_inside_plan_root() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-preflight-inside",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "inside-plan-root-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "demo/index.tsx",
                    "contents": "export default function Home() {}\n"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: cwd.join("demo/project-plan.md"),
        project_root: cwd.join("demo"),
        source_action_id: "action-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue from the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/index.tsx")).unwrap(),
        "export default function Home() {}\n"
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_anchors_expected_unrooted_paths_under_plan_root() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-anchor-expected-paths",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("tui-state-memory-test")).unwrap();
    std::fs::write(
            cwd.join("tui-state-memory-test/PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "anchor-expected-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Creating remaining files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "anchor-expected-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("tui-state-memory-test/PLAN.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "tui-state-memory-test/PLAN.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("tui-state-memory-test/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(!cwd.join("src/main.py").exists());
    assert!(cwd.join("tui-state-memory-test/requirements.txt").is_file());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(
                &applied.result,
                VerifiedActionResult::FileWritten { path }
                    if path == &cwd
                        .join("tui-state-memory-test/src/main.py")
                        .display()
                        .to_string()
            )
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_repairs_non_approvable_contract_before_filesystem_changes() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-blocks-bad-contract",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
        cwd.join("demo/plan.md"),
        "# Project Plan\n\n```text\nsrc/main.py\n```\n",
    )
    .unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "blocked-plan-exec-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "src/main.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan first.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "repair-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "demo/plan.md",
                        "contents": "# Project Plan\n\n```text\nsrc/main.py\n```\n\n## Verification\n- `src/main.py` exists.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n"
                    }),
                    assistant_summary: Some("repair plan".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan revised."),
        ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!cwd.join("demo/src/main.py").exists());
    assert!(cwd.join("demo/plan.md").is_file());
    assert!(session
        .latest_plan_contract()
        .is_some_and(|contract| contract.review_draft().is_approvable()));
    assert_eq!(session.actions().len(), 1);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(&applied.result, VerifiedActionResult::File(
                crate::event::FileActionVerification::FileOverwritten { path }
            ) if path.ends_with("demo/plan.md"))
    )));
    assert_eq!(
        session
            .latest_reasoning_trace()
            .and_then(|trace| trace.route.as_deref()),
        Some("plan_creation")
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Cannot execute the plan yet"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_anchors_unlisted_parent_directory_under_plan_root() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-anchor-parent-dir",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating parent directory.").with_tool_calls(vec![
            RawModelToolCall {
                id: "parent-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "src" }),
                assistant_summary: Some("create src".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "main-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(cwd.join("demo/src").is_dir());
    assert!(cwd.join("demo/src/main.py").is_file());
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert!(!cwd.join("src").exists());
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|message| message.content.contains("Missing expected files")
            && message.content.contains("demo/src/main.py")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_repair_only_updates_same_plan_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-repair-same-plan-only",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo-bad")).unwrap();
    std::fs::write(
            cwd.join("demo-bad/PLAN.md"),
            "# Project Plan\n\n```text\ncli.py\nutils.py\n```\n\n## Verification\n- Run `pytest tests/test_cli.py`.\n\n## Acceptance Criteria\n- Running `python -m demo-bad.cli` prints a greeting.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "blocked-exec-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "cli.py",
                        "contents": "print('hello')\n"
                    }),
                    assistant_summary: None,
                },
            ]),
            crate::event::ProviderOutput::new("Repairing plan.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "rename-question-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::AskGuidance),
                    arguments: json!({
                        "question": "Should I rename the project folder to demo_bad?"
                    }),
                    assistant_summary: None,
                },
                RawModelToolCall {
                    id: "wrong-folder-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "demo_bad" }),
                    assistant_summary: Some("create replacement folder".to_string()),
                },
                RawModelToolCall {
                    id: "repair-plan-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::OverwriteFile),
                    arguments: json!({
                        "target_path": "demo-bad/PLAN.md",
                        "contents": "# Project Plan\n\n```text\ncli.py\nutils.py\ntests/test_cli.py\n```\n\n## Verification\n- `cli.py`, `utils.py`, and `tests/test_cli.py` exist.\n- Run `pytest tests/test_cli.py`.\n\n## Acceptance Criteria\n- Running `python cli.py --name Alice` prints `Hello, Alice!`.\n- The test suite passes.\n"
                    }),
                    assistant_summary: Some("repair same plan file".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Plan revised."),
        ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo-bad/PLAN.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo-bad/PLAN.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!cwd.join("demo_bad").exists());
    assert!(!cwd.join("demo-bad/cli.py").exists());
    assert!(std::fs::read_to_string(cwd.join("demo-bad/PLAN.md"))
        .unwrap()
        .contains("tests/test_cli.py"));
    assert!(session
        .latest_plan_contract()
        .is_some_and(|contract| contract.review_draft().is_approvable()));
    assert_eq!(session.actions().len(), 1);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("Should I rename the project folder")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_skips_off_root_directory_and_continues_missing_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-continue-missing",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "off-root-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "src" }),
                assistant_summary: Some("create src".to_string()),
            },
            RawModelToolCall {
                id: "main-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "guidance-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::AskGuidance),
                arguments: json!({
                    "question": "Should I create the src directory inside demo?"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Creating remaining file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!cwd.join("src").exists());
    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("Should I create the src directory")
    )));
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|message| message.content.contains("Missing expected files")
            && message.content.contains("demo/requirements.txt")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_skips_late_directory_that_file_action_already_created() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-late-dir",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "main-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Creating parent directory late.").with_tool_calls(vec![
            RawModelToolCall {
                id: "late-src-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "src" }),
                assistant_summary: Some("create src late".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(cwd.join("demo/src").is_dir());
    assert!(cwd.join("demo/src/main.py").is_file());
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert_eq!(session.actions().len(), 2);
    assert!(session
        .actions()
        .iter()
        .all(|record| { !matches!(record.action.request, ActionRequest::CreateDirectory(_)) }));
    assert_eq!(
        provider.messages.lock().unwrap().len(),
        1,
        "completed plan execution should not request a late redundant directory round"
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("skipped final provider synthesis"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_skips_destructive_followup_after_expected_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-skip-delete",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist and destructive extra actions are skipped.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "main-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
            RawModelToolCall {
                id: "delete-main-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::DeleteFile),
                arguments: json!({
                    "target_path": "src/main.py"
                }),
                assistant_summary: Some("delete main".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert!(matches!(
        session.pending_action_selection(),
        PendingActionSelection::None
    ));
    assert_eq!(session.actions().len(), 2);
    assert!(session
        .actions()
        .iter()
        .all(|record| { !matches!(record.action.request, ActionRequest::DeleteFile(_)) }));
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );
    assert_eq!(
        provider.messages.lock().unwrap().len(),
        1,
        "completed plan execution should not request a destructive follow-up round"
    );
    assert_eq!(
        session
            .latest_reasoning_trace()
            .and_then(|trace| trace.route.as_deref()),
        Some("plan_execution")
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line
                == "plan execution completed after skipped tool feedback; skipped final provider synthesis")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_reports_off_plan_verification_script_attempt() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-off-plan-verify-script",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("greeter")).unwrap();
    std::fs::write(
            cwd.join("greeter/plan.md"),
            "# Project Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "verify-readme-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "README.md",
                    "contents": "# Greeter\n"
                }),
                assistant_summary: Some("create readme".to_string()),
            },
            RawModelToolCall {
                id: "verify-requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
            RawModelToolCall {
                id: "verify-main-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "verify-test-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "tests/test_main.py",
                    "contents": "def test_smoke():\n    assert True\n"
                }),
                assistant_summary: Some("create test".to_string()),
            },
            RawModelToolCall {
                id: "verify-script-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "shell_verify.sh",
                    "contents": "python -m py_compile src/main.py tests/test_main.py\n"
                }),
                assistant_summary: Some("create verification script".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("greeter/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "greeter/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(cwd.join("greeter/README.md").is_file());
    assert!(cwd.join("greeter/requirements.txt").is_file());
    assert!(cwd.join("greeter/src/main.py").is_file());
    assert!(cwd.join("greeter/tests/test_main.py").is_file());
    assert!(!cwd.join("greeter/shell_verify.sh").exists());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("Skipped off-plan file")
                && message.content.contains("shell_verify.sh")
                && message
                    .content
                    .contains("Verification commands can stay in the plan's Verification section")
    )));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.runtime_checks.iter().any(|line| line
            .contains("Skipped off-plan file")
            && line.contains("shell_verify.sh"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_repairs_malformed_tool_call_without_raw_error() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-repairs-malformed",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\n```\n\n## Verification\n- `src/main.py` exists.\n\n## Acceptance Criteria\n- Expected file exists under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating directory.").with_tool_calls(vec![
            RawModelToolCall {
                id: "malformed-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!("src"),
                assistant_summary: Some("create src".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "repaired-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::Error(error)
            if error.message.contains("tool call is incomplete or malformed")
    )));
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|message| message
            .content
            .contains("send a corrected `create_directory` tool call")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_can_complete_many_expected_paths_one_per_round() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-many-rounds",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n  main.py\ntests/\n  __init__.py\n  test_main.py\nREADME.md\nrequirements.txt\n.gitignore\n```\n\n## Verification\n- All listed files and directories exist.\n\n## Acceptance Criteria\n- The complete expected tree is present.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Create src.").with_tool_calls(vec![RawModelToolCall {
            id: "many-src".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
            arguments: json!({ "target_path": "src" }),
            assistant_summary: Some("create src".to_string()),
        }]),
        crate::event::ProviderOutput::new("Create tests.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-tests".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "tests" }),
                assistant_summary: Some("create tests".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Create readme.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-readme".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "README.md",
                    "contents": "# Demo\n"
                }),
                assistant_summary: Some("create readme".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Create main.").with_tool_calls(vec![RawModelToolCall {
            id: "many-main".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: json!({
                "target_path": "src/main.py",
                "contents": "print('hello')\n"
            }),
            assistant_summary: Some("create main".to_string()),
        }]),
        crate::event::ProviderOutput::new("Create test init.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-test-init".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "tests/__init__.py",
                    "contents": ""
                }),
                assistant_summary: Some("create test init".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Create test file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-test-main".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "tests/test_main.py",
                    "contents": "def test_smoke():\n    assert True\n"
                }),
                assistant_summary: Some("create test".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Create requirements.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-requirements".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Create gitignore.").with_tool_calls(vec![
            RawModelToolCall {
                id: "many-gitignore".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": ".gitignore",
                    "contents": "__pycache__/\n"
                }),
                assistant_summary: Some("create gitignore".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    for path in [
        "demo/src/main.py",
        "demo/tests/__init__.py",
        "demo/tests/test_main.py",
        "demo/README.md",
        "demo/requirements.txt",
        "demo/.gitignore",
    ] {
        assert!(cwd.join(path).is_file(), "missing {path}");
    }
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_skips_existing_expected_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-skip-existing-files",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo/src")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
    std::fs::write(cwd.join("demo/src/main.py"), "print('existing')\n").unwrap();
    std::fs::write(cwd.join("demo/requirements.txt"), "").unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Recreating expected files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "existing-main-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('new')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
            RawModelToolCall {
                id: "existing-requirements-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "requirements.txt",
                    "contents": ""
                }),
                assistant_summary: Some("create requirements".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
        "print('existing')\n"
    );
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert!(session.actions().is_empty());
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionFailed(_))));
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );
    assert_eq!(
        provider.messages.lock().unwrap().len(),
        1,
        "completed plan execution should not request a redundant follow-up round"
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
            .runtime_checks
            .iter()
            .any(|line| line
                == "plan execution completed after skipped tool feedback; skipped final provider synthesis")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_execution_continues_for_missing_expected_directories() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-missing-dirs",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/\n└─ main.py\ntests/\n```\n\n## Verification\n- `src/main.py` exists and `tests/` exists.\n\n## Acceptance Criteria\n- Missing expected files and directories are created under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating first file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "main-file-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "src/main.py",
                    "contents": "print('hello')\n"
                }),
                assistant_summary: Some("create main".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Creating missing directory.").with_tool_calls(vec![
            RawModelToolCall {
                id: "tests-dir-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: json!({ "target_path": "tests" }),
                assistant_summary: Some("create tests".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(cwd.join("demo/src/main.py")).unwrap(),
        "print('hello')\n"
    );
    assert!(cwd.join("demo/tests").is_dir());
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(
            |message| message.content.contains("Missing expected directories")
                && message.content.contains("demo/tests")
        ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_plain_fallback_continues_missing_plan_repair() {
    #[derive(Debug, Clone)]
    struct FallbackDuringPlanProvider {
        tool_outputs: std::sync::Arc<
            std::sync::Mutex<Vec<Result<crate::event::ProviderOutput, ProviderError>>>,
        >,
        fallback_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
        messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<ChatMessage>>>>,
    }

    impl ControllerProvider for FallbackDuringPlanProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("fallback-plan", None, "request")
        }

        fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
            Err(ProviderError::configuration("unused"))
        }

        fn chat_messages_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.messages.lock().unwrap().push(messages);
            Ok(self.fallback_outputs.lock().unwrap().remove(0))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<crate::event::ProviderOutput, ProviderError> {
            self.messages.lock().unwrap().push(messages);
            self.tool_outputs.lock().unwrap().remove(0)
        }
    }

    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-fallback-repair",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    std::fs::write(
            cwd.join("demo/plan.md"),
            "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\ntests/\n```\n\n## Verification\n- `src/main.py`, `requirements.txt`, and `tests/` exist.\n\n## Acceptance Criteria\n- Missing expected paths are created under the plan root.\n",
        )
        .unwrap();
    let provider = FallbackDuringPlanProvider {
            tool_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                Ok(crate::event::ProviderOutput::new("Creating first file.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "main-file-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "src/main.py",
                            "contents": "print('hello')\n"
                        }),
                        assistant_summary: Some("create main".to_string()),
                    }],
                )),
                Err(ProviderError::empty_response("provider response contained no text")),
                Ok(crate::event::ProviderOutput::new("Creating remaining paths.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "requirements-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create requirements".to_string()),
                        },
                        RawModelToolCall {
                            id: "tests-dir-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                            arguments: json!({ "target_path": "tests" }),
                            assistant_summary: Some("create tests".to_string()),
                        },
                    ],
                )),
                Ok(crate::event::ProviderOutput::new("Done.")),
            ])),
            fallback_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                crate::event::ProviderOutput::new(
                    "<|channel|>commentary to=filesystem.create code<|message|>{\"path\":\"requirements.txt\",\"contents\":\"\"}",
                ),
            ])),
            messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("demo/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "demo/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(cwd.join("demo/src/main.py").is_file());
    assert!(cwd.join("demo/requirements.txt").is_file());
    assert!(cwd.join("demo/tests").is_dir());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message) if message.content.contains("<|channel|>")
    )));
    assert!(provider
        .messages
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(
            |message| message.content.contains("Missing expected directories")
                && message.content.contains("Missing expected files")
        ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_context_uses_cwd_relative_paths_for_tool_turns() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-context-cwd-relative",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("tui-state-test")).unwrap();
    std::fs::write(
            cwd.join("tui-state-test/PLAN.md"),
            "# Project Plan\n\n```text\n├── src\n│   └── main.py\n└── requirements.txt\n```\n\n## Verification\n- `src/main.py` and `requirements.txt` exist.\n\n## Acceptance Criteria\n- Expected files exist under the plan root.\n",
        )
        .unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I found the verified plan."),
        crate::event::ProviderOutput::new("I still need tool actions."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: cwd.join("tui-state-test"),
        source_action_id: "action-folder".to_string(),
    });
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("tui-state-test/PLAN.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: cwd.join("tui-state-test/PLAN.md").display().to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the plan",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("- latest verified folder: tui-state-test"));
    assert!(verified_context
        .content
        .contains("- latest verified plan: tui-state-test/PLAN.md"));
    assert!(verified_context
        .content
        .contains("- latest structured plan root: tui-state-test"));
    assert!(verified_context
        .content
        .contains("- missing expected directories:"));
    assert!(verified_context.content.contains("  - tui-state-test/src"));
    assert!(verified_context
        .content
        .contains("- missing expected files:"));
    assert!(verified_context
        .content
        .contains("  - tui-state-test/src/main.py"));
    assert!(verified_context
        .content
        .contains("  - tui-state-test/requirements.txt"));
    assert!(!verified_context
        .content
        .contains("playground/tui-state-test"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_folder_prompt_context_prefers_workspace_ancestor_over_child_folder() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-folder-context-ancestor",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let project = cwd.join("workspace");
    let child = project.join("notes");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&child).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I found the workspace."),
        crate::event::ProviderOutput::new("I still need tool actions."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: project.clone(),
        source_action_id: "action-project".to_string(),
    });
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: child,
        source_action_id: "action-child".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue in that workspace",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("- latest verified folder: workspace"));
    assert!(!verified_context
        .content
        .contains("- latest verified folder: workspace/notes"));
    let selection = session
        .latest_provider_prompt_memory_selection()
        .expect("selection should be recorded");
    assert_eq!(selection.selected[0].path, project);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_artifact_memory_injects_created_files_into_tool_turns() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-artifact-context-tool",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let project = cwd.join("workspace");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&project).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I can use verified artifacts."),
        crate::event::ProviderOutput::new("No tool action needed."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: project.clone(),
        source_action_id: "action-folder".to_string(),
    });
    push_verified_file_record(&mut session, "action-readme", "workspace/README.md");
    push_verified_file_record(&mut session, "action-main", "workspace/src/main.py");

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue the project",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("- verified artifacts from prior actions:"));
    assert!(verified_context.content.contains("latest action turn"));
    assert!(verified_context.content.contains("action-main turn"));
    assert!(verified_context
        .content
        .contains("created_file workspace/src/main.py under workspace"));
    assert!(verified_context.content.contains("action-readme turn"));

    let selection = session
        .latest_provider_prompt_memory_selection()
        .expect("artifact prompt selection should be recorded");
    assert!(selection.selected.iter().any(
        |fact| fact.kind == "verified_artifact" && fact.path.ends_with("workspace/src/main.py")
    ));
    let artifact_facts = selection
        .selected
        .iter()
        .filter(|fact| fact.kind == "verified_artifact")
        .collect::<Vec<_>>();
    let unique_artifact_facts = artifact_facts
        .iter()
        .map(|fact| (fact.source_action_id.as_str(), fact.path.as_path()))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(artifact_facts.len(), unique_artifact_facts.len());
    assert_eq!(
        verified_context
            .content
            .matches("workspace/src/main.py")
            .count(),
        1
    );
    assert_eq!(
        verified_context
            .content
            .matches("workspace/README.md")
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_artifact_memory_stays_out_of_plain_chat() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-artifact-plain-clean",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new();
    let mut session = Session::new("session", &root, &root);
    push_verified_file_record(&mut session, "action-notes", "notes.txt");

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    let joined = joined_request_messages(&requests[0]);
    assert!(!joined.contains("verified artifacts from prior actions"));
    assert!(!joined.contains("notes.txt"));
    assert!(session.latest_provider_prompt_memory_selection().is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn durable_session_log_memory_injects_prior_artifacts_into_tool_turns() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-durable-artifact-context-tool",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write_prior_session_log(
        &root,
        "prior-session",
        &[
            r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
            r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":2,"metadata":{"action_id":"action-shell","action_kind":"ShellCommand","operation":"shell_command","command_chars":7}}"#,
            "not json",
        ],
    );
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I can use durable verified artifacts."),
        crate::event::ProviderOutput::new("No tool action needed."),
    ]);
    let mut session = Session::new("current-session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue the project from the previous session",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("- durable verified artifacts from local session logs:"));
    assert!(verified_context
        .content
        .contains("prior-session:action-prior turn 1 file_written prior/README.md"));
    assert!(!verified_context.content.contains("action-shell"));

    let selection = session
        .latest_provider_prompt_memory_selection()
        .expect("durable artifact prompt selection should be recorded");
    assert!(selection.selected.iter().any(|fact| {
        fact.kind == "durable_verified_artifact"
            && fact.path.ends_with("prior/README.md")
            && fact.source_action_id == "prior-session:action-prior"
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn durable_session_log_memory_stays_out_of_plan_work_tool_turns() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-durable-artifact-plan-clean",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write_prior_session_log(
        &root,
        "prior-session",
        &[
            r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
        ],
    );
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_output(crate::event::ProviderOutput::new("No tool action needed."));
    let mut session = Session::new("current-session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "Create a project plan, then execute it.",
    );

    let tool_request = only_tool_request(&provider);
    let joined = joined_request_messages(&tool_request);
    assert!(!joined.contains("durable verified artifacts"));
    assert!(!joined.contains("prior/README.md"));
    assert!(!session
        .latest_provider_prompt_memory_selection()
        .map(|selection| selection
            .selected
            .iter()
            .any(|fact| fact.kind == "durable_verified_artifact"))
        .unwrap_or(false));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn durable_session_log_memory_stays_out_of_plain_chat() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-durable-artifact-plain-clean",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    write_prior_session_log(
        &root,
        "prior-session",
        &[
            r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-prior","action_kind":"CreateFile","operation":"file_written","path":"prior/README.md"}}"#,
        ],
    );
    let provider = CapturingProvider::new();
    let mut session = Session::new("current-session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    let joined = joined_request_messages(&requests[0]);
    assert!(!joined.contains("durable verified artifacts"));
    assert!(!joined.contains("prior/README.md"));
    assert!(session.latest_provider_prompt_memory_selection().is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_artifact_memory_prompt_caps_and_reports_omitted_count() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-artifact-context-cap",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I can use capped verified artifacts."),
        crate::event::ProviderOutput::new("No tool action needed."),
    ]);
    let mut session = Session::new("session", &root, &root);
    for index in 1..=8 {
        push_verified_file_record(
            &mut session,
            &format!("action-{index}"),
            &format!("file-{index}.txt"),
        );
    }

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "use the first file you created",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("earliest session artifacts"));
    assert!(verified_context.content.contains("action-1 turn"));
    assert!(verified_context.content.contains("file-1.txt"));
    assert!(verified_context
        .content
        .contains("latest session artifacts"));
    assert!(verified_context.content.contains("action-8 turn"));
    assert!(verified_context.content.contains("file-8.txt"));
    assert!(verified_context
        .content
        .contains("omitted 2 older verified artifact(s) due to prompt cap"));
    assert!(verified_context
        .content
        .contains("omitted 5 older verified artifact(s) due to prompt cap"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn completed_structured_plan_prompt_context_keeps_files_editable() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-completed-plan-editable-context",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let project = cwd.join("workspace");
    let plan_path = project.join("plan.md");
    let readme_path = project.join("README.md");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&plan_path, "# Plan\n").unwrap();
    std::fs::write(&readme_path, "old\n").unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("I can update it."),
        crate::event::ProviderOutput::new("I still need tool actions."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_plan_reference(crate::session::VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Completed,
        expected_directories: Vec::new(),
        expected_files: vec![readme_path],
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "overwrite README.md in that project",
        PermissionPolicyMode::FullAccess,
    );

    let messages = provider.messages.lock().unwrap();
    let verified_context = messages[0]
        .iter()
        .find(|message| message.content.contains("Verified filesystem context"))
        .expect("tool turn should include verified memory context");
    assert!(verified_context
        .content
        .contains("completed structured plan files are still editable"));
    assert!(verified_context.content.contains("workspace/README.md"));
    assert!(verified_context
        .content
        .contains("runtime validation and policy decide"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_folder_context_anchors_relative_existing_file_actions_under_workspace() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-folder-action-anchor",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let project = cwd.join("workspace");
    let notes = project.join("notes");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("archive.txt"), "archive\n").unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Moving archive.").with_tool_calls(vec![
            RawModelToolCall {
                id: "move-relative-under-workspace".to_string(),
                name: RawModelToolName::Known(ModelToolName::MoveFile),
                arguments: json!({
                    "source_path": "notes/archive.txt",
                    "target_path": "notes/archived.txt"
                }),
                assistant_summary: Some("move archive".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: project.clone(),
        source_action_id: "action-project".to_string(),
    });
    session.record_verified_folder_reference(crate::session::VerifiedFolderReference {
        path: notes,
        source_action_id: "action-notes".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "move notes/archive.txt to notes/archived.txt",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!project.join("notes/archive.txt").exists());
    assert!(project.join("notes/archived.txt").is_file());
    assert!(!cwd.join("notes/archived.txt").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_allows_deduplicated_cwd_prefix_target() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-preflight-duplicate-prefix",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("demo")).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "duplicate-prefix-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "playground/demo/index.tsx",
                    "contents": "export default function Home() {}\n"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Done."),
    ]);
    let mut session = Session::new("session", &root, &cwd);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: cwd.join("demo/project-plan.md"),
        project_root: cwd.join("demo"),
        source_action_id: "action-plan".to_string(),
    });

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "continue from the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(cwd.join("demo/index.tsx").is_file());
    assert!(!cwd.join("playground/demo/index.tsx").exists());
    assert_eq!(session.actions().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_move_source_path_gets_model_repair_without_raw_tool_error() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-missing-move-source",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Moving the file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "missing-move-source-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::MoveFile),
                arguments: json!({ "target_path": "renamed.md" }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Which source path should I move?"),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "move the file",
        PermissionPolicyMode::FullAccess,
    );

    assert!(session.actions().is_empty());
    assert_eq!(provider.messages.lock().unwrap().len(), 2);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("source path")
    )));
    assert_no_raw_tool_validation_error(&session);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_shell_cwd_gets_model_repair_without_running_command() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-missing-shell-cwd",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Running the command.").with_tool_calls(vec![
            RawModelToolCall {
                id: "missing-shell-cwd-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({ "command": "printf hello" }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Which working directory should I use?"),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "run a shell command",
        PermissionPolicyMode::FullAccess,
    );

    assert!(session.actions().is_empty());
    assert_eq!(provider.messages.lock().unwrap().len(), 2);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("working directory")
    )));
    assert_no_raw_tool_validation_error(&session);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn malformed_patch_find_gets_model_repair_without_raw_tool_error() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-missing-patch-find",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.md"), "old\n").unwrap();
    let provider = SequenceProvider::new(vec![
        crate::event::ProviderOutput::new("Patching the file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "missing-patch-find-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::PatchFile),
                arguments: json!({
                    "target_path": "notes.md",
                    "find": "",
                    "replace": "new"
                }),
                assistant_summary: None,
            },
        ]),
        crate::event::ProviderOutput::new("Which exact text should I replace?"),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "patch notes",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        std::fs::read_to_string(root.join("notes.md")).unwrap(),
        "old\n"
    );
    assert!(session.actions().is_empty());
    assert_eq!(provider.messages.lock().unwrap().len(), 2);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("exact text")
    )));
    assert_no_raw_tool_validation_error(&session);

    let _ = std::fs::remove_dir_all(&root);
}

fn assert_no_raw_tool_validation_error(session: &Session) {
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::Error(error)
            if error.message.contains("model tool")
                || error.message.contains("missing required argument")
                || error.message.contains("Tool error")
    )));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapturedProviderRequestMode {
    Plain,
    Tool,
}

#[derive(Debug, Clone)]
struct CapturedProviderRequest {
    mode: CapturedProviderRequestMode,
    messages: Vec<ChatMessage>,
    tool_count: usize,
    tool_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct CapturingProvider {
    requests: std::sync::Arc<std::sync::Mutex<Vec<CapturedProviderRequest>>>,
    plain_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
    tool_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
    model: Option<String>,
}

impl CapturingProvider {
    fn new() -> Self {
        Self {
            requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            plain_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                crate::event::ProviderOutput::new("{\"route\":\"chat\"}"),
                crate::event::ProviderOutput::new("Plain answer."),
            ])),
            tool_outputs: std::sync::Arc::new(std::sync::Mutex::new(vec![
                crate::event::ProviderOutput::new("I'll create it."),
            ])),
            model: Some("test-model".to_string()),
        }
    }

    fn with_tool_output(mut self, output: crate::event::ProviderOutput) -> Self {
        self.tool_outputs = std::sync::Arc::new(std::sync::Mutex::new(vec![output]));
        self
    }

    fn with_tool_outputs(mut self, outputs: Vec<crate::event::ProviderOutput>) -> Self {
        self.tool_outputs = std::sync::Arc::new(std::sync::Mutex::new(outputs));
        self
    }

    fn with_plain_output(mut self, output: crate::event::ProviderOutput) -> Self {
        self.plain_outputs = std::sync::Arc::new(std::sync::Mutex::new(vec![output]));
        self
    }

    fn with_plain_outputs(mut self, outputs: Vec<crate::event::ProviderOutput>) -> Self {
        self.plain_outputs = std::sync::Arc::new(std::sync::Mutex::new(outputs));
        self
    }

    fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    fn requests(&self) -> Vec<CapturedProviderRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn next_plain_output(&self) -> crate::event::ProviderOutput {
        let mut outputs = self.plain_outputs.lock().unwrap();
        if outputs.len() > 1 {
            outputs.remove(0)
        } else {
            outputs
                .first()
                .cloned()
                .unwrap_or_else(|| crate::event::ProviderOutput::new("Plain answer."))
        }
    }

    fn next_tool_output(&self, has_tool_result: bool) -> crate::event::ProviderOutput {
        let mut outputs = self.tool_outputs.lock().unwrap();
        if !outputs.is_empty() {
            return outputs.remove(0);
        }
        if has_tool_result {
            crate::event::ProviderOutput::new("Done.")
        } else {
            crate::event::ProviderOutput::new("I'll create it.")
        }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum CapturedToolStep {
    Output(crate::event::ProviderOutput),
    EmptyResponse,
}

#[derive(Debug, Clone)]
struct CapturingProviderWithToolErrors {
    requests: std::sync::Arc<std::sync::Mutex<Vec<CapturedProviderRequest>>>,
    plain_outputs: std::sync::Arc<std::sync::Mutex<Vec<crate::event::ProviderOutput>>>,
    tool_steps: std::sync::Arc<std::sync::Mutex<Vec<CapturedToolStep>>>,
}

impl CapturingProviderWithToolErrors {
    fn new(
        plain_outputs: Vec<crate::event::ProviderOutput>,
        tool_steps: Vec<CapturedToolStep>,
    ) -> Self {
        Self {
            requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            plain_outputs: std::sync::Arc::new(std::sync::Mutex::new(plain_outputs)),
            tool_steps: std::sync::Arc::new(std::sync::Mutex::new(tool_steps)),
        }
    }

    fn requests(&self) -> Vec<CapturedProviderRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn next_plain_output(&self) -> crate::event::ProviderOutput {
        let mut outputs = self.plain_outputs.lock().unwrap();
        if outputs.len() > 1 {
            outputs.remove(0)
        } else {
            outputs.first().cloned().unwrap_or_else(|| {
                crate::event::ProviderOutput::new(
                    "{\"route\":\"chat\",\"content\":\"Plain answer.\"}",
                )
            })
        }
    }

    fn next_tool_step(&self) -> CapturedToolStep {
        let mut steps = self.tool_steps.lock().unwrap();
        if steps.is_empty() {
            CapturedToolStep::Output(crate::event::ProviderOutput::new("Done."))
        } else {
            steps.remove(0)
        }
    }
}

impl ControllerProvider for CapturingProviderWithToolErrors {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("capture", Some("test-model".to_string()), "request")
    }

    fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
        Ok(self.next_plain_output())
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        self.requests.lock().unwrap().push(CapturedProviderRequest {
            mode: CapturedProviderRequestMode::Tool,
            messages,
            tool_count: tools.len(),
            tool_names: tools.into_iter().map(|tool| tool.function.name).collect(),
        });
        match self.next_tool_step() {
            CapturedToolStep::Output(output) => Ok(output),
            CapturedToolStep::EmptyResponse => {
                Err(ProviderError::empty_response("empty tool response"))
            }
        }
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        self.requests.lock().unwrap().push(CapturedProviderRequest {
            mode: CapturedProviderRequestMode::Plain,
            messages,
            tool_count: 0,
            tool_names: Vec::new(),
        });
        Ok(self.next_plain_output())
    }
}

fn joined_request_messages(request: &CapturedProviderRequest) -> String {
    request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn only_tool_request(provider: &CapturingProvider) -> CapturedProviderRequest {
    provider
        .requests()
        .into_iter()
        .find(|request| request.mode == CapturedProviderRequestMode::Tool)
        .expect("tool request should be captured")
}

fn push_verified_file_record(session: &mut Session, action_id: &str, path: &str) {
    session.start_reasoning_trace(format!("turn for {action_id}"));
    let action = Action::proposed(
        action_id,
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from(path),
            contents: String::new(),
        }),
        "create file",
    )
    .approve()
    .mark_applied();
    let mut record = ActionRecord::new(action);
    record.verified_result = Some(VerifiedActionResult::File(
        crate::event::FileActionVerification::FileCreated {
            path: path.to_string(),
        },
    ));
    session.push_action(record);
}

fn write_prior_session_log(root: &Path, session_id: &str, lines: &[&str]) {
    let path = crate::local_session_log::session_log_file_path(root, session_id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

fn trace_events(root: &Path, session_id: &str) -> Vec<serde_json::Value> {
    let path = crate::local_trace::trace_file_path(root, session_id);
    let contents = std::fs::read_to_string(path).expect("trace file should exist");
    contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("trace line should be valid json"))
        .collect()
}

fn session_log_events(root: &Path, session_id: &str) -> Vec<serde_json::Value> {
    let path = crate::local_session_log::session_log_file_path(root, session_id);
    let contents = std::fs::read_to_string(path).expect("session log file should exist");
    contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("session log line should be valid json"))
        .collect()
}

fn trace_kinds(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| event.get("kind").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
        .collect()
}

impl ControllerProvider for CapturingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("capture", self.model.clone(), "request")
    }

    fn chat(&self, _prompt: &str) -> Result<crate::event::ProviderOutput, ProviderError> {
        Ok(self.next_plain_output())
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        let has_tool_result = messages
            .iter()
            .any(|message| matches!(message.role, ChatRole::Tool));
        self.requests.lock().unwrap().push(CapturedProviderRequest {
            mode: CapturedProviderRequestMode::Tool,
            messages,
            tool_count: tools.len(),
            tool_names: tools.into_iter().map(|tool| tool.function.name).collect(),
        });
        Ok(self.next_tool_output(has_tool_result))
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<crate::event::ProviderOutput, ProviderError> {
        self.requests.lock().unwrap().push(CapturedProviderRequest {
            mode: CapturedProviderRequestMode::Plain,
            messages,
            tool_count: 0,
            tool_names: Vec::new(),
        });
        Ok(self.next_plain_output())
    }
}

#[test]
fn permissive_agent_plain_text_uses_plain_provider_request_first() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plain-text-runtime",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    for input in ["say hi", "what are you?", "write a short sentence"] {
        let provider = CapturingProvider::new();
        let mut session = Session::new("session", &root, &root);

        run_permissive_agent_turn(&provider, &mut session, input);

        let requests = provider.requests();
        assert_eq!(requests.len(), 2, "unexpected request count for {input}");
        assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[0].tool_count, 0);
        assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
        assert_eq!(requests[1].tool_count, 0);
        assert_eq!(requests[0].messages.last(), Some(&ChatMessage::user(input)));
        let joined = joined_request_messages(&requests[0]);
        assert!(
            requests[0].messages[0].content.len() <= 700,
            "plain route prompt grew: {}",
            requests[0].messages[0].content
        );
        assert!(joined.contains("Runtime location:"));
        assert!(joined.contains(&format!("project_root={}", root.display())));
        assert!(joined.contains("Current/root/this folder/project means cwd"));
        assert!(!joined.contains("latest verified folder"));
        assert!(!joined.contains("latest verified plan"));
        assert!(!joined.contains("Verified filesystem context"));
        assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("plain_chat")
                    && started.model.as_deref() == Some("test-model")
                    && started.tool_count == Some(0)
        )));
        assert!(!session.events().iter().any(|event| matches!(
            event,
            Event::ProviderStarted(started)
                if started.request_mode.as_deref() == Some("tool_enabled")
        )));
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn trivial_greeting_uses_plain_provider_request() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-trivial-chat-provider-first",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_model("qwen3.6-35b-a3b-ud-mlx")
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new("{\"route\":\"chat\"}"),
            crate::event::ProviderOutput::new("Model-authored greeting."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "hello!");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(requests[0].tool_names.is_empty());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Model-authored greeting."
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| {
        trace.route.as_deref() == Some("chat")
            && !trace
                .model_decisions
                .iter()
                .any(|line| line.contains("fast path"))
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn request_modes_split_tool_and_tool_result_synthesis_without_caps() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-request-mode-split",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_tool_output(
            crate::event::ProviderOutput::new("Run verification.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "budget-shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf done > marker.txt",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("run verification".to_string()),
                },
            ]),
        )
        .with_plain_output(crate::event::ProviderOutput::new(
            "Model-authored shell result summary.",
        ));
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "run the verification command",
        PermissionPolicyMode::FullAccess,
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Model-authored shell result summary."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn report_only_shell_execution_synthesizes_after_one_verified_result() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-transaction",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
            ),
            crate::event::ProviderOutput::new("The build command completed successfully: build ok"),
        ])
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Run build.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf 'build ok\\n'",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("run build".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Run redundant check.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-2".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "pwd",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("run redundant check".to_string()),
                },
            ]),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_turn_with_policy(
        &provider,
        &mut session,
        "Run printf build-ok and report the result. Do not edit files.",
        PermissionPolicyMode::FullAccess,
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(
        requests[1].tool_names,
        vec!["ask_guidance", "shell_command"]
    );
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[2].tool_count, 0);
    let applied_shell_actions = session
        .actions()
        .iter()
        .filter(|record| matches!(record.verified_result, Some(VerifiedActionResult::Shell(_))))
        .count();
    assert_eq!(applied_shell_actions, 1);
    let synthesis_context = joined_request_messages(&requests[2]);
    assert!(synthesis_context.contains("VERIFIED_SHELL_RESULT"));
    assert!(synthesis_context.contains(crate::agent_synthesis::AGENT_SHELL_RESULT_SYNTHESIS_PROMPT));
    assert!(!synthesis_context.contains("Use tools to do the user's requested"));
    assert!(synthesis_context.contains("command: printf 'build ok"));
    assert!(synthesis_context.contains("exit_code: 0"));
    assert!(synthesis_context.contains("stdout_summary:"));
    assert!(synthesis_context.contains("build ok"));
    assert!(synthesis_context.contains("answer_now: true"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
                && started.tool_count == Some(0)
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "The build command completed successfully: build ok"
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shell_result_synthesis_retries_when_short_stdout_is_not_exact() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-exact-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
            ),
            crate::event::ProviderOutput::new(
                "The current directory is /Users/yuval/git/elgar/playground/Nextjs-1",
            ),
            crate::event::ProviderOutput::new(
                "The current directory is /Users/yuval/__git/elgar/playground/Nextjs-1",
            ),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Run pwd.").with_tool_calls(vec![RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "printf '/Users/yuval/__git/elgar/playground/Nextjs-1\\n'",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("run pwd".to_string()),
            }]),
        );
    let mut session = Session::new("session", &root, &root);

    run_agent_turn_with_policy(
        &provider,
        &mut session,
        "Run pwd and report the result. Do not edit files.",
        PermissionPolicyMode::FullAccess,
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[3].mode, CapturedProviderRequestMode::Plain);
    let retry_context = joined_request_messages(&requests[3]);
    assert!(retry_context.contains("EXACT_VERIFIED_STDOUT"));
    assert!(retry_context.contains(crate::agent_synthesis::AGENT_SHELL_RESULT_SYNTHESIS_PROMPT));
    assert!(!retry_context.contains("Use tools to do the user's requested"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("/Users/yuval/__git/elgar/playground/Nextjs-1")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn generic_execute_can_continue_after_failed_shell_result() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-fix-continues",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let fix_path = root.join("fix.txt");
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Run failing check.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf 'needs fix\\n' >&2; exit 7",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("run failing check".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Apply fix.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "fix-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": fix_path.display().to_string(),
                        "contents": "fixed\n"
                    }),
                    assistant_summary: Some("write fix marker".to_string()),
                },
            ]),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_turn_with_policy(
        &provider,
        &mut session,
        "Run the check, then repair the issue.",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(std::fs::read_to_string(&fix_path).unwrap(), "fixed\n");
    assert!(session.actions().iter().any(|record| matches!(
        record.verified_result.as_ref(),
        Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(7)
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn command_shaped_state_route_retries_without_state_classifier() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-command-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new("{\"route\":\"state\"}"),
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
            ),
            crate::event::ProviderOutput::new(format!("The command printed {}.", root.display())),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Run pwd.").with_tool_calls(vec![RawModelToolCall {
                id: "pwd-shell".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "pwd",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("run pwd".to_string()),
            }]),
        );
    let mut session = Session::new("session", &root, &root);

    run_agent_turn_with_policy(
        &provider,
        &mut session,
        "Run pwd and report the result.",
        PermissionPolicyMode::FullAccess,
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 5);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("plain_state_classifier")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("plain_route_retry")
    )));
    assert!(joined_request_messages(&requests[4]).contains("EXACT_VERIFIED_STDOUT"));
    assert!(session.actions().iter().any(|record| matches!(
        record.verified_result.as_ref(),
        Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(0)
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn identical_successful_shell_command_runs_once_per_turn() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-repeat-shell-breaker",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_tool_outputs(vec![
        crate::event::ProviderOutput::new("List project tree.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "pwd",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("list project tree".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("List project tree again.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "pwd",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("list project tree again".to_string()),
            },
        ]),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "show me the project tree",
        PermissionPolicyMode::FullAccess,
    );

    let applied_shell_actions = session
        .actions()
        .iter()
        .filter(|record| matches!(record.verified_result, Some(VerifiedActionResult::Shell(_))))
        .count();
    assert_eq!(applied_shell_actions, 1);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("Skipped repeated shell command")
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Skipped repeated shell command"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn recursive_project_listing_shell_command_is_rewritten_to_bounded_inspection() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-safe-project-listing",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("node_modules/pkg/index.js"),
        "module.exports = {}\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_tool_output(
        crate::event::ProviderOutput::new("List project tree.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "ls -R .",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("list project tree".to_string()),
            },
        ]),
    );
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "show me the project tree",
        PermissionPolicyMode::FullAccess,
    );

    let shell = session
        .actions()
        .iter()
        .find_map(|record| match record.verified_result.as_ref()? {
            VerifiedActionResult::Shell(shell) => Some(shell),
            _ => None,
        })
        .expect("shell action should be verified");
    assert!(
        shell.command.starts_with("find . -maxdepth 3"),
        "{}",
        shell.command
    );
    assert!(shell.stdout.contains("./app/page.tsx"), "{}", shell.stdout);
    assert!(
        !shell.stdout.contains("node_modules/pkg"),
        "{}",
        shell.stdout
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("rewrote heavy project listing"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn piped_project_listing_shell_command_is_rewritten_to_bounded_inspection() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-piped-safe-project-listing",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("node_modules/pkg/index.js"),
        "module.exports = {}\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_tool_output(
            crate::event::ProviderOutput::new("List project tree.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "find . -maxdepth 3 -not -path '*/node_modules/*' -not -path '*/.git/*' | head -80",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("list project tree".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "show me the project tree",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let shell = session
        .actions()
        .iter()
        .find_map(|record| match record.verified_result.as_ref()? {
            VerifiedActionResult::Shell(shell) => Some(shell),
            _ => None,
        })
        .expect("shell action should be verified");
    assert!(
        shell.command.starts_with("find . -maxdepth 3"),
        "{}",
        shell.command
    );
    assert!(shell.stdout.contains("./app/page.tsx"), "{}", shell.stdout);
    assert!(
        !shell.stdout.contains("node_modules/pkg"),
        "{}",
        shell.stdout
    );
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("rewrote heavy project listing"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sorted_project_listing_shell_command_is_rewritten_to_bounded_inspection() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-sorted-safe-project-listing",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_tool_output(
            crate::event::ProviderOutput::new("List project tree.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "find . -not -path './node_modules/*' -not -path './.git/*' -not -path './.next/*' | sort",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("list project tree".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "show me the project tree",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let shell = session
        .actions()
        .iter()
        .find_map(|record| match record.verified_result.as_ref()? {
            VerifiedActionResult::Shell(shell) => Some(shell),
            _ => None,
        })
        .expect("shell action should be verified");
    assert!(
        shell.command.starts_with("find . -maxdepth 3"),
        "{}",
        shell.command
    );
    assert!(shell.stdout.contains("./app/page.tsx"), "{}", shell.stdout);
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shell_execution_model_route_exposes_only_shell_safe_tools() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-execution-model-route",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Run verification.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "echo build ok",
                        "cwd": root.display().to_string(),
                        "timeout_seconds": 120
                    }),
                    assistant_summary: Some("run verification".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);

    run_agent_turn_with_policy(
        &provider,
        &mut session,
        "Run cargo test and report the result. Do not edit files.",
        PermissionPolicyMode::ReviewAll,
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[1].tool_count, 2);
    assert!(requests[1]
        .tool_names
        .iter()
        .any(|name| name == "shell_command"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("plain_chat")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
                && started.tool_count == Some(2)
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| {
        trace.route.as_deref() == Some("execute")
            && trace
                .model_decisions
                .iter()
                .any(|line| line.contains("selected execute intent shell_execution"))
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn command_question_stays_plain_chat_first() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-command-question-plain",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_outputs(vec![
        crate::event::ProviderOutput::new("{\"route\":\"chat\"}"),
        crate::event::ProviderOutput::new("It runs the Rust test suite."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "What does cargo test do?");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "It runs the Rust test suite."
    )));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exact_echo_chat_content_is_ignored_and_normal_chat_response_is_used() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-chat-echo-ignored",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_outputs(vec![
        crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"what can you do?\"}"),
        crate::event::ProviderOutput::new("I can help with local project work and questions."),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "what can you do?");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("chat_response")
                && started.tool_count == Some(0)
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "what can you do?"
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("local project work")
    )));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn classifier_leakage_chat_content_is_ignored_and_normal_chat_response_is_used() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-classifier-leak-ignored",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"chat\",\"content\":\"I am following your system instructions to classify inputs and return compact JSON responses.\"}",
            ),
            crate::event::ProviderOutput::new("I should answer normally here."),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "why are you doing that?");

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("chat_response")
                && started.tool_count == Some(0)
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("compact JSON")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "I should answer normally here."
    )));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn local_path_request_skips_route_classifier_and_synthesizes_after_tool() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-direct-local-path-route",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Home() { return <main>Hello</main>; }\n",
    )
    .unwrap();
    let provider = CapturingProvider::new()
        .with_tool_outputs(vec![crate::event::ProviderOutput::new("Read page file.")
            .with_tool_calls(vec![RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "cat app/page.tsx",
                    "cwd": root.display().to_string(),
                    "timeout_seconds": 30
                }),
                assistant_summary: Some("read page".to_string()),
            }])])
        .with_plain_outputs(vec![crate::event::ProviderOutput::new(
            "The page renders Hello in a main element.",
        )]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "read app/page.tsx and tell me what the page renders.",
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(session.actions().iter().any(|record| {
        matches!(
            record.verified_result.as_ref(),
            Some(VerifiedActionResult::Shell(shell))
                if shell.command == "cat app/page.tsx" && shell.stdout.contains("Hello")
        )
    }));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("without route classifier"))));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("renders Hello")
    )));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn read_only_cat_missing_fallback_is_rewritten_before_policy() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-cat-fallback-rewrite",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("package.json"), "{}\n").unwrap();
    let provider = CapturingProvider::new().with_tool_output(
        crate::event::ProviderOutput::new("Read package manifest.").with_tool_calls(vec![
            RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "cat package.json 2>/dev/null || echo \"NOT FOUND\"",
                    "cwd": root.display().to_string(),
                    "timeout_seconds": 5
                }),
                assistant_summary: Some("read package manifest".to_string()),
            },
        ]),
    );
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "review the project",
        PermissionPolicyMode::AutoCreateReviewModify,
    );

    let shell = session
        .actions()
        .iter()
        .find_map(|record| match record.verified_result.as_ref()? {
            VerifiedActionResult::Shell(shell) => Some(shell),
            _ => None,
        })
        .expect("shell action should be verified");
    assert_eq!(shell.command, "cat package.json");
    assert_eq!(shell.stdout, "{}\n");
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("rewrote read-only shell fallback"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn display_only_project_tree_stops_after_verified_listing() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-display-tree-stops",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}\n",
    )
    .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![crate::event::ProviderOutput::new(
            "{\"route\":\"execute\"}",
        )])
        .with_tool_outputs(vec![crate::event::ProviderOutput::new(
            "List project files.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "shell-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({
                "command": "find . -maxdepth 3 -not -path './node_modules/*' -print",
                "cwd": root.display().to_string()
            }),
            assistant_summary: Some("list files".to_string()),
        }])]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "show me the project tree");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if matches!(
                started.request_mode.as_deref(),
                Some("tool_result_synthesis")
            )
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("display-only shell result"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn display_only_file_read_stops_after_verified_code_panel() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-display-read-stops",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}\n",
    )
    .unwrap();
    let provider =
        CapturingProvider::new().with_tool_outputs(vec![crate::event::ProviderOutput::new(
            "Read file.",
        )
        .with_tool_calls(vec![RawModelToolCall {
            id: "shell-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({
                "command": "cat app/page.tsx",
                "cwd": root.display().to_string()
            }),
            assistant_summary: Some("read file".to_string()),
        }])]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "read app/page.tsx");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
    )));
    assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(&applied.result, VerifiedActionResult::Shell(shell) if shell.stdout.contains("Home"))
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn file_read_with_followup_question_synthesizes_after_verified_code_panel() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-read-and-tell-synthesizes",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/page.tsx"),
        "export default function Home() { return <main>Hello</main> }\n",
    )
    .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![crate::event::ProviderOutput::new(
            "The page renders a main element with Hello.",
        )])
        .with_tool_outputs(vec![crate::event::ProviderOutput::new("Read file.")
            .with_tool_calls(vec![RawModelToolCall {
                id: "shell-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "cat app/page.tsx",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("read file".to_string()),
            }])]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "read app/page.tsx and tell me what the page renders",
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_result_synthesis")
                && started.tool_count == Some(0)
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("main element with Hello")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plain_chat_writes_redacted_local_trace_without_tools_or_memory() {
    std::env::set_var("ELGAR_TRACE", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plain-trace",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"chat\",\"content\":\"Plain answer.\"}",
    ));
    let mut session = Session::new("trace-session", &root, &root);

    let result = run_permissive_agent_turn(
        &provider,
        &mut session,
        "hello secret-user-prompt-that-must-not-be-traced",
    );

    assert_eq!(result.route, Route::AskModel);
    let events = trace_events(&root, "trace-session");
    let kinds = trace_kinds(&events);
    assert!(kinds.contains(&"turn_start".to_string()));
    assert!(kinds.contains(&"provider_request_start".to_string()));
    assert!(kinds.contains(&"provider_request_finish".to_string()));
    assert!(kinds.contains(&"route_decision".to_string()));
    assert!(kinds.contains(&"turn_finish".to_string()));
    assert!(!kinds.contains(&"memory_selected".to_string()));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("secret-user-prompt-that-must-not-be-traced"));
    assert!(events.iter().any(|event| {
        event.get("kind").and_then(serde_json::Value::as_str) == Some("provider_request_start")
            && event
                .get("metadata")
                .and_then(|metadata| metadata.get("tool_count"))
                .and_then(serde_json::Value::as_u64)
                == Some(0)
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tool_action_turn_writes_action_trace_without_file_contents() {
    std::env::set_var("ELGAR_TRACE", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-tool-trace",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "secret-file-contents-that-must-not-be-traced",
                    }),
                    assistant_summary: None,
                },
            ]),
        );
    let mut session = Session::new("trace-session-tool", &root, &root);

    let result = run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a file with secret-file-contents-that-must-not-be-traced",
    );

    assert_eq!(result.route, Route::AskModel);
    assert!(root.join("notes.txt").is_file());
    let events = trace_events(&root, "trace-session-tool");
    let kinds = trace_kinds(&events);
    assert!(kinds.contains(&"tool_call_validated".to_string()));
    assert!(kinds.contains(&"policy_decision".to_string()));
    assert!(kinds.contains(&"action_approved".to_string()));
    assert!(kinds.contains(&"action_applied".to_string()));
    assert!(kinds.contains(&"turn_finish".to_string()));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("secret-file-contents-that-must-not-be-traced"));
    assert!(serialized.contains("notes.txt"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_writes_trace_metadata_without_prompt_text() {
    std::env::set_var("ELGAR_TRACE", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-trace",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("TracePlan");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("plan.md"), "# Trace Plan\n").unwrap();
    std::fs::write(project.join("README.md"), "# Trace\n").unwrap();
    std::fs::write(project.join("src/main.py"), "print('trace')\n").unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"status\"}",
    ));
    let mut session = Session::new("trace-session-state", &root, &root);
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: project.join("plan.md"),
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: vec![root.join("TracePlan/src")],
        expected_files: vec![
            root.join("TracePlan/README.md"),
            root.join("TracePlan/src/main.py"),
        ],
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "did you execute the trace plan secret-state-prompt",
    );

    let events = trace_events(&root, "trace-session-state");
    let state_answer = events
        .iter()
        .find(|event| event.get("kind").and_then(serde_json::Value::as_str) == Some("state_answer"))
        .expect("state answer trace should be written");
    let metadata = state_answer
        .get("metadata")
        .expect("state answer should include metadata");
    assert_eq!(
        metadata
            .get("state_answer_kind")
            .and_then(serde_json::Value::as_str),
        Some("status")
    );
    assert_eq!(
        metadata
            .get("plan_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        metadata
            .get("answer_scope")
            .and_then(serde_json::Value::as_str),
        Some("session_status")
    );
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("secret-state-prompt"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plain_chat_writes_redacted_append_only_session_log() {
    std::env::set_var("ELGAR_SESSION_LOG", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plain-session-log",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"chat\",\"content\":\"Plain answer secret-assistant-log-content.\"}",
    ));
    let mut session = Session::new("session-log-plain", &root, &root);

    let result = run_permissive_agent_turn(
        &provider,
        &mut session,
        "hello secret-user-session-log-content",
    );

    assert_eq!(result.route, Route::AskModel);
    let events = session_log_events(&root, "session-log-plain");
    let kinds = trace_kinds(&events);
    assert!(kinds.contains(&"turn_start".to_string()));
    assert!(kinds.contains(&"user_message".to_string()));
    assert!(kinds.contains(&"provider_request_start".to_string()));
    assert!(kinds.contains(&"provider_request_finish".to_string()));
    assert!(kinds.contains(&"route_decision".to_string()));
    assert!(kinds.contains(&"assistant_message".to_string()));
    assert!(kinds.contains(&"turn_finish".to_string()));
    assert!(!kinds.contains(&"memory_selected".to_string()));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("secret-user-session-log-content"));
    assert!(!serialized.contains("secret-assistant-log-content"));
    assert!(events.iter().all(|event| {
        event.get("session_id").and_then(serde_json::Value::as_str) == Some("session-log-plain")
            && event.get("turn_index").and_then(serde_json::Value::as_u64) == Some(1)
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tool_action_turn_writes_session_log_without_file_contents() {
    std::env::set_var("ELGAR_SESSION_LOG", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-tool-session-log",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "secret-session-log-file-contents",
                    }),
                    assistant_summary: None,
                },
            ]),
        );
    let mut session = Session::new("session-log-tool", &root, &root);

    let result = run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a file with secret-session-log-file-contents",
    );

    assert_eq!(result.route, Route::AskModel);
    assert!(root.join("notes.txt").is_file());
    let events = session_log_events(&root, "session-log-tool");
    let kinds = trace_kinds(&events);
    assert!(kinds.contains(&"tool_call_validated".to_string()));
    assert!(kinds.contains(&"policy_decision".to_string()));
    assert!(kinds.contains(&"action_approved".to_string()));
    assert!(kinds.contains(&"action_applied".to_string()));
    assert!(kinds.contains(&"turn_finish".to_string()));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("secret-session-log-file-contents"));
    assert!(serialized.contains("notes.txt"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_resolves_empty_latest_folder_to_project_files() {
    std::env::set_var("ELGAR_TRACE", "on");
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-resolver-project-files",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("PostNewSmoke1");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("tests")).unwrap();
    std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
    std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
    std::fs::write(project.join("requirements.txt"), "").unwrap();
    std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
    std::fs::write(
        project.join("tests/test_main.py"),
        "def test_main(): pass\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
    ));
    let mut session = Session::new("trace-session-resolver-project", &root, &root);
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: project.join("PLAN.md"),
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Completed,
        expected_directories: vec![
            root.join("PostNewSmoke1/src"),
            root.join("PostNewSmoke1/tests"),
        ],
        expected_files: vec![
            root.join("PostNewSmoke1/README.md"),
            root.join("PostNewSmoke1/requirements.txt"),
            root.join("PostNewSmoke1/src/main.py"),
            root.join("PostNewSmoke1/tests/test_main.py"),
        ],
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "What files did you create in playground/PostNewSmoke1?",
    );

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("project: PostNewSmoke1")
                && message.content.contains("files: 4/4 present")
                && message.content.contains("PostNewSmoke1/src/main.py")
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("No verified folder creation recorded")
    )));

    let events = trace_events(&root, "trace-session-resolver-project");
    let state_answer = events
        .iter()
        .find(|event| event.get("kind").and_then(serde_json::Value::as_str) == Some("state_answer"))
        .expect("state answer trace should be written");
    let metadata = state_answer
        .get("metadata")
        .expect("state answer should include metadata");
    assert_eq!(
        metadata
            .get("requested_state_answer_kind")
            .and_then(serde_json::Value::as_str),
        Some("latest_folder")
    );
    assert_eq!(
        metadata
            .get("resolved_state_answer_kind")
            .and_then(serde_json::Value::as_str),
        Some("project_files")
    );
    assert_eq!(
        metadata
            .get("state_answer_fallback_reason")
            .and_then(serde_json::Value::as_str),
        Some("requested_latest_folder_with_referenced_project_files")
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_resolves_broad_created_summary_to_referenced_project_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-resolver-created-project",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("PostNewSmoke2");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("tests")).unwrap();
    std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
    std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
    std::fs::write(project.join("requirements.txt"), "").unwrap();
    std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
    std::fs::write(
        project.join("tests/test_main.py"),
        "def test_main(): pass\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"created_summary\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: project.join("PLAN.md"),
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Completed,
        expected_directories: vec![
            root.join("PostNewSmoke2/src"),
            root.join("PostNewSmoke2/tests"),
        ],
        expected_files: vec![
            root.join("PostNewSmoke2/README.md"),
            root.join("PostNewSmoke2/requirements.txt"),
            root.join("PostNewSmoke2/src/main.py"),
            root.join("PostNewSmoke2/tests/test_main.py"),
        ],
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "What files did you create in PostNewSmoke2?",
    );

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("project: PostNewSmoke2")
                && message.content.contains("files: 4/4 present")
                && message.content.contains("PostNewSmoke2/tests/test_main.py")
                && !message.content.contains("current session:")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_keeps_empty_latest_folder_without_better_state() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-empty-latest-folder",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
    ));
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "what is the latest folder?");

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == "No verified folder creation recorded."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_latest_folder_reports_latest_project_root_without_file_fallback() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-latest-project-folder",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("StateResolverSmoke4");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("tests")).unwrap();
    std::fs::write(project.join("PLAN.md"), "# Plan\n").unwrap();
    std::fs::write(project.join("README.md"), "# Notes\n").unwrap();
    std::fs::write(project.join("requirements.txt"), "").unwrap();
    std::fs::write(project.join("src/main.py"), "print('hi')\n").unwrap();
    std::fs::write(
        project.join("tests/test_main.py"),
        "def test_main(): pass\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: project.join("PLAN.md"),
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Completed,
        expected_directories: vec![
            root.join("StateResolverSmoke4/src"),
            root.join("StateResolverSmoke4/tests"),
        ],
        expected_files: vec![
            root.join("StateResolverSmoke4/README.md"),
            root.join("StateResolverSmoke4/requirements.txt"),
            root.join("StateResolverSmoke4/src/main.py"),
            root.join("StateResolverSmoke4/tests/test_main.py"),
        ],
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "What is the latest folder you created?",
    );

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == "StateResolverSmoke4"
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_answer_resolves_empty_kind_to_created_summary_for_artifacts_without_project() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-answer-resolver-created-summary",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    push_verified_file_record(&mut session, "action-file", "standalone.txt");

    run_permissive_agent_turn(&provider, &mut session, "what did you create?");

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == "current session:\n- file standalone.txt"
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn normal_text_model_execute_decision_enters_tool_loop_without_slash_command() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-normal-execute-decision",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "normal-execute-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                    arguments: json!({ "target_path": "model-selected-folder" }),
                    assistant_summary: Some("create model-selected folder".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "please handle this request");

    assert!(root.join("model-selected-folder").is_dir());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert!(requests[1].tool_count > 0);
    assert!(requests[2]
        .messages
        .iter()
        .any(|message| matches!(message.role, ChatRole::Tool)));
    assert!(session.events().iter().any(|event| matches!(
            event,
            Event::ActionApplied(applied)
                if matches!(
                    &applied.result,
                    VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated { path })
                        if path.ends_with("model-selected-folder")
                )
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unstructured_route_response_retries_json_before_accepting_chat() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-route-json-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    "# Project Plan\n\n```text\nRepairPlan/\nREADME.md\n```\n\n## Verification\n- Check files.\n\n## Acceptance Criteria\n- Files exist.\n",
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"done\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "route-repair-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "RepairPlan/PLAN.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check README.md, src/main.py, and requirements.txt exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: root.join("Demo"),
        source_action_id: "action-folder".to_string(),
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan for a tiny app",
    );

    assert!(root.join("RepairPlan/PLAN.md").is_file());
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].tool_count, 0);
    assert!(
        joined_request_messages(&requests[1])
            .contains("previous no-tool routing response was not valid route JSON")
            || joined_request_messages(&requests[1])
                .contains("Previous no-tool routing response was not valid route JSON")
    );
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    assert!(requests[2].tool_count > 0);
    assert_eq!(requests[3].mode, CapturedProviderRequestMode::Plain);
    assert!(joined_request_messages(&requests[3]).contains("A verified plan was just created"));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("# Project Plan")
                && message.source == AssistantMessageSource::Provider
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("retrying route JSON")));
    assert_eq!(trace.route.as_deref(), Some("plan_creation"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn artifact_like_chat_route_retries_before_rendering_plan_json() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-artifact-chat-route-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let artifact_json = format!(
        "{{\"project_name\":\"Demo\",\"files\":[{}]}}",
        "\"README.md\",\"src/main.py\",\"requirements.txt\",".repeat(80)
    );
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": artifact_json,
                    })
                    .to_string(),
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "artifact-chat-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ArtifactChatPlan/plan.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: root.join("Demo"),
        source_action_id: "action-folder".to_string(),
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan for a tiny app",
    );

    assert!(root.join("ArtifactChatPlan/plan.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("\"project_name\"")
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("artifact-like chat")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn local_file_work_chat_route_retries_before_rendering_fake_claim() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-local-file-chat-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                r#"{"route":"chat","content":"Created USAGE.md with the specified content."}"#,
            ),
            crate::event::ProviderOutput::new(
                r#"{"route":"chat","content":"Created USAGE.md in the project root."}"#,
            ),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating USAGE.md.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "usage-file".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "Demo/USAGE.md",
                        "contents": "PYTHONPATH=src python -m demo.cli sample.txt\n"
                    }),
                    assistant_summary: Some("create usage file".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: root.join("Demo"),
        source_action_id: "action-folder".to_string(),
    });

    run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create USAGE.md inside Demo containing exact text PYTHONPATH=src python -m demo.cli sample.txt.",
        );

    assert!(root.join("Demo/USAGE.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Created USAGE.md")
    )));
    let requests = provider.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Plain)
            .count(),
        2
    );
    assert!(joined_request_messages(&requests[1]).contains("local filesystem or shell syntax"));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .model_decisions
        .iter()
        .any(|line| line.contains("local work-shaped input"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shell_work_chat_route_retries_before_rendering_fake_claim() {
    assert!(input_has_run_prefixed_command_shape(
        "run python -m compileall src inside that project"
    ));
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-chat-repair",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    let expected_file = root.join("compile.out");
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                r#"{"route":"chat","content":"Compiled all Python files successfully."}"#,
            ),
            crate::event::ProviderOutput::new(
                r#"{"route":"chat","content":"I executed the compile command successfully."}"#,
            ),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Running compile command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "compile-shell".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf ok > compile.out",
                        "cwd": root.display().to_string(),
                        "expected_file": expected_file.display().to_string()
                    }),
                    assistant_summary: Some("run verification command".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_folder_reference(VerifiedFolderReference {
        path: root.join("Demo"),
        source_action_id: "action-folder".to_string(),
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "run python -m compileall src inside that project",
    );

    assert_eq!(std::fs::read_to_string(&expected_file).unwrap(), "ok");
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Compiled all Python")
    )));
    assert!(session.actions().iter().any(|record| {
        matches!(
            record.verified_result.as_ref(),
            Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(0)
        )
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn compact_json_plan_chat_routes_to_execute_instead_of_rendering() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-compact-json-artifact-chat",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let compact_plan_json = r#"{
  "project_name": "CompactJsonPlan",
  "structure": {
    "README.md": "Project overview.",
    "src/main.py": "CLI entry point.",
    "requirements.txt": "Runtime dependencies."
  },
  "verification": "Run python src/main.py --help.",
  "acceptance_criteria": ["All listed files exist."]
}"#;
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": compact_plan_json,
                    })
                    .to_string(),
                ),
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "compact-json-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CompactJsonPlan/PLAN.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Run python src/main.py --help after execution.\n\n## Acceptance Criteria\n- All listed files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan for a tiny app",
    );

    assert!(root.join("CompactJsonPlan/PLAN.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("\"project_name\"")
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("artifact-like chat")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn numbered_tree_plan_chat_counts_local_paths_as_artifact_shape() {
    let artifact_markdown = r#"Project Plan: Tiny Python CLI Todo App
1. Folder Structure
   - playground/ManualEfficiencyFollowupCodex3/
     ├── README.md
     ├── src/
     │   └── main.py
     └── requirements.txt`
2. README.md
   - Project title and brief description.
3. src/main.py
   - Entry point for CLI using argparse.
4. requirements.txt
   - No external dependency.
5. Verification & Acceptance Criteria
   - python src/main.py add "Task description" adds a task.
   - python src/main.py list displays all tasks with IDs.
"#;

    assert!(local_path_like_token_count(artifact_markdown) >= 3);
    assert!(numbered_artifact_line_count(artifact_markdown) >= 4);
    assert!(looks_like_misrouted_artifact_chat(artifact_markdown));
    assert!(looks_like_misrouted_artifact_chat_after_retry(
        artifact_markdown
    ));
}

#[test]
fn short_numbered_plan_chat_counts_as_artifact_shape() {
    let artifact_markdown = r#"Plan:
1. Create folder playground/same-prompt-plan-execute-1.
2. Create README.md explaining project.
3. Create calculator.py with functions.
4. Create ui.py for a small CLI.
5. Add python -m unittest verification.
6. Create test_calculator.py to verify functions.
"#;

    assert!(local_path_like_token_count(artifact_markdown) >= 3);
    assert!(numbered_artifact_line_count(artifact_markdown) >= 4);
    assert!(looks_like_misrouted_artifact_chat(artifact_markdown));
    assert!(looks_like_misrouted_artifact_chat_after_retry(
        artifact_markdown
    ));
}

#[test]
fn artifact_like_chat_after_route_retry_executes_instead_of_rendering() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-artifact-chat-after-route-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let artifact_markdown = format!(
            "{}\n{}",
            "File | Purpose\nREADME.md | docs\nsrc/main.py | CLI\nrequirements.txt | deps\nacceptance_criteria.md | checks\n",
            "Describe setup, usage, verification, and acceptance details.\n".repeat(8)
        );
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("Project plan artifact"),
                crate::event::ProviderOutput::new(
                    serde_json::json!({
                        "route": "chat",
                        "content": artifact_markdown,
                    })
                    .to_string(),
                ),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "artifact-chat-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "ArtifactChatRetryPlan/plan.md",
                            "contents": "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan for a tiny app",
    );

    assert!(root.join("ArtifactChatRetryPlan/plan.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("acceptance_criteria.md")
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert_eq!(trace.route.as_deref(), Some("plan_creation"));
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("compact artifact-like chat after retry")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn raw_artifact_text_after_route_retry_executes_instead_of_erroring() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-raw-artifact-after-route-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let artifact_markdown = format!(
            "{}\n{}",
            r#"Project Plan - Tiny Notes CLI

File tree:
playground/RawArtifactRetryPlan/
├── README.md
├── requirements.txt
└── src/
    └── main.py

Verification:
- Check that README.md exists.
- Check that requirements.txt exists.
- Check that src/main.py exists.

Acceptance Criteria:
- The plan file exists.
- The future implementation files are listed.
"#,
            "The README should document installation, usage, verification, and acceptance criteria for the future implementation.\n".repeat(8)
        );
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("I will draft the project plan."),
                crate::event::ProviderOutput::new(artifact_markdown),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating only the plan file.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "raw-artifact-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/RawArtifactRetryPlan/PLAN.md",
                            "contents": "# Raw Artifact Retry Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files are listed.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan for a tiny notes cli",
    );

    assert!(root
        .join("playground/RawArtifactRetryPlan/PLAN.md")
        .is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::Error(error)
            if error
                .message
                .contains("Model routing response was not valid JSON")
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert_eq!(trace.route.as_deref(), Some("plan_creation"));
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("raw artifact-like text after retry")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn newly_created_plan_is_not_executed_in_same_turn_even_with_execution_intent() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-execution-intent",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan and files.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-exec-intent-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/plan.md",
                                "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-exec-intent-readme-too-early".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/README.md",
                                "contents": "# Calculator UI\n"
                            }),
                            assistant_summary: Some("create README too early".to_string()),
                        },
                    ],
                ),
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-exec-intent-readme".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/README.md",
                            "contents": "# Calculator UI\n"
                        }),
                        assistant_summary: Some("create README".to_string()),
                    },
                    RawModelToolCall {
                        id: "plan-exec-intent-calc".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/calculator.py",
                            "contents": "class Calculator:\n    pass\n"
                        }),
                        assistant_summary: Some("create calculator".to_string()),
                    },
                    RawModelToolCall {
                        id: "plan-exec-intent-ui".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/ui.py",
                            "contents": "from calculator import Calculator\n"
                        }),
                        assistant_summary: Some("create ui".to_string()),
                    },
                ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "execute the plan you just created");

    assert!(root.join("CalculatorUI/plan.md").is_file());
    assert!(!root.join("CalculatorUI/README.md").exists());
    assert!(!root.join("CalculatorUI/calculator.py").exists());
    assert!(!root.join("CalculatorUI/ui.py").exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should be recorded");
    assert_eq!(
        plan.status,
        crate::session::StructuredProjectPlanStatus::Verified
    );
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("execute intent plan_execution")));
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("plan creation completed; skipped final provider synthesis")));
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line
            .contains("Skipped extra implementation tool calls in this plan-creation turn")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_plan_creation_execution_intent_can_create_plan_then_files_same_turn() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-create-execute-intent",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-create-execute-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/plan.md",
                            "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files from plan.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-create-execute-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/README.md",
                                "contents": "# Calculator UI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-create-execute-calc".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/calculator.py",
                                "contents": "class Calculator:\n    pass\n"
                            }),
                            assistant_summary: Some("create calculator".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-create-execute-ui".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "CalculatorUI/ui.py",
                                "contents": "from calculator import Calculator\n"
                            }),
                            assistant_summary: Some("create ui".to_string()),
                        },
                    ],
                ),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a plan for the calculator UI and execute it",
    );

    assert!(root.join("CalculatorUI/plan.md").is_file());
    assert!(root.join("CalculatorUI/README.md").is_file());
    assert!(root.join("CalculatorUI/calculator.py").is_file());
    assert!(root.join("CalculatorUI/ui.py").is_file());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should be recorded");
    assert_eq!(
        plan.status,
        crate::session::StructuredProjectPlanStatus::Completed
    );
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .model_decisions
        .iter()
        .any(|line| line.contains("execute intent plan_creation_execution")));
    assert!(trace.runtime_checks.iter().any(|line| line
        .contains("new verified plan created during explicit plan creation execution turn")));
    let requests = provider.requests();
    assert!(requests
        .iter()
        .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
        .skip(1)
        .any(|request| joined_request_messages(request)
            .contains("call the needed file and directory tools in one assistant response")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_plan_creation_execution_can_use_create_files_batch() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-create-execute-batch",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "batch-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "NotesCLI/plan.md",
                            "contents": "# Notes Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python -m src.main`.\n- Run `pytest tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist and tests pass.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "batch-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["NotesCLI/src", "NotesCLI/tests"],
                            "files": [
                                {
                                    "target_path": "NotesCLI/README.md",
                                    "contents": "# Notes CLI\n"
                                },
                                {
                                    "target_path": "NotesCLI/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "NotesCLI/src/main.py",
                                    "contents": "def main():\n    print('notes')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "NotesCLI/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a notes project plan and execute it",
    );

    for path in [
        "NotesCLI/plan.md",
        "NotesCLI/README.md",
        "NotesCLI/requirements.txt",
        "NotesCLI/src/main.py",
        "NotesCLI/tests/test_main.py",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(
        session.actions().len(),
        5,
        "plan plus four expected files should be applied without another provider round"
    );
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn generic_execute_plan_creation_can_post_decide_to_execute_same_turn() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-post-plan-execute",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new(
                    "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
                ),
            ])
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-decision-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "PostDecision/PLAN.md",
                            "contents": "# Post Decision Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-decision-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": ["PostDecision/src"],
                            "files": [
                                {
                                    "target_path": "PostDecision/README.md",
                                    "contents": "# Post Decision\n"
                                },
                                {
                                    "target_path": "PostDecision/src/main.py",
                                    "contents": "print('ok')\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a project plan, then execute it",
    );

    for path in [
        "PostDecision/PLAN.md",
        "PostDecision/README.md",
        "PostDecision/src/main.py",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
    assert!(joined_request_messages(&requests[2]).contains("A verified plan was just created"));
    assert_eq!(requests[3].mode, CapturedProviderRequestMode::Tool);
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .model_decisions
        .iter()
        .any(|line| line.contains("post-plan classifier selected plan execution"))));
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should be recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_execution_create_files_batch_does_not_reclassify_readme_as_new_plan() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-readme-planish",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(cwd.join("GemmaReadingTracker1")).unwrap();
    std::fs::write(
            cwd.join("GemmaReadingTracker1/plan.md"),
            "# Project Plan\n\n```text\nREADME.md\nrequirements.txt\nmain.py\ntracker/__init__.py\ntracker/models.py\ntracker/storage.py\ntracker/cli.py\ntests/__init__.py\ntests/test_models.py\ntests/test_storage.py\n```\n\n## Verification\n- Run pytest.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(
                    vec![RawModelToolCall {
                        id: "reading-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "GemmaReadingTracker1/tracker",
                                "GemmaReadingTracker1/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "GemmaReadingTracker1/README.md",
                                    "contents": "# Project Plan\n\nThis README describes the reading tracker project plan and usage.\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/requirements.txt",
                                    "contents": "pytest\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/main.py",
                                    "contents": "from tracker.cli import main\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/__init__.py",
                                    "contents": ""
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/models.py",
                                    "contents": "class Book:\n    pass\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/storage.py",
                                    "contents": "def load_books():\n    return []\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tracker/cli.py",
                                    "contents": "def main():\n    print('reading tracker')\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/__init__.py",
                                    "contents": ""
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/test_models.py",
                                    "contents": "def test_model_smoke():\n    assert True\n"
                                },
                                {
                                    "target_path": "GemmaReadingTracker1/tests/test_storage.py",
                                    "contents": "def test_storage_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create reading tracker files".to_string()),
                    }],
                ),
            );
    let mut session = Session::new("session", &root, &cwd);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("GemmaReadingTracker1/plan.md"),
            contents: "# Project Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "GemmaReadingTracker1/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    for path in [
        "GemmaReadingTracker1/README.md",
        "GemmaReadingTracker1/requirements.txt",
        "GemmaReadingTracker1/main.py",
        "GemmaReadingTracker1/tracker/__init__.py",
        "GemmaReadingTracker1/tracker/models.py",
        "GemmaReadingTracker1/tracker/storage.py",
        "GemmaReadingTracker1/tracker/cli.py",
        "GemmaReadingTracker1/tests/__init__.py",
        "GemmaReadingTracker1/tests/test_models.py",
        "GemmaReadingTracker1/tests/test_storage.py",
    ] {
        assert!(cwd.join(path).is_file(), "missing {path}");
    }
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should remain recorded");
    assert_eq!(
        plan.source_plan_path,
        cwd.join("GemmaReadingTracker1/plan.md")
    );
    assert_eq!(
        plan.runtime_status(),
        StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_execution_blocks_implementation_when_new_plan_needs_repair() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-create-exec-bad-plan",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating files before the plan.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "bad-plan-early-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README too early".to_string()),
                        },
                        RawModelToolCall {
                            id: "bad-plan-file".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/project_plan.txt",
                                "contents": "Create README.md, requirements.txt, src/main.py, and tests/test_main.py.\n"
                            }),
                            assistant_summary: Some("create incomplete plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "bad-plan-requirements".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create requirements too early".to_string()),
                        },
                    ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project, execute it, and run verification",
    );

    assert!(root.join("GreeterCLI/project_plan.txt").is_file());
    assert!(!root.join("GreeterCLI/README.md").exists());
    assert!(!root.join("GreeterCLI/requirements.txt").exists());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("The plan needs revision before execution")
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Skipped non-plan repair action"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_prompt_project_root_anchors_bare_project_paths() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-prompt-root-anchor",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating project with bare paths.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "prompt-root-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "project_plan.md",
                                "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- Expected files exist and compile.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "prompt-root-files".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFiles),
                            arguments: json!({
                                "directories": ["src", "tests"],
                                "files": [
                                    {
                                        "target_path": "README.md",
                                        "contents": "# Greeter CLI\n"
                                    },
                                    {
                                        "target_path": "requirements.txt",
                                        "contents": ""
                                    },
                                    {
                                        "target_path": "src/main.py",
                                        "contents": "def main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n"
                                    },
                                    {
                                        "target_path": "tests/test_main.py",
                                        "contents": "def test_smoke():\n    assert True\n"
                                    }
                                ]
                            }),
                            assistant_summary: Some("create files".to_string()),
                        },
                    ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project inside playground/GreeterPromptRoot and execute it",
    );

    for path in [
        "playground/GreeterPromptRoot/project_plan.md",
        "playground/GreeterPromptRoot/README.md",
        "playground/GreeterPromptRoot/requirements.txt",
        "playground/GreeterPromptRoot/src/main.py",
        "playground/GreeterPromptRoot/tests/test_main.py",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
    assert!(!root.join("project_plan.md").exists());
    assert!(!root.join("README.md").exists());
    assert!(!root.join("src/main.py").exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should be recorded");
    assert_eq!(plan.project_root, root.join("playground/GreeterPromptRoot"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_prompt_project_root_deduplicates_cwd_relative_prefix() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-prompt-root-dedupe-cwd",
        std::process::id()
    ));
    let cwd = root.join("playground");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&cwd).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating a plan at the requested root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "dedupe-root-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/CoreSolidNotes1/PLAN.md",
                            "contents": "# Notes Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Do not run shell commands.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
            );
    let mut session = Session::new("session", &root, &cwd);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "Create only a project plan. The project root must be exactly playground/CoreSolidNotes1.",
    );

    assert!(cwd.join("CoreSolidNotes1/PLAN.md").is_file());
    assert!(!cwd.join("playground/CoreSolidNotes1/PLAN.md").exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should be recorded without duplicated cwd prefix");
    assert_eq!(plan.project_root, cwd.join("CoreSolidNotes1"));
    assert_eq!(plan.source_plan_path, cwd.join("CoreSolidNotes1/PLAN.md"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_prompt_project_root_rebases_sibling_project_paths() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-prompt-root-rebase-sibling",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan under the wrong sibling root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "sibling-root-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/GemmaBookmarkManager5/PLAN.md",
                            "contents": "# Bookmark Manager Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The bookmark manager files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    }]),
                crate::event::ProviderOutput::new("Creating files under the same wrong sibling root.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "sibling-root-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "playground/GemmaBookmarkManager5/src",
                                "playground/GemmaBookmarkManager5/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/README.md",
                                    "contents": "# Bookmark Manager\n"
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/src/main.py",
                                    "contents": "def main():\n    print('bookmarks')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "playground/GemmaBookmarkManager5/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    }]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
            &provider,
            &mut session,
            "Create a complete bookmark manager project. The project root must be exactly playground/GemmaBookmarkManagerSamePrompt5. First create a project plan, then execute it.",
        );

    for path in [
        "playground/GemmaBookmarkManagerSamePrompt5/PLAN.md",
        "playground/GemmaBookmarkManagerSamePrompt5/README.md",
        "playground/GemmaBookmarkManagerSamePrompt5/requirements.txt",
        "playground/GemmaBookmarkManagerSamePrompt5/src/main.py",
        "playground/GemmaBookmarkManagerSamePrompt5/tests/test_main.py",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
    assert!(!root
        .join("playground/GemmaBookmarkManager5/PLAN.md")
        .exists());
    assert!(!root
        .join("playground/GemmaBookmarkManagerSamePrompt5/playground/GemmaBookmarkManager5/PLAN.md")
        .exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("rebased plan should be recorded");
    assert_eq!(
        plan.project_root,
        root.join("playground/GemmaBookmarkManagerSamePrompt5")
    );
    assert_eq!(
        plan.runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_execution_requires_plan_before_implementation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-required-first",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Implementing without a plan.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "plan-required-early-shell".to_string(),
                        name: RawModelToolName::Known(ModelToolName::ShellCommand),
                        arguments: json!({
                            "command": "printf '# Greeter CLI\\n' > README.md",
                            "cwd": "GreeterCLI",
                            "expected_file": "README.md"
                        }),
                        assistant_summary: Some("create README too early".to_string()),
                    }]),
                crate::event::ProviderOutput::new("Creating plan and files.").with_tool_calls(
                    vec![
                        RawModelToolCall {
                            id: "plan-required-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/plan.md",
                                "contents": "# Greeter Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py`.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-required-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                        RawModelToolCall {
                            id: "plan-required-main".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/src/main.py",
                                "contents": "print('hello')\n"
                            }),
                            assistant_summary: Some("create main".to_string()),
                        },
                    ],
                ),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project, first create a plan, then execute it",
    );

    assert!(root.join("GreeterCLI/plan.md").is_file());
    assert!(root.join("GreeterCLI/README.md").is_file());
    assert!(root.join("GreeterCLI/src/main.py").is_file());
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Create the project plan file first")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_execution_plain_create_without_plan_is_applied() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-adhoc-create-plan-intent",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating notes.txt.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "adhoc-create-notes".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "hello world\n"
                    }),
                    assistant_summary: Some("create notes".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a file notes.txt with the text hello world",
    );

    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "hello world\n"
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(&applied.result, VerifiedActionResult::FileWritten { path }
                if path.ends_with("notes.txt"))
                || matches!(&applied.result, VerifiedActionResult::File(
                    crate::event::FileActionVerification::FileCreated { path }
                ) if path.ends_with("notes.txt"))
    )));
    assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Create the project plan file first"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_execution_plain_create_after_completed_plan_is_applied() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-post-plan-create",
        std::process::id()
    ));
    let project = root.join("Demo");
    let plan_path = project.join("plan.md");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(&plan_path, "# Demo Plan\n").unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating usage.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "post-plan-usage".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "Demo/USAGE.md",
                        "contents": "PYTHONPATH=src python -m text_tools.cli sample.txt.\n"
                    }),
                    assistant_summary: Some("create usage".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Completed,
        expected_directories: vec![project.clone()],
        expected_files: Vec::new(),
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "Create USAGE.md inside Demo containing a command line.",
    );

    assert_eq!(
        std::fs::read_to_string(project.join("USAGE.md")).unwrap(),
        "PYTHONPATH=src python -m text_tools.cli sample.txt.\n"
    );
    assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Create the project plan file first"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_only_does_not_mark_plan_executing() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-only-not-executing",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-only-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "myapp/plan.md",
                            "contents": "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("No further tool actions."),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a plan in ./myapp for a tiny script that prints hi",
    );

    assert!(root.join("myapp/plan.md").is_file());
    assert!(!root.join("myapp/script.py").exists());
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should be recorded");
    assert_eq!(
        plan.runtime_status(),
        crate::session::StructuredProjectPlanStatus::Verified
    );
    assert_ne!(
        plan.status,
        crate::session::StructuredProjectPlanStatus::Executing
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_plan_preflight_allows_unrelated_non_execution_create() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-preflight-unrelated",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let plan_provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating the plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "unrelated-plan-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "myapp/plan.md",
                            "contents": "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("No further tool actions."),
            ]);
    let mut session = Session::new("session", &root, &root);
    run_permissive_agent_turn(
        &plan_provider,
        &mut session,
        "create a plan in ./myapp for a tiny script that prints hi",
    );

    let create_provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating unrelated file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "unrelated-create".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "other/x.txt",
                        "contents": "outside plan\n"
                    }),
                    assistant_summary: Some("create unrelated file".to_string()),
                },
            ]),
        );

    run_permissive_agent_turn(
        &create_provider,
        &mut session,
        "create file other/x.txt outside the plan",
    );

    assert_eq!(
        std::fs::read_to_string(root.join("other/x.txt")).unwrap(),
        "outside plan\n"
    );
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Verified
    );
    assert!(!session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("verified plan is rooted"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_last_block_reports_latest_preflight_block() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-last-block-state",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("myapp");
    std::fs::create_dir_all(&project).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Tiny Script Plan\n\n```text\nscript.py\n```\n\n## Verification\n- Run `python script.py`.\n\n## Acceptance Criteria\n- `script.py` prints hi.\n",
        )
        .unwrap();
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: Vec::new(),
        expected_files: vec![project.join("script.py")],
    });
    let blocked_provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating plan file and unrelated file.")
                .with_tool_calls(vec![
                    RawModelToolCall {
                        id: "blocked-plan-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "myapp/script.py",
                            "contents": "print('hi')\n"
                        }),
                        assistant_summary: Some("create expected script".to_string()),
                    },
                    RawModelToolCall {
                        id: "blocked-outside-file".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "other/x.txt",
                            "contents": "outside plan\n"
                        }),
                        assistant_summary: Some("create outside file".to_string()),
                    },
                ]),
        );

    run_permissive_agent_turn(
        &blocked_provider,
        &mut session,
        "execute the verified plan and create other/x.txt too",
    );

    assert!(!project.join("script.py").exists());
    assert!(!root.join("other/x.txt").exists());
    let block = session
        .latest_runtime_block()
        .expect("preflight block should be recorded")
        .message
        .clone();
    assert!(block.contains("verified plan is rooted"));
    assert!(block.contains("other/x.txt"));

    let state_provider = CapturingProvider::new().with_plain_output(
        crate::event::ProviderOutput::new("{\"route\":\"state\",\"answer_kind\":\"last_block\"}"),
    );

    run_permissive_agent_turn(
        &state_provider,
        &mut session,
        "why was the previous request blocked?",
    );

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == block
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn chat_route_after_runtime_block_retries_state_routing() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-block-chat-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_outputs(vec![
        crate::event::ProviderOutput::new(
            "{\"route\":\"chat\",\"content\":\"I need more details.\"}",
        ),
        crate::event::ProviderOutput::new("{\"route\":\"chat\",\"content\":\"Still not sure.\"}"),
    ]);
    let mut session = Session::new("session", &root, &root);
    session.record_runtime_block(
            "The verified plan is rooted at myapp, but the tool call targets other/x.txt outside that project. No filesystem action was applied.",
        );

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "explain the latest runtime outcome",
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert!(joined_request_messages(&requests[1]).contains("runtime block/skip/failure"));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.route.as_deref() == Some("state")));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("verified plan is rooted at myapp")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stale_runtime_block_does_not_hijack_later_chat() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-stale-block-chat",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"chat\",\"content\":\"Hello!\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    session.record_runtime_block("Old block message.");
    session.start_reasoning_trace("intervening turn one");
    session.start_reasoning_trace("intervening turn two");

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.route.as_deref() == Some("chat")));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Hello!"
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == "Old block message."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_action_clears_prior_runtime_block() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-clear-block-on-action",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let action_provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "clear-block-create".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ok.txt",
                        "contents": "ok\n"
                    }),
                    assistant_summary: Some("create ok file".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_runtime_block("Previous block message.");

    run_permissive_agent_turn(&action_provider, &mut session, "create ok.txt");

    assert!(root.join("ok.txt").is_file());
    assert!(session.latest_runtime_block().is_none());

    let chat_provider =
        CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"chat\",\"content\":\"Hello after action.\"}",
        ));

    run_permissive_agent_turn(&chat_provider, &mut session, "hello");

    assert_eq!(chat_provider.requests().len(), 1);
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.route.as_deref() == Some("chat")));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Hello after action."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn repeated_identical_all_skipped_tool_results_stop_the_loop() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-repeated-skip-breaker",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    std::fs::write(
            root.join("Demo/plan.md"),
            "# Demo Plan\n\n```text\nREADME.md\n```\n\n## Verification\n- README.md exists.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
    let mut session = Session::new("session", &root, &root);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("Demo/plan.md"),
            contents: "# Demo Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "Demo/plan.md".to_string(),
        }),
    );
    let repeated_create = |id: &str| RawModelToolCall {
        id: id.to_string(),
        name: RawModelToolName::Known(ModelToolName::CreateFile),
        arguments: json!({
            "target_path": "notes.txt",
            "contents": "hello world\n"
        }),
        assistant_summary: Some("create notes".to_string()),
    };
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Trying the create.")
                .with_tool_calls(vec![repeated_create("repeat-skip-1")]),
            crate::event::ProviderOutput::new("Trying the create again.")
                .with_tool_calls(vec![repeated_create("repeat-skip-2")]),
            crate::event::ProviderOutput::new("Created notes.txt."),
        ]);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a file notes.txt with the text hello world",
    );

    assert!(!root.join("notes.txt").exists());
    assert!(provider.requests().len() <= REPEATED_IDENTICAL_SKIP_BREAKER_LIMIT + 1);
    assert_eq!(
        session
            .events()
            .iter()
            .filter(|event| matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content.contains("repeated the same blocked tool result")
            ))
            .count(),
        1
    );
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Created notes.txt")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn execute_prose_without_verified_action_is_not_reported_as_success() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-false-success-guard",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Demo")).unwrap();
    std::fs::write(
            root.join("Demo/plan.md"),
            "# Demo Plan\n\n```text\nREADME.md\n```\n\n## Verification\n- README.md exists.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
    let mut session = Session::new("session", &root, &root);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("Demo/plan.md"),
            contents: "# Demo Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "Demo/plan.md".to_string(),
        }),
    );
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Trying the create.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "false-success-skipped".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "hello world\n"
                    }),
                    assistant_summary: Some("create notes".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Created notes.txt with hello world."),
        ]);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a file notes.txt with the text hello world",
    );

    assert!(!root.join("notes.txt").exists());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Created notes.txt")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("No verified filesystem change occurred")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn executable_command_shape_skips_existing_file_tools_and_accepts_shell_command() {
    assert!(input_contains_executable_command_shape("cat notes.txt"));
    assert!(input_contains_executable_command_shape(
        "PYTHONPATH=src python -m text_tools.cli sample.txt"
    ));
    assert!(!input_contains_executable_command_shape(
        "Create USAGE.md containing PYTHONPATH=src python -m text_tools.cli sample.txt."
    ));

    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-intent-guard",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "hello world\n").unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Rewriting notes.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-intent-wrong-file".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "notes.txt",
                        "contents": "changed\n"
                    }),
                    assistant_summary: Some("rewrite notes".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Running cat.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-intent-cat".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "cat notes.txt",
                        "cwd": ".",
                        "expected_effect": "hello world",
                        "expected_file": "notes.txt"
                    }),
                    assistant_summary: Some("cat notes".to_string()),
                },
            ]),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "cat notes.txt");

    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "hello world\n"
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(&applied.result, VerifiedActionResult::Shell(shell)
                if shell.exit_code == Some(0)
                    && shell.stdout.contains("hello world"))
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Tool `create_file` is not available"))));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("ignored shell expected paths"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shell_execution_intent_exposes_only_shell_safe_tools_before_model_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-shell-scoped-tools",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("USAGE.md"), "usage\n").unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"shell_execution\"}",
        ))
        .with_tool_outputs(vec![
            crate::event::ProviderOutput::new("Creating usage.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-scoped-wrong-file".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "USAGE.md",
                        "contents": "changed\n"
                    }),
                    assistant_summary: Some("rewrite usage".to_string()),
                },
            ]),
            crate::event::ProviderOutput::new("Running cat.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "shell-scoped-cat".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "cat USAGE.md",
                        "cwd": ".",
                        "expected_effect": "usage",
                        "expected_file": "USAGE.md"
                    }),
                    assistant_summary: Some("cat usage".to_string()),
                },
            ]),
        ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "run cat USAGE.md");

    let tool_requests = provider
        .requests()
        .into_iter()
        .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
        .collect::<Vec<_>>();
    assert!(tool_requests.len() >= 2);
    for tool_request in &tool_requests {
        assert_eq!(
            tool_request.tool_names,
            vec!["ask_guidance".to_string(), "shell_command".to_string()]
        );
        assert_eq!(tool_request.tool_count, 2);
        assert!(!tool_request
            .tool_names
            .iter()
            .any(|name| name == "create_file" || name == "create_files"));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("USAGE.md")).unwrap(),
        "usage\n"
    );
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Tool `create_file` is not available"))));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("ignored shell expected paths"))));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ActionApplied(applied)
            if matches!(&applied.result, VerifiedActionResult::Shell(shell)
                if shell.stdout.contains("usage"))
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_execution_intent_exposes_plan_safe_tools_before_model_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-scoped-tools",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("PlanScoped");
    std::fs::create_dir_all(&project).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected files exist.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "plan-scoped-files".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFiles),
                    arguments: json!({
                        "directories": ["PlanScoped/src"],
                        "files": [
                            {
                                "target_path": "PlanScoped/README.md",
                                "contents": "# Plan Scoped\n"
                            },
                            {
                                "target_path": "PlanScoped/src/main.py",
                                "contents": "print('ok')\n"
                            }
                        ]
                    }),
                    assistant_summary: Some("create files".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: vec![project.join("src")],
        expected_files: vec![project.join("README.md"), project.join("src/main.py")],
    });

    run_permissive_agent_turn(&provider, &mut session, "execute the verified plan");

    let tool_request = only_tool_request(&provider);
    assert_eq!(
        tool_request.tool_names,
        vec![
            "ask_guidance".to_string(),
            "create_files".to_string(),
            "create_file".to_string(),
            "create_directory".to_string(),
            "overwrite_file".to_string(),
            "patch_file".to_string(),
            "shell_command".to_string(),
        ]
    );
    assert_eq!(tool_request.tool_count, 7);
    assert!(!tool_request
        .tool_names
        .iter()
        .any(|name| name == "delete_file" || name == "move_file"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_tool_command_still_exposes_full_tool_set() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-explicit-full-tools",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_tool_output(
        crate::event::ProviderOutput::new("Creating file.").with_tool_calls(vec![
            RawModelToolCall {
                id: "explicit-full-tools-file".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "notes.txt",
                    "contents": "hello\n"
                }),
                assistant_summary: Some("create file".to_string()),
            },
        ]),
    );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "/tool create notes.txt");

    let tool_request = only_tool_request(&provider);
    assert_eq!(tool_request.tool_count, 9);
    assert!(tool_request
        .tool_names
        .iter()
        .any(|name| name == "delete_file"));
    assert!(tool_request
        .tool_names
        .iter()
        .any(|name| name == "move_file"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_tool_command_continues_after_repeated_shell_feedback() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-explicit-repeat-continues",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("postcss.config.mjs"),
        "module.exports = { plugins: { tailwindcss: {}, autoprefixer: {} } }\n",
    )
    .unwrap();
    let provider = CapturingProvider::new().with_tool_outputs(vec![
        crate::event::ProviderOutput::new("Read config.").with_tool_calls(vec![RawModelToolCall {
            id: "explicit-repeat-cat-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({
                "command": "cat postcss.config.mjs",
                "cwd": root.display().to_string()
            }),
            assistant_summary: Some("read config".to_string()),
        }]),
        crate::event::ProviderOutput::new("Read config again.").with_tool_calls(vec![
            RawModelToolCall {
                id: "explicit-repeat-cat-2".to_string(),
                name: RawModelToolName::Known(ModelToolName::ShellCommand),
                arguments: json!({
                    "command": "cat postcss.config.mjs",
                    "cwd": root.display().to_string()
                }),
                assistant_summary: Some("read config again".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("List files.").with_tool_calls(vec![RawModelToolCall {
            id: "explicit-repeat-list".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({
                "command": "ls -a",
                "cwd": root.display().to_string()
            }),
            assistant_summary: Some("list files".to_string()),
        }]),
        crate::event::ProviderOutput::new("Fix config.").with_tool_calls(vec![RawModelToolCall {
            id: "explicit-repeat-overwrite".to_string(),
            name: RawModelToolName::Known(ModelToolName::OverwriteFile),
            arguments: json!({
                "target_path": "postcss.config.mjs",
                "contents": "export default { plugins: { tailwindcss: {}, autoprefixer: {} } }\n"
            }),
            assistant_summary: Some("fix config".to_string()),
        }]),
    ]);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "Fix postcss.config.mjs by replacing CommonJS module.exports with an ESM export default object.",
            PermissionPolicyMode::FullAccess,
        );

    assert_eq!(
        std::fs::read_to_string(root.join("postcss.config.mjs")).unwrap(),
        "export default { plugins: { tailwindcss: {}, autoprefixer: {} } }\n"
    );
    let applied_shell_actions = session
        .actions()
        .iter()
        .filter(|record| matches!(record.verified_result, Some(VerifiedActionResult::Shell(_))))
        .count();
    assert_eq!(applied_shell_actions, 1);
    assert!(session.actions().iter().any(|record| matches!(
        record.verified_result,
        Some(VerifiedActionResult::File(
            crate::event::FileActionVerification::FileOverwritten { .. }
        ))
    )));
    let requests = provider.requests();
    assert!(
        joined_request_messages(&requests[0]).contains("Explicit tool command"),
        "stripped TUI /tool turns should carry explicit tool guidance"
    );
    assert!(
        requests
            .iter()
            .any(|request| joined_request_messages(request)
                .contains("Use the earlier tool result already in context")),
        "provider should receive explicit repeated-shell recovery guidance"
    );
    assert!(
        requests
            .iter()
            .any(|request| joined_request_messages(request)
                .contains("Call overwrite_file or patch_file now")),
        "provider should receive explicit edit guidance after follow-up inspection"
    );
    assert!(
        requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .count()
            >= 3,
        "explicit /tool should continue after repeated shell feedback"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_tool_command_stops_repeated_read_only_stall() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-explicit-read-stall",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("postcss.config.mjs"), "module.exports = {}\n").unwrap();
    let mut outputs = vec![
        crate::event::ProviderOutput::new("Read config.").with_tool_calls(vec![RawModelToolCall {
            id: "explicit-stall-cat-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: json!({
                "command": "cat postcss.config.mjs",
                "cwd": root.display().to_string()
            }),
            assistant_summary: Some("read config".to_string()),
        }]),
    ];
    for index in 2..=8 {
        outputs.push(
            crate::event::ProviderOutput::new("Read again.").with_tool_calls(vec![
                RawModelToolCall {
                    id: format!("explicit-stall-cat-{index}"),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "cat postcss.config.mjs",
                        "cwd": root.display().to_string()
                    }),
                    assistant_summary: Some("read config again".to_string()),
                },
            ]),
        );
    }
    let provider = CapturingProvider::new().with_tool_outputs(outputs);
    let mut session = Session::new("session", &root, &root);

    run_agent_tool_turn_with_policy(
            &provider,
            &mut session,
            "Fix postcss.config.mjs by replacing CommonJS module.exports with an ESM export default object.",
            PermissionPolicyMode::FullAccess,
        );

    assert_eq!(
        std::fs::read_to_string(root.join("postcss.config.mjs")).unwrap(),
        "module.exports = {}\n"
    );
    assert!(
        provider.requests().len() < MAX_AGENT_TOOL_ROUNDS,
        "explicit read-only stall should stop before the tool-round cap"
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("kept returning read-only shell commands")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn execute_no_tool_text_response_retries_without_rendering_fake_claim() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-execute-no-tool-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new(
                    "Plan created for playground/FakePlan. Files added: README.md, src/main.py.",
                ),
                crate::event::ProviderOutput::new("Creating the plan file.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "no-tool-retry-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "playground/FakePlan/plan.md",
                            "contents": "# Fake Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan inside playground/FakePlan",
    );

    assert!(root.join("playground/FakePlan/plan.md").is_file());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Files added")
    )));
    let requests = provider.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .count(),
        2
    );
    assert!(joined_request_messages(
        requests
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .nth(1)
            .expect("second tool request should exist")
    )
    .contains("This route requires tool actions"));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("execute route returned no tool calls"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn execute_empty_tool_response_does_not_plain_fallback_to_fake_completion() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-execute-empty-no-fallback",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProviderWithToolErrors::new(
        vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ),
            crate::event::ProviderOutput::new(
                "Created project files and ran verification successfully.",
            ),
        ],
        vec![CapturedToolStep::EmptyResponse],
    );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project, execute it, and run verification",
    );

    assert!(!root.join("GreeterCLI").exists());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content.contains("Created project files")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("did not return any tool actions")
    )));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.runtime_checks.iter().any(
            |line| line.contains("empty tool response on execute route; requested tool repair")
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_empty_tool_response_retries_and_creates_plan() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-empty-tool-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProviderWithToolErrors::new(
            vec![crate::event::ProviderOutput::new("{\"route\":\"execute\"}")],
            vec![
                CapturedToolStep::EmptyResponse,
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating the plan file.").with_tool_calls(
                        vec![RawModelToolCall {
                            id: "empty-retry-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "playground/RetryPlan/plan.md",
                                "contents": "# Retry Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Check expected files.\n\n## Acceptance Criteria\n- Expected files exist.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        }],
                    ),
                ),
            ],
        );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan inside playground/RetryPlan",
    );

    assert!(root.join("playground/RetryPlan/plan.md").is_file());
    let requests = provider.requests();
    assert!(requests.len() >= 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    assert!(requests
        .iter()
        .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
        .nth(1)
        .is_some_and(
            |request| joined_request_messages(request).contains("This route requires tool actions")
        ));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.runtime_checks.iter().any(
            |line| line.contains("empty tool response on execute route; requested tool repair")
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bare_plan_artifact_is_anchored_to_batch_project_root() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-bare-plan-batch-root",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "bare-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "PLAN.md",
                            "contents": "# Greeter Plan\n\n## File Tree\n```text\nplayground/GreeterCLI/\n├── README.md\n├── requirements.txt\n├── src/\n│   └── main.py\n└── tests/\n    └── test_main.py\n```\n\n## Verification\n- Run `python -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter runs and tests pass.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "bare-plan-files".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFiles),
                        arguments: json!({
                            "directories": [
                                "tests",
                                "playground/GreeterCLI/src",
                                "playground/GreeterCLI/tests"
                            ],
                            "files": [
                                {
                                    "target_path": "playground/GreeterCLI/README.md",
                                    "contents": "# Greeter CLI\n"
                                },
                                {
                                    "target_path": "playground/GreeterCLI/requirements.txt",
                                    "contents": ""
                                },
                                {
                                    "target_path": "playground/GreeterCLI/src/main.py",
                                    "contents": "def main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n"
                                },
                                {
                                    "target_path": "playground/GreeterCLI/tests/test_main.py",
                                    "contents": "def test_smoke():\n    assert True\n"
                                }
                            ]
                        }),
                        assistant_summary: Some("create files".to_string()),
                    },
                ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project in playground and execute it",
    );

    assert!(!root.join("PLAN.md").exists());
    assert!(root.join("playground/GreeterCLI/PLAN.md").is_file());
    for path in [
        "playground/GreeterCLI/README.md",
        "playground/GreeterCLI/requirements.txt",
        "playground/GreeterCLI/src/main.py",
        "playground/GreeterCLI/tests/test_main.py",
    ] {
        assert!(root.join(path).is_file(), "missing {path}");
    }
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("anchored plan should be recorded");
    assert_eq!(plan.project_root, root.join("playground/GreeterCLI"));
    assert_eq!(
        plan.runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_execution_can_run_shell_verification_after_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-shell-verify",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let project = root.join("GreeterCLI");
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan, files, and verifying.")
                    .with_tool_calls(vec![
                    RawModelToolCall {
                        id: "verify-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/plan.md",
                            "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter files exist and compile.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                        RawModelToolCall {
                            id: "verify-files".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFiles),
                            arguments: json!({
                                "directories": ["GreeterCLI/src", "GreeterCLI/tests"],
                                "files": [
                                    {
                                        "target_path": "GreeterCLI/README.md",
                                        "contents": "# Greeter CLI\n"
                                    },
                                    {
                                        "target_path": "GreeterCLI/requirements.txt",
                                        "contents": ""
                                    },
                                    {
                                        "target_path": "GreeterCLI/src/main.py",
                                        "contents": "def greeting(name='World'):\n    return f'Hello, {name}!'\n\nif __name__ == '__main__':\n    print(greeting())\n"
                                    },
                                    {
                                        "target_path": "GreeterCLI/tests/test_main.py",
                                        "contents": "import sys\nfrom pathlib import Path\nsys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))\nfrom main import greeting\n\ndef test_greeting():\n    assert greeting('Alice') == 'Hello, Alice!'\n"
                                    }
                                ]
                            }),
                            assistant_summary: Some("create files".to_string()),
                        },
                        RawModelToolCall {
                            id: "verify-shell".to_string(),
                            name: RawModelToolName::Known(ModelToolName::ShellCommand),
                            arguments: json!({
                                "command": "python3 -m py_compile src/main.py tests/test_main.py",
                                "cwd": project.display().to_string(),
                                "expected_effect": "Python files compile"
                            }),
                            assistant_summary: Some("compile Python files".to_string()),
                        },
                    ]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project, execute it, and run verification",
    );

    assert!(project.join("plan.md").is_file());
    assert!(project.join("src/main.py").is_file());
    assert!(project.join("tests/test_main.py").is_file());
    assert!(session.actions().iter().any(|record| {
        matches!(
            record.verified_result.as_ref(),
            Some(VerifiedActionResult::Shell(shell)) if shell.exit_code == Some(0)
        )
    }));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| !trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Skipped shell command during verified plan execution"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_execution_stops_after_no_progress_off_plan_tool_round() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-no-progress",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_outputs(vec![
                crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "no-progress-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/plan.md",
                            "contents": "# Greeter Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Run `python3 -m py_compile src/main.py tests/test_main.py`.\n\n## Acceptance Criteria\n- The greeter files exist and compile.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
                crate::event::ProviderOutput::new("Creating part of the project.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "no-progress-readme".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "GreeterCLI/README.md",
                                "contents": "# Greeter CLI\n"
                            }),
                            assistant_summary: Some("create README".to_string()),
                        },
                    ]),
                crate::event::ProviderOutput::new("Trying wrong paths and verification.")
                    .with_tool_calls(vec![
                        RawModelToolCall {
                            id: "no-progress-wrong-test".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "Greeter/wrong/test_main.py",
                                "contents": "def test_smoke():\n    assert True\n"
                            }),
                            assistant_summary: Some("create wrong test".to_string()),
                        },
                        RawModelToolCall {
                            id: "no-progress-wrong-requirements".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "Greeter/wrong/requirements.txt",
                                "contents": ""
                            }),
                            assistant_summary: Some("create wrong requirements".to_string()),
                        },
                        RawModelToolCall {
                            id: "no-progress-shell".to_string(),
                            name: RawModelToolName::Known(ModelToolName::ShellCommand),
                            arguments: json!({
                                "command": "python3 -m py_compile src/main.py tests/test_main.py",
                                "cwd": root.join("GreeterCLI").display().to_string(),
                                "expected_effect": "Python files compile"
                            }),
                            assistant_summary: Some("compile Python files".to_string()),
                        },
                    ]),
                crate::event::ProviderOutput::new("This request should not be reached.")
                    .with_tool_calls(vec![RawModelToolCall {
                        id: "no-progress-late-test".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "GreeterCLI/tests/test_main.py",
                            "contents": "def test_late():\n    assert True\n"
                        }),
                        assistant_summary: Some("late test".to_string()),
                    }]),
            ]);
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a greeter project, execute it, and run verification",
    );

    assert!(root.join("GreeterCLI/plan.md").is_file());
    assert!(root.join("GreeterCLI/README.md").is_file());
    assert!(!root.join("GreeterCLI/src/main.py").exists());
    assert!(!root.join("GreeterCLI/tests/test_main.py").exists());
    assert!(!root.join("GreeterCLI/requirements.txt").exists());
    assert!(!root.join("Greeter/wrong/test_main.py").exists());
    assert_eq!(
        provider
            .requests()
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .count(),
        3,
        "the no-progress skipped/off-plan round should stop the loop"
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("Stopped because the last tool response")
                && message.content.contains("GreeterCLI/tests/test_main.py")
                && message.content.contains("GreeterCLI/requirements.txt")
    )));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("plan execution made no progress; stopped provider loop"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_execution_stops_after_partial_create_files_batch() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-partial-batch",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Journal")).unwrap();
    std::fs::write(
            root.join("Journal/plan.md"),
            "# Journal Plan\n\n```text\nREADME.md\nrequirements.txt\nsrc/main.py\ntests/test_main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project scaffold is complete.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new().with_tool_outputs(vec![
        crate::event::ProviderOutput::new("Creating most files.").with_tool_calls(vec![
            RawModelToolCall {
                id: "partial-batch".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFiles),
                arguments: json!({
                    "directories": ["Journal/src", "Journal/tests"],
                    "files": [
                        {
                            "target_path": "Journal/README.md",
                            "contents": "# Journal\n"
                        },
                        {
                            "target_path": "Journal/requirements.txt",
                            "contents": ""
                        },
                        {
                            "target_path": "Journal/src/main.py",
                            "contents": "print('journal')\n"
                        }
                    ]
                }),
                assistant_summary: Some("create most expected files".to_string()),
            },
        ]),
        crate::event::ProviderOutput::new("This repair request should not be reached.")
            .with_tool_calls(vec![RawModelToolCall {
                id: "partial-late-test".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: json!({
                    "target_path": "Journal/tests/test_main.py",
                    "contents": "def test_late():\n    assert True\n"
                }),
                assistant_summary: Some("late test".to_string()),
            }]),
    ]);
    let mut session = Session::new("session", &root, &root);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(CreateFileAction {
            target_path: PathBuf::from("Journal/plan.md"),
            contents: "# Journal Plan\n".to_string(),
        }),
        "create plan",
    )
    .approve()
    .mark_applied();
    record_verified_project_memory(
        &mut session,
        &plan_action,
        &VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
            path: "Journal/plan.md".to_string(),
        }),
    );

    run_agent_tool_turn_with_policy(
        &provider,
        &mut session,
        "execute the verified plan",
        PermissionPolicyMode::FullAccess,
    );

    assert!(root.join("Journal/README.md").is_file());
    assert!(root.join("Journal/requirements.txt").is_file());
    assert!(root.join("Journal/src/main.py").is_file());
    assert!(!root.join("Journal/tests/test_main.py").exists());
    assert_eq!(
        provider
            .requests()
            .iter()
            .filter(|request| request.mode == CapturedProviderRequestMode::Tool)
            .count(),
        1,
        "partial create_files batch should not trigger an open-ended repair request"
    );
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("No further model repair request was sent")
                && message.content.contains("Journal/tests/test_main.py")
    )));
    assert!(session
        .latest_runtime_block()
        .is_some_and(|block| block.message.contains("Journal/tests/test_main.py")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_execution_empty_tool_response_continues_without_plain_fallback() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-exec-empty-tool-no-fallback",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProviderWithToolErrors::new(
            vec![crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            )],
            vec![
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating plan first.").with_tool_calls(
                        vec![RawModelToolCall {
                            id: "empty-response-plan".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: json!({
                                "target_path": "NotesCLI/PROJECT_PLAN.md",
                                "contents": "# Notes Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- `README.md` and `src/main.py` exist.\n\n## Acceptance Criteria\n- Running `python -m src.main` prints a notes CLI help message.\n"
                            }),
                            assistant_summary: Some("create plan".to_string()),
                        }],
                    ),
                ),
                CapturedToolStep::EmptyResponse,
                CapturedToolStep::Output(
                    crate::event::ProviderOutput::new("Creating all missing files.")
                        .with_tool_calls(vec![
                            RawModelToolCall {
                                id: "empty-response-readme".to_string(),
                                name: RawModelToolName::Known(ModelToolName::CreateFile),
                                arguments: json!({
                                    "target_path": "NotesCLI/README.md",
                                    "contents": "# Notes CLI\n"
                                }),
                                assistant_summary: Some("create README".to_string()),
                            },
                            RawModelToolCall {
                                id: "empty-response-main".to_string(),
                                name: RawModelToolName::Known(ModelToolName::CreateFile),
                                arguments: json!({
                                    "target_path": "NotesCLI/src/main.py",
                                    "contents": "def main():\n    print('notes')\n\nif __name__ == '__main__':\n    main()\n"
                                }),
                                assistant_summary: Some("create main".to_string()),
                            },
                        ]),
                ),
            ],
        );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a notes plan and execute it",
    );

    assert!(root.join("NotesCLI/PROJECT_PLAN.md").is_file());
    assert!(root.join("NotesCLI/README.md").is_file());
    assert!(root.join("NotesCLI/src/main.py").is_file());
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "plain route plus three tool attempts; no plain fallback request"
    );
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert!(requests[1..]
        .iter()
        .all(|request| request.mode == CapturedProviderRequestMode::Tool));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(
            |trace| trace.runtime_checks.iter().any(|line| line.contains(
                "empty tool response during plan execution; continued from verified missing paths"
            ))
        ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_with_skipped_files_waits_for_followup_without_post_decision() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-skipped-files-no-post-decision",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "post-plan-decision-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/plan.md",
                            "contents": "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                    RawModelToolCall {
                        id: "post-plan-decision-readme-too-early".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "CalculatorUI/README.md",
                            "contents": "# Calculator UI\n"
                        }),
                        assistant_summary: Some("create README too early".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a plan for the calculator UI and execute it",
    );

    assert!(root.join("CalculatorUI/plan.md").is_file());
    assert!(!root.join("CalculatorUI/README.md").exists());
    assert!(!root.join("CalculatorUI/calculator.py").exists());
    assert!(!root.join("CalculatorUI/ui.py").exists());
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.runtime_checks.iter().any(
            |line| line.contains("plan creation completed; skipped final provider synthesis")
        )));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_only_route_post_plan_decision_can_keep_plan_only_boundary() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-only-no-post-decision",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_outputs(vec![
                crate::event::ProviderOutput::new("{\"route\":\"execute\"}"),
                crate::event::ProviderOutput::new("{\"route\":\"state\",\"answer_kind\":\"plan\"}"),
            ])
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan only.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-only-route-plan".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "TodoPlan/PLAN.md",
                            "contents": "# Todo Plan\n\n```text\nREADME.md\nsrc/main.py\nrequirements.txt\n```\n\n## Verification\n- Expected files exist after execution.\n\n## Acceptance Criteria\n- The project matches the listed file tree.\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create only a project plan inside TodoPlan",
    );

    assert!(root.join("TodoPlan/PLAN.md").is_file());
    assert!(!root.join("TodoPlan/README.md").exists());
    assert!(!root.join("TodoPlan/src/main.py").exists());
    assert!(!root.join("TodoPlan/requirements.txt").exists());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Plain);
    assert!(joined_request_messages(&requests[2]).contains("A verified plan was just created"));
    assert!(session
        .latest_reasoning_trace()
        .is_some_and(|trace| trace.runtime_checks.iter().any(
            |line| line.contains("plan creation completed; skipped final provider synthesis")
        )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn followup_route_can_bind_latest_verified_plan_with_model_requested_context() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-followup-context",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("CalculatorUI");
    std::fs::create_dir_all(&project).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Calculator Plan\n\n```text\nREADME.md\ncalculator.py\nui.py\n```\n\n## Verification\n- `calculator.py`, `ui.py`, and `README.md` exist.\n\n## Acceptance Criteria\n- Running `python ui.py` launches the calculator UI.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new(
                "{\"route\":\"ask_guidance\",\"question\":\"Which plan should I execute?\"}",
            ),
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "followup-readme".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "README.md",
                        "contents": "# Calculator UI\n"
                    }),
                    assistant_summary: Some("create README".to_string()),
                },
                RawModelToolCall {
                    id: "followup-calc".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "calculator.py",
                        "contents": "class Calculator:\n    pass\n"
                    }),
                    assistant_summary: Some("create calculator".to_string()),
                },
                RawModelToolCall {
                    id: "followup-ui".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "ui.py",
                        "contents": "from calculator import Calculator\n"
                    }),
                    assistant_summary: Some("create ui".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: Vec::new(),
        expected_files: vec![
            project.join("README.md"),
            project.join("calculator.py"),
            project.join("ui.py"),
        ],
    });

    run_permissive_agent_turn(&provider, &mut session, "the plan you just created");

    assert!(project.join("README.md").is_file());
    assert!(project.join("calculator.py").is_file());
    assert!(project.join("ui.py").is_file());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert!(!joined_request_messages(&requests[0]).contains("latest verified plan"));
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    let context_retry_request = joined_request_messages(&requests[1]);
    assert!(context_retry_request.contains("latest verified plan: CalculatorUI/plan.md"));
    assert!(context_retry_request.contains("create all missing expected paths"));
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    let first_tool_request = joined_request_messages(&requests[2]);
    assert!(first_tool_request.contains("Verified plan execution contract"));
    for path in [
        "CalculatorUI/README.md",
        "CalculatorUI/calculator.py",
        "CalculatorUI/ui.py",
    ] {
        assert!(
            first_tool_request.contains(path),
            "first tool request did not include missing path {path}"
        );
    }
    let plan = session
        .project_memory()
        .latest_structured_plan()
        .expect("plan should remain recorded");
    assert_eq!(
        plan.runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line
            .contains("seeded verified plan execution contract before first tool request")));
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("skipped final provider synthesis")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn generic_state_status_with_incomplete_plan_retries_and_can_execute() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-plan-retry",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("RetryPlan");
    std::fs::create_dir_all(&project).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Retry Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_outputs(vec![
            crate::event::ProviderOutput::new("{\"route\":\"state\",\"answer_kind\":\"status\"}"),
            crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
            ),
        ])
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "state-retry-files".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFiles),
                    arguments: json!({
                        "directories": ["RetryPlan/src"],
                        "files": [
                            {
                                "target_path": "RetryPlan/README.md",
                                "contents": "# Retry Plan\n"
                            },
                            {
                                "target_path": "RetryPlan/src/main.py",
                                "contents": "print('retry')\n"
                            }
                        ]
                    }),
                    assistant_summary: Some("create files".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: vec![project.join("src")],
        expected_files: vec![project.join("README.md"), project.join("src/main.py")],
    });

    run_permissive_agent_turn(&provider, &mut session, "execute the plan!");

    assert!(project.join("README.md").is_file());
    assert!(project.join("src/main.py").is_file());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    assert!(joined_request_messages(&requests[1]).contains("incomplete verified plan is available"));
    assert_eq!(requests[2].mode, CapturedProviderRequestMode::Tool);
    assert!(session.latest_reasoning_trace().is_some_and(|trace| trace
        .model_decisions
        .iter()
        .any(|line| line
            .contains("state route selected generic status with an incomplete verified plan"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn existing_verified_plan_execution_is_not_blocked_by_plan_creation_intent() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-existing-plan-creation-intent",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("TodoPlan");
    std::fs::create_dir_all(&project).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Todo Plan\n\n```text\nTodoPlan/\n├── README.md\n├── src/main.py\n└── requirements.txt\n```\n\n## Verification\n- Expected files exist.\n\n## Acceptance Criteria\n- The project matches the plan.\n",
        )
        .unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating missing files.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "existing-plan-readme".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "README.md",
                            "contents": "# Todo Plan\n"
                        }),
                        assistant_summary: Some("create README".to_string()),
                    },
                    RawModelToolCall {
                        id: "existing-plan-main".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "src/main.py",
                            "contents": "def main():\n    print('todo')\n\nif __name__ == '__main__':\n    main()\n"
                        }),
                        assistant_summary: Some("create main".to_string()),
                    },
                    RawModelToolCall {
                        id: "existing-plan-reqs".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "requirements.txt",
                            "contents": ""
                        }),
                        assistant_summary: Some("create requirements".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        path: plan_path.clone(),
        project_root: project.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project.clone(),
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: Vec::new(),
        expected_files: vec![
            project.join("README.md"),
            project.join("src/main.py"),
            project.join("requirements.txt"),
        ],
    });

    run_permissive_agent_turn(&provider, &mut session, "please execute the plan");

    assert!(project.join("README.md").is_file());
    assert!(project.join("src/main.py").is_file());
    assert!(project.join("requirements.txt").is_file());
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(!trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("Create the project plan file first")));
    assert_eq!(
        session
            .project_memory()
            .latest_structured_plan()
            .expect("plan should remain recorded")
            .runtime_status(),
        crate::session::StructuredProjectPlanStatus::Completed
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn normal_text_model_plain_answer_renders_without_tools() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-normal-chat-decision",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"chat\",\"content\":\"Hello there.\"}",
    ));
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert!(requests[0].messages[0]
        .content
        .contains("{\"route\":\"execute\"}"));
    assert!(requests[0].messages[0]
        .content
        .contains("local file/artifact/plan work"));
    assert!(requests[0].messages[0]
        .content
        .contains("Return compact JSON"));
    assert!(requests[0].messages[0].content.len() <= 700);
    assert!(!requests[0].messages[0].content.contains("Use `/tool"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Hello there."
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message) if message.content.contains("\"route\"")
    )));
    assert!(session.actions().is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn text_only_code_block_prompt_falls_back_to_visible_chat_without_tools() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-text-code-block-chat",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let input = "Write only one fenced TOML code block with [features], js_repl = false, count = 42, url = \"https://mcp.linear.app/mcp\", and a # disabled comment. No prose.";
    let output = "```toml\n[features]\njs_repl = false\ncount = 42\nurl = \"https://mcp.linear.app/mcp\"\n# disabled\n```";
    let provider =
        CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(output));
    let mut session = Session::new("session", &root, &root);

    assert_eq!(local_path_like_token_count(input), 0);
    assert!(!input_contains_local_work_syntax(input));
    run_permissive_agent_turn(&provider, &mut session, input);

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == output
    )));
    assert!(!session
        .events()
        .iter()
        .any(|event| matches!(event, Event::Error(_))));
    assert!(session.actions().is_empty());
    assert!(session.latest_reasoning_trace().is_some_and(|trace| {
        trace.route.as_deref() == Some("chat")
            && trace
                .model_decisions
                .iter()
                .any(|line| line.contains("unstructured text for text-only input"))
    }));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn wrapped_chat_route_does_not_trigger_tool_protocol_fallback() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-wrapped-chat-route",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "<|channel|>final<|message|>{\"route\":\"chat\",\"content\":\"Hello.\"}",
    ));
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Hello."
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_hello_after_verified_folder_stays_one_plain_request() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-hello-no-folder-memory",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("remembered")).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"chat\",\"content\":\"Hello.\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    let folder_action = Action::proposed(
        "action-folder",
        ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
            target_path: PathBuf::from("remembered"),
        }),
        "create remembered folder",
    )
    .approve()
    .mark_applied();
    let result =
        VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
            path: "remembered".to_string(),
        });
    let mut record = ActionRecord::new(folder_action.clone());
    record.verified_result = Some(result.clone());
    session.push_action(record);
    record_verified_project_memory(&mut session, &folder_action, &result);

    run_permissive_agent_turn(&provider, &mut session, "hello");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(
        requests[0].messages.last(),
        Some(&ChatMessage::user("hello"))
    );
    let joined = joined_request_messages(&requests[0]);
    assert!(!joined.contains("latest verified folder"));
    assert!(!joined.contains("remembered"));
    assert!(!joined.contains("Verified filesystem context"));
    assert!(session.latest_provider_prompt_memory_selection().is_none());
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "Hello."
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_state_question_uses_model_selected_state_route() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-question-model",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("remembered")).unwrap();
    std::fs::create_dir_all(root.join("latest-folder")).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    for (action_id, target_path) in [
        ("action-folder-1", "remembered"),
        ("action-folder-2", "latest-folder"),
    ] {
        let folder_action = Action::proposed(
            action_id,
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from(target_path),
            }),
            "create folder",
        )
        .approve()
        .mark_applied();
        let result =
            VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: target_path.to_string(),
            });
        let mut record = ActionRecord::new(folder_action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &folder_action, &result);
    }

    run_permissive_agent_turn(&provider, &mut session, "what did you create?");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(
        requests[0].messages.last(),
        Some(&ChatMessage::user("what did you create?"))
    );
    let joined = joined_request_messages(&requests[0]);
    assert!(joined.contains("{\"route\":\"state\""));
    assert!(!joined.contains("latest verified folder"));
    assert!(!joined.contains("remembered"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content == "latest-folder"
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn model_selected_state_route_can_report_recent_changes() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-recent-changes",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"recent_changes\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    session.start_reasoning_trace("create a file");
    let action = Action::proposed_create_file("action-file", "latest.txt", "hi\n", "create")
        .approve()
        .mark_applied();
    let result = VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
        path: "latest.txt".to_string(),
    });
    let mut record = ActionRecord::new(action.clone());
    record.verified_result = Some(result.clone());
    session.push_action(record);
    record_verified_project_memory(&mut session, &action, &result);

    run_permissive_agent_turn(&provider, &mut session, "what changed in the last action?");

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("created latest.txt")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn state_route_without_kind_uses_secondary_classifier_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-kind-classifier",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // Plain route picks `state` with no kind; the secondary classifier then
    // resolves the precise view.
    let provider = CapturingProvider::new().with_plain_outputs(vec![
        crate::event::ProviderOutput::new("{\"route\":\"state\"}"),
        crate::event::ProviderOutput::new("{\"answer_kind\":\"recent_changes\"}"),
    ]);
    let mut session = Session::new("session", &root, &root);

    // One prior action-producing turn so recent_changes has content.
    session.start_reasoning_trace("create the config");
    let action = Action::proposed_create_file("a1", "next.config.js", "", "create")
        .approve()
        .mark_applied();
    let mut record = ActionRecord::new(action);
    record.verified_result = Some(VerifiedActionResult::File(
        crate::event::FileActionVerification::FileCreated {
            path: "next.config.js".to_string(),
        },
    ));
    session.push_action(record);

    run_permissive_agent_turn(&provider, &mut session, "what did you just do?");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Plain);
    // The answer-kind menu is only sent on the secondary classifier call,
    // never on the always-sent route prompt.
    assert!(!joined_request_messages(&requests[0]).contains("Valid answer kinds"));
    assert!(joined_request_messages(&requests[1]).contains("Valid answer kinds"));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("plain_state_classifier")
    )));
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content.contains("next.config.js")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_creation_request_after_verified_folder_is_not_state_answer() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plan-request-after-folder",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("planned")).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new("{\"route\":\"execute\"}"))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating plan.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "plan-after-folder-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateFile),
                        arguments: json!({
                            "target_path": "planned/plan.md",
                            "contents": "# Project Plan\n\n```text\nsrc/main.py\nrequirements.txt\n```\n"
                        }),
                        assistant_summary: Some("create plan".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);
    let folder_action = Action::proposed(
        "action-folder",
        ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
            target_path: PathBuf::from("planned"),
        }),
        "create planned folder",
    )
    .approve()
    .mark_applied();
    let result =
        VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
            path: "planned".to_string(),
        });
    let mut record = ActionRecord::new(folder_action.clone());
    record.verified_result = Some(result.clone());
    session.push_action(record);
    record_verified_project_memory(&mut session, &folder_action, &result);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a markdown project plan inside planned for a tiny Python CLI app",
    );

    assert!(root.join("planned/plan.md").is_file());
    assert!(session.project_memory().latest_structured_plan().is_some());
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message) if message.content == "No verified plan recorded."
    )));
    let requests = provider.requests();
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert!(requests[1].tool_count > 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn verified_state_answer_keeps_latest_folder_ahead_of_created_files() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-state-latest-folder",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("session", &root, &root);

    for (action_id, request, result) in [
        (
            "action-file",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("demo/requirements.txt"),
                contents: String::new(),
            }),
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "demo/requirements.txt".to_string(),
            }),
        ),
        (
            "action-folder",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("unrelated"),
            }),
            VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "unrelated".to_string(),
            }),
        ),
    ] {
        let action = Action::proposed(action_id, request, "apply")
            .approve()
            .mark_applied();
        let mut record = ActionRecord::new(action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &action, &result);
    }

    let answer = verified_session_state_answer(&session, VerifiedStateAnswerKind::Summary);

    assert!(answer.contains("latest folder: unrelated"));
    assert!(answer.contains("latest file: demo/requirements.txt"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_created_summary_uses_verified_action_records() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-created-summary",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"state\",\"answer_kind\":\"created_summary\"}",
    ));
    let mut session = Session::new("session", &root, &root);

    for (action_id, request, result) in [
        (
            "action-folder",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("demo"),
            }),
            VerifiedActionResult::File(crate::event::FileActionVerification::DirectoryCreated {
                path: "demo".to_string(),
            }),
        ),
        (
            "action-file",
            ActionRequest::CreateFile(crate::action::CreateFileAction {
                target_path: PathBuf::from("demo/requirements.txt"),
                contents: String::new(),
            }),
            VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
                path: "demo/requirements.txt".to_string(),
            }),
        ),
    ] {
        let action = Action::proposed(action_id, request, "apply")
            .approve()
            .mark_applied();
        let mut record = ActionRecord::new(action.clone());
        record.verified_result = Some(result.clone());
        session.push_action(record);
        record_verified_project_memory(&mut session, &action, &result);
    }

    run_permissive_agent_turn(&provider, &mut session, "what did you create?");

    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::VerifiedState
                && message.content
                    == "current session:\n- directory demo\n- file demo/requirements.txt"
    )));
    assert_eq!(provider.requests().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn completed_plan_execution_intent_skips_tool_loop() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-completed-plan-execution-short-circuit",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("DonePlan");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Done Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected paths are present.\n",
        )
        .unwrap();
    std::fs::write(project.join("README.md"), "# Done\n").unwrap();
    std::fs::write(project.join("src/main.py"), "def main():\n    pass\n").unwrap();
    let provider = CapturingProvider::new().with_plain_output(crate::event::ProviderOutput::new(
        "{\"route\":\"execute\",\"intent\":\"plan_execution\"}",
    ));
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        project_root: project.clone(),
        path: plan_path.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: vec![root.join("DonePlan/src")],
        expected_files: vec![
            root.join("DonePlan/README.md"),
            root.join("DonePlan/src/main.py"),
        ],
    });

    run_permissive_agent_turn(&provider, &mut session, "execute the latest plan");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Controller
                && message.content.contains("already complete")
    )));
    let trace = session
        .latest_reasoning_trace()
        .expect("reasoning trace should exist");
    assert!(trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("already complete; skipped tool loop")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn completed_plan_execution_intent_does_not_skip_local_shell_work() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-completed-plan-shell-work",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("DonePlanShell");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let plan_path = project.join("plan.md");
    std::fs::write(
            &plan_path,
            "# Done Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification\n- Files exist.\n\n## Acceptance Criteria\n- Expected paths are present.\n",
        )
        .unwrap();
    std::fs::write(project.join("README.md"), "# Done\n").unwrap();
    std::fs::write(project.join("src/main.py"), "def main():\n    pass\n").unwrap();
    let expected_file = project.join("shell.out");
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Running requested command.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "completed-plan-shell".to_string(),
                    name: RawModelToolName::Known(ModelToolName::ShellCommand),
                    arguments: json!({
                        "command": "printf ok > shell.out",
                        "cwd": project.display().to_string(),
                        "expected_file": expected_file.display().to_string()
                    }),
                    assistant_summary: Some("run shell command".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    session.record_verified_plan_reference(VerifiedPlanReference {
        project_root: project.clone(),
        path: plan_path.clone(),
        source_action_id: "action-plan".to_string(),
    });
    session.record_structured_project_plan(crate::session::StructuredProjectPlan {
        source_action_id: Some("action-plan".to_string()),
        source_plan_path: plan_path,
        project_root: project,
        stage: "verified-plan".to_string(),
        status: crate::session::StructuredProjectPlanStatus::Verified,
        expected_directories: vec![root.join("DonePlanShell/src")],
        expected_files: vec![
            root.join("DonePlanShell/README.md"),
            root.join("DonePlanShell/src/main.py"),
        ],
    });

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "run PYTHONPATH=src python -m done.cli sample.txt inside that project",
    );

    assert_eq!(std::fs::read_to_string(expected_file).unwrap(), "ok");
    let requests = provider.requests();
    assert!(requests
        .iter()
        .any(|request| request.mode == CapturedProviderRequestMode::Plain));
    assert!(requests.iter().any(|request| {
        request.mode == CapturedProviderRequestMode::Tool
            && request
                .tool_names
                .iter()
                .any(|name| name == "shell_command")
    }));
    assert!(session.latest_reasoning_trace().is_some_and(|trace| !trace
        .runtime_checks
        .iter()
        .any(|line| line.contains("already complete; skipped tool loop"))));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_raw_tool_protocol_decision_enters_tool_loop_without_surfacing_protocol() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-plain-raw-tool-protocol",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new()
            .with_plain_output(crate::event::ProviderOutput::new(
                "<|channel|>commentary to=filesystem.create code<|message|>{\"path\":\"testharness\",\"contents\":\"\"}\nCreated folder testharness.",
            ))
            .with_tool_output(
                crate::event::ProviderOutput::new("Creating it.").with_tool_calls(vec![
                    RawModelToolCall {
                        id: "raw-protocol-retry-call-1".to_string(),
                        name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                        arguments: json!({ "target_path": "testharness" }),
                        assistant_summary: Some("create testharness folder".to_string()),
                    },
                ]),
            );
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "create a folder and name it testharness",
    );

    assert!(root.join("testharness").is_dir());
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Plain);
    assert_eq!(requests[0].tool_count, 0);
    assert_eq!(requests[1].mode, CapturedProviderRequestMode::Tool);
    assert!(requests[1].tool_count > 0);
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.content.contains("<|channel|>")
                || message.content.contains("Created folder testharness")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_ok_after_verified_plan_stays_plain_without_file_creation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-ok-no-plan-execution",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    let plan_path = root.join("app/project-plan.md");
    std::fs::write(
        &plan_path,
        "# Project Plan\n\n- Create package.json.\n- Create src/main.ts.\n",
    )
    .unwrap();
    let provider = CapturingProvider::new()
        .with_plain_output(crate::event::ProviderOutput::new(
            "{\"route\":\"chat\",\"content\":\"Ok.\"}",
        ))
        .with_tool_output(
            crate::event::ProviderOutput::new("Creating project files.").with_tool_calls(vec![
                RawModelToolCall {
                    id: "bad-ok-call-1".to_string(),
                    name: RawModelToolName::Known(ModelToolName::CreateFile),
                    arguments: json!({
                        "target_path": "package.json",
                        "contents": "{}\n"
                    }),
                    assistant_summary: Some("create package".to_string()),
                },
            ]),
        );
    let mut session = Session::new("session", &root, &root);
    let plan_action = Action::proposed(
        "action-plan",
        ActionRequest::CreateFile(crate::action::CreateFileAction {
            target_path: PathBuf::from("app/project-plan.md"),
            contents: String::new(),
        }),
        "create project plan",
    )
    .approve()
    .mark_applied();
    let result = VerifiedActionResult::File(crate::event::FileActionVerification::FileCreated {
        path: "app/project-plan.md".to_string(),
    });
    let mut plan_record = ActionRecord::new(plan_action.clone());
    plan_record.verified_result = Some(result.clone());
    session.push_action(plan_record);
    record_verified_project_memory(&mut session, &plan_action, &result);

    run_permissive_agent_turn(&provider, &mut session, "ok");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    for request in &requests {
        assert_eq!(request.mode, CapturedProviderRequestMode::Plain);
        assert_eq!(request.tool_count, 0);
    }
    assert!(!root.join("app/package.json").exists());
    assert!(!root.join("package.json").exists());
    assert_eq!(session.actions().len(), 1);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("plain_chat")
                && started.model.as_deref() == Some("test-model")
                && started.tool_count == Some(0)
    )));
    assert!(!session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_action_request_uses_tool_enabled_provider_request() {
    let root =
        std::env::temp_dir().join(format!("elgar-agent-loop-{}-tool-chat", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new();
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(&provider, &mut session, "/tool create a folder called Demo");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode, CapturedProviderRequestMode::Tool);
    assert!(requests[0].tool_count > 0);
    assert!(session.events().iter().any(|event| matches!(
        event,
        Event::ProviderStarted(started)
            if started.request_mode.as_deref() == Some("tool_enabled")
                && started.tool_count.is_some_and(|count| count > 0)
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn permissive_agent_prompt_requests_complete_scaffold_without_stack_specific_template() {
    let root = std::env::temp_dir().join(format!(
        "elgar-agent-loop-{}-next-tailwind-prompt",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let provider = CapturingProvider::new();
    let mut session = Session::new("session", &root, &root);

    run_permissive_agent_turn(
        &provider,
        &mut session,
        "/tool create a TS Next.js and Tailwind project in ~/next-tailwind-ts-project",
    );

    let requests = provider.requests();
    let tool_request = requests
        .iter()
        .find(|request| request.mode == CapturedProviderRequestMode::Tool)
        .expect("project creation should use tool path");
    let system_prompt = &tool_request.messages[0].content;
    assert!(system_prompt.contains("infer the necessary starter files"));
    assert!(system_prompt.contains("complete runnable scaffold"));
    assert!(system_prompt.contains("do not make completed files immutable"));
    assert!(!system_prompt.contains("next-env.d.ts"));
    assert!(!system_prompt.contains("tailwind.config"));

    let _ = std::fs::remove_dir_all(&root);
}
