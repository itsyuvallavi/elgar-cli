mod conversation;
mod event_rendering;
mod provider_thinking;
mod status;
mod tool_activity;
mod verification_rendering;

#[cfg(test)]
use provider_thinking::is_low_value_provider_tool_planning_thinking;

pub(crate) use conversation::ConversationLineStyle;
pub use conversation::ConversationPane;
pub use status::{CopyArea, InputArea, StatusLine};

use verification_rendering::user_display_path;

#[cfg(test)]
mod tests {
    use elgar_core::{
        event::{
            ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
            ErrorEvent, Event, FileActionVerification, ProviderFinished, ProviderOutput,
            ProviderStarted, ProviderTokenUsage, ShellActionVerification, UserMessage,
            VerifiedActionResult,
        },
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    };

    use super::{
        is_low_value_provider_tool_planning_thinking, ConversationLineStyle, ConversationPane,
        CopyArea, InputArea, StatusLine,
    };

    #[test]
    fn conversation_displays_user_assistant_provider_action_and_error_output() {
        let mut conversation = ConversationPane::default();
        let events = vec![
            Event::UserMessage(UserMessage::new("hello")),
            Event::AssistantMessage(AssistantMessage::new(
                "hi",
                AssistantMessageSource::Controller,
            )),
            Event::ProviderStarted(ProviderStarted::new("stub-provider", "request-1")),
            Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("provider text"),
            )),
            Event::ActionProposed(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )),
            Event::ActionApproved(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )),
            Event::ActionApplied(ActionApplied::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                VerifiedActionResult::FileWritten {
                    path: "hello.py".to_string(),
                },
            )),
            Event::ActionRejected(ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::CreateFile,
                "write rejected.py",
            )),
            Event::ActionFailed(ActionFailed::new(
                "action-3",
                elgar_core::event::ActionKind::CreateFile,
                "permission denied",
            )),
            Event::Error(ErrorEvent::new("boom")),
        ];

        for event in &events {
            conversation.push_event(event);
        }

        let rendered = conversation.render_body();
        assert!(rendered.contains("> hello"));
        assert!(!rendered.contains("User\n"));
        assert!(rendered.contains("hi"));
        assert!(!rendered.contains("Elgar: hi"));
        assert!(!rendered.contains("thinking"));
        assert!(!rendered.contains("request-1"));
        assert!(!rendered.contains("Provider text is suggestion only."));
        assert!(rendered.contains("I can write hello.py. Approve to write it."));
        assert!(rendered.contains("Approved. Applying the action."));
        assert!(rendered.contains("Wrote hello.py."));
        assert!(rendered.contains("Rejected. No changes were made."));
        assert!(rendered.contains("Action failed: action-3 CreateFile permission denied"));
        assert!(rendered.contains("Error: boom"));
    }

    #[test]
    fn conversation_renders_shell_result_exit_code_and_output() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::ShellCommand,
            VerifiedActionResult::Shell(ShellActionVerification {
                command: "printf hello".to_string(),
                cwd: "/repo".to_string(),
                stdout: "hello\n".to_string(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                exit_code: Some(0),
                elapsed_millis: 12,
                timed_out: false,
                verified_effect: None,
            }),
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("Shell command finished: exit 0."));
        assert!(rendered.contains("stdout: hello"));
        assert!(!rendered.contains("Shell command finished and verification was recorded."));
    }

    #[test]
    fn empty_panes_render_default_body_text() {
        assert_eq!(
            ConversationPane::default().render_body(),
            "(empty conversation)"
        );
        assert_eq!(InputArea::default().render_body(), "> ");
        assert_eq!(CopyArea::default().render_hint(), "");
    }

    #[test]
    fn completed_provider_output_does_not_render_blank_rows() {
        let mut conversation = ConversationPane::default();
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Plan\n\n- One\n\n- Two\n\ncode:\n```python\nprint(\"one\")\n\nprint(\"two\")\n```",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_lines_with_styles();

        assert!(rendered
            .iter()
            .all(|(line, _style)| !line.trim().is_empty()));
        assert_eq!(
            rendered
                .iter()
                .map(|(line, _style)| line.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Plan",
                "- One",
                "- Two",
                "code:",
                "code (python):",
                "    print(\"one\")",
                "    print(\"two\")",
            ]
        );
    }

    #[test]
    fn copy_area_tracks_copy_result_without_changing_conversation() {
        let mut copy = CopyArea::default();

        copy.mark_copied(12);
        assert_eq!(copy.render_hint(), "copied conversation (12 bytes)");

        copy.mark_failed("terminal rejected OSC 52");
        assert_eq!(copy.render_hint(), "copy failed: terminal rejected OSC 52");
    }

    #[test]
    fn status_line_tracks_last_event_kind() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new("boom")));

        assert_eq!(status.text, "error");
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn conversation_renders_provider_errors_with_calm_copy() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));

        assert_eq!(
            conversation.render_body(),
            "Provider error · fake-provider\nProvider provider error (404): model missing"
        );
    }

    #[test]
    fn conversation_renders_controller_errors_without_provider_label() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));

        assert_eq!(
            conversation.render_body(),
            "Error: Input was not recognized."
        );
    }

    #[test]
    fn conversation_renders_assistant_markdown_as_presentation_only_text() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Plan:\n- **read** files\n- `render` output\n\n```rust\nfn main() {}\n```",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("Plan:\n- read files\n- render output"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("code (rust):\n    fn main() {}"));
        assert!(!rendered.contains("```"));
        assert!(!rendered.contains("**read**"));
    }

    #[test]
    fn conversation_renders_assistant_markdown_tables_readably() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "| File | State |\n| --- | --- |\n| src/lib.rs | changed |",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("  File"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("changed"));
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn conversation_uses_pi_style_user_block_and_unlabeled_provider_reply() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::UserMessage(UserMessage::new(
            "explain this\nin two lines",
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "short answer",
            AssistantMessageSource::Provider,
        )));

        assert_eq!(
            conversation.render_body(),
            "> explain this\n> in two lines\nshort answer"
        );
    }

    #[test]
    fn conversation_pulses_loading_inside_transcript() {
        let mut conversation = ConversationPane::default();

        conversation.push_pending_provider_turn("hello");
        assert_eq!(conversation.render_body(), "> hello\n◐ working");

        conversation.advance_loading_pulse();
        assert_eq!(conversation.render_body(), "> hello\n◓ working");

        conversation.discard_pending_provider_turn();
        assert_eq!(conversation.render_body(), "(empty conversation)");
    }

    #[test]
    fn conversation_renders_explicit_provider_thinking_before_model_answer() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer")
                .with_thinking("Need to respond as Elgar, short.\nSimple greeting."),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        let thinking_index = rendered.find("Need to respond as Elgar, short.").unwrap();
        let model_index = rendered.find("final answer").unwrap();

        assert!(!rendered.contains("Thinking\n"));
        assert!(!rendered.contains("thinking:"));
        assert!(thinking_index < model_index);
        assert!(!rendered.contains("request-1"));
    }

    #[test]
    fn conversation_renders_turn_duration_and_token_cost() {
        let mut conversation = ConversationPane::default();
        let usage = ProviderTokenUsage {
            prompt_tokens: Some(2_200),
            completion_tokens: Some(24),
            total_tokens: Some(2_224),
        };

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "lm-studio",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "lm-studio",
            "request-1",
            ProviderOutput::new("final answer"),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));
        conversation.push_turn_metrics(11_040, Some(&usage));

        let rendered = conversation.render_body();

        assert!(rendered.contains("response 11.0s · ↑2.2k ↓24 · 2.2k provider tokens"));
        assert!(rendered.contains("final answer"));
        assert!(!rendered.contains("request-1"));
    }

    #[test]
    fn conversation_renders_turn_duration_without_token_usage_when_missing() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "lm-studio",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "lm-studio",
            "request-1",
            ProviderOutput::new("final answer"),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));
        conversation.push_turn_metrics(11_040, None);

        let rendered = conversation.render_body();

        assert!(rendered.contains("response 11.0s"));
        assert!(!rendered.contains("↑"));
        assert!(!rendered.contains("tokens"));
        assert!(rendered.contains("final answer"));
    }

    #[test]
    fn conversation_hides_low_value_provider_thinking() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer").with_thinking("Answering succinctly."),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();

        assert!(rendered.contains("Answering succinctly"));
        assert!(rendered.contains("final answer"));
    }

    #[test]
    fn conversation_hides_provider_tool_planning_thinking_but_keeps_visible_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("Created the requested files.").with_thinking(
                "Create directory. Use create_directory tool. Path? Desktop relative: Desktop/ElgarLiveE2E.\n\
                 Create file plan.md in that folder. Use create_file.\n\
                 Create files per plan. Use create_file calls for each file. Also need to initialise project?\n\
                 Create files. Provide tool calls for each missing file.\n\
                 Create files with content. Provide tool calls only, one per file.",
            ),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Created the requested files.",
            AssistantMessageSource::Provider,
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "Desktop/ElgarLiveE2E/plan.md".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("Create directory."));
        assert!(!rendered.contains("Use create_directory tool"));
        assert!(!rendered.contains("Desktop relative"));
        assert!(!rendered.contains("Create file plan.md"));
        assert!(!rendered.contains("Use create_file"));
        assert!(!rendered.contains("Create files per plan"));
        assert!(!rendered.contains("initialise project"));
        assert!(!rendered.contains("Provide tool calls"));
        assert!(rendered.contains("Created the requested files."));
        assert!(rendered.contains("Wrote Desktop/ElgarLiveE2E/plan.md."));
    }

    #[test]
    fn conversation_hides_provider_thinking_for_tool_call_turns() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("").with_thinking(
                "We need to create folder ~/ElgarManualSmoke and set up a TS Next.js Tailwind project.\n\
                 Use write tool for each file. Let's implement.",
            )
            .with_tool_calls(vec![RawModelToolCall {
                id: "call-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": "package.json",
                    "contents": "{}\n"
                }),
                assistant_summary: None,
            }]),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/ElgarManualSmoke/package.json".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("We need to create folder"));
        assert!(!rendered.contains("Use write tool"));
        assert!(!rendered.contains("Let's implement"));
        assert_eq!(
            rendered,
            "Wrote /Users/yuval/ElgarManualSmoke/package.json."
        );
    }

    #[test]
    fn conversation_summarizes_consecutive_project_create_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateDirectory,
                "create next-tailwind-ts-project",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "/Users/yuval/next-tailwind-ts-project".to_string(),
                },
            ),
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::CreateDirectory,
                "create app",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "/Users/yuval/next-tailwind-ts-project/app".to_string(),
                },
            ),
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-3",
                elgar_core::event::ActionKind::CreateFile,
                "create package.json",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/next-tailwind-ts-project/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-4",
                elgar_core::event::ActionKind::CreateFile,
                "create app/page.tsx",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-4",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/next-tailwind-ts-project/app/page.tsx".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\nCreated project: /Users/yuval/next-tailwind-ts-project\nVerified: 2 folders, 2 files"
        );
        assert!(rendered.contains("Tool result"));
        assert!(rendered.contains("Verified: 2 folders, 2 files"));
        assert_eq!(rendered.lines().count(), 3);
        assert_eq!(
            conversation
                .render_lines_with_styles()
                .into_iter()
                .map(|(_line, style)| style)
                .collect::<Vec<_>>(),
            vec![
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool
            ]
        );
        assert!(!rendered.contains("Wrote /Users/yuval/next-tailwind-ts-project/package.json."));
        assert!(!rendered.contains("Wrote /Users/yuval/next-tailwind-ts-project/app/page.tsx."));
        assert!(!rendered.contains("Created /Users/yuval/next-tailwind-ts-project/app."));
        assert_eq!(conversation.render_copy_body(), rendered);
    }

    #[test]
    fn conversation_summarizes_project_create_results_across_interleaved_tool_error() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
                path: "/Users/yuval/__git/elgar/my-nextjs-app".to_string(),
            }),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/next-env.d.ts".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-4",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextapp/next.config.js".to_string(),
            },
        )));
        conversation.push_event(&Event::Error(ErrorEvent::new(
            "model tool `patch_file` is missing required argument `target_path`",
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-5",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\n\
             Created project: /Users/yuval/__git/elgar/my-nextjs-app\n\
             Verified: 1 folder, 4 files\n\
             Outside project: 1 file\n\
             Tool call incomplete: patch_file needs target_path. No action was applied."
        );
        assert_eq!(rendered.matches("Tool result").count(), 1);
        assert_eq!(rendered.matches("Tool call incomplete:").count(), 1);
        assert!(!rendered.contains("Created /Users/yuval/__git/elgar/my-nextjs-app."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/package.json."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/next-env.d.ts."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextapp/next.config.js."));
        assert!(
            !rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js.")
        );
        assert!(!rendered.contains("model tool `patch_file`"));
        assert_eq!(
            conversation
                .render_lines_with_styles()
                .into_iter()
                .map(|(_line, style)| style)
                .collect::<Vec<_>>(),
            vec![
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Plain
            ]
        );
    }

    #[test]
    fn conversation_summarizes_project_root_when_first_directory_is_child_folder() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
                path: "/Users/yuval/__git/elgar/demo/src".to_string(),
            }),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/demo/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/demo/src/App.tsx".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\n\
             Created project: /Users/yuval/__git/elgar/demo\n\
             Verified: 1 folder, 2 files"
        );
        assert!(!rendered.contains("Outside project"));
    }

    #[test]
    fn conversation_keeps_single_create_result_specific() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "Desktop/ElgarLiveE2E/plan.md".to_string(),
            },
        )));

        assert_eq!(
            conversation.render_body(),
            "Wrote Desktop/ElgarLiveE2E/plan.md."
        );
    }

    #[test]
    fn provider_thinking_filter_catches_tool_planning_without_upstream_help() {
        for hidden in [
            "Use create_directory tool.",
            "Use create_file calls for each file.",
            "Use shellcommand.",
            "Use shell command.",
            "Use write_file tool.",
            "Use planner tool call.",
            "Next tool call: create_file.",
        ] {
            assert!(
                is_low_value_provider_tool_planning_thinking(hidden),
                "{hidden:?} should be hidden"
            );
        }

        for visible in [
            "Reviewing the existing panes tests.",
            "Use clear wording in the final answer.",
            "Checking that normal provider answers remain visible.",
        ] {
            assert!(
                !is_low_value_provider_tool_planning_thinking(visible),
                "{visible:?} should remain visible"
            );
        }
    }

    #[test]
    fn conversation_copy_omits_provider_thinking_blocks_but_keeps_visible_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_line(
            "> Create a folder on my Desktop called ElgarLiveE2E".to_string(),
            ConversationLineStyle::User,
        );
        conversation.push_line(
            "Create directory on Desktop.\n\
             Create file plan.md in that directory.\n\
             Create files per plan: package.json, tsconfig.json, vite.config.ts maybe...\n\
             Create files. We don't have content. Should we ask guidance? Probably need to create files with...\n\
             Call create_file for each target_path with contents. Provide minimal starter files."
                .to_string(),
            ConversationLineStyle::Thinking,
        );
        conversation.push_line("Done.".to_string(), ConversationLineStyle::Plain);
        conversation.push_line(
            "Created Desktop/ElgarLiveE2E.".to_string(),
            ConversationLineStyle::Plain,
        );

        let rendered = conversation.render_body();
        let copied = conversation.render_copy_body();

        assert!(rendered.contains("Create directory on Desktop."));
        assert!(copied.contains("> Create a folder on my Desktop called ElgarLiveE2E"));
        assert!(copied.contains("Done."));
        assert!(copied.contains("Created Desktop/ElgarLiveE2E."));
        assert!(!copied.contains("Create directory on Desktop."));
        assert!(!copied.contains("Create file plan.md in that directory."));
        assert!(!copied.contains("Create files per plan"));
        assert!(!copied.contains("We don't have content"));
        assert!(!copied.contains("Should we ask guidance"));
        assert!(!copied.contains("Call create_file for each target_path"));
        assert!(!copied.contains("Provide minimal starter files"));
    }

    #[test]
    fn conversation_keeps_existing_progress_when_provider_thinking_is_absent() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer"),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("thinking"));
        assert!(rendered.contains("final answer"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("Thinking\nfinal answer"));
    }

    #[test]
    fn conversation_scrollback_computes_view_offset_without_changing_lines() {
        let mut conversation = ConversationPane {
            lines: (0..10).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };
        let original_lines = conversation.lines.clone();

        assert_eq!(conversation.scroll_offset(4), 6);

        conversation.scroll_up(2);
        assert_eq!(conversation.scroll_offset(4), 4);
        assert_eq!(conversation.lines, original_lines);

        conversation.scroll_down(1);
        assert_eq!(conversation.scroll_offset(4), 5);

        conversation.follow_latest();
        assert_eq!(conversation.scroll_offset(4), 6);
    }

    #[test]
    fn conversation_scrollback_clamps_to_available_content() {
        let mut conversation = ConversationPane {
            lines: (0..3).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };

        assert_eq!(conversation.scroll_offset(6), 0);

        conversation.scroll_up(100);
        assert_eq!(conversation.scroll_offset(2), 0);
    }

    #[test]
    fn status_line_distinguishes_provider_and_controller_errors() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));
        assert_eq!(status.render_body(), "provider error");

        status.observe_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn status_line_uses_compact_human_readable_provider_text() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        assert_eq!(status.text, "◐ working");
        assert!(status.provider_active());

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        assert_eq!(status.text, "reply ready");
        assert!(!status.provider_active());
    }

    #[test]
    fn status_line_cycles_terminal_safe_thinking_pulse() {
        let mut status = StatusLine::ready();

        status.start_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◓ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◑ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◒ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "reply ready");
    }
}
