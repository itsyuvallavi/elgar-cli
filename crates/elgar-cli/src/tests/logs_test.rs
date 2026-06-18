//! Tests for read-only log diagnostics.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    is_logs_command, render_latest_turn_summary, render_logs_latest_from_args, run_logs_from_args,
    LogsDiagnosticError,
};

#[test]
fn logs_command_is_explicit() {
    assert!(is_logs_command(&["logs".to_string(), "latest".to_string()]));
    assert!(is_logs_command(&["logs".to_string()]));
    assert!(!is_logs_command(&["hello".to_string()]));
}

#[test]
fn logs_latest_args_require_latest_subcommand() {
    let error = render_logs_latest_from_args(&["logs".to_string()], PathBuf::from(".").as_path())
        .unwrap_err();

    assert_eq!(error, LogsDiagnosticError::UnsupportedCommand);
    assert_eq!(
        error.to_string(),
        "usage: elgar logs latest | elgar logs --follow"
    );
}

#[test]
fn logs_latest_dispatch_writes_summary() {
    let root = test_root("logs-dispatch-latest");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("terminal-tui-session.jsonl"),
        r#"{"session_id":"terminal-tui-session","turn_id":0,"summary":"turn_perf_summary","duration_ms":4382,"metadata":{"backend":"OpenAiChatCompletions","api_call_count":1,"unknown_provider_call_count":0,"provider_request_count":1,"loop_round_count":0,"tool_request_count":0,"tool_execution_count":0,"permission_prompt_count":0,"permission_approved_count":0,"permission_denied_count":0,"synthesis_count":0,"prompt_tokens":12,"completion_tokens":196,"total_tokens":208,"total_provider_duration_millis":4378,"stream":false,"error":false}}"#,
    )
    .unwrap();
    let mut output = Vec::new();

    run_logs_from_args(
        &["logs".to_string(), "latest".to_string()],
        &root,
        &mut output,
    )
    .unwrap();
    let rendered = String::from_utf8(output).unwrap();

    assert!(rendered.contains("Latest turn summary"));
    assert!(rendered.contains("session: terminal-tui-session"));
}

#[test]
fn latest_turn_summary_renders_compact_view() {
    let root = test_root("latest-turn-summary");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("terminal-tui-session.jsonl"),
        r#"{"session_id":"terminal-tui-session","turn_id":0,"summary":"turn_perf_summary","duration_ms":4382,"metadata":{"backend":"OpenAiChatCompletions","api_call_count":1,"unknown_provider_call_count":0,"provider_request_count":1,"loop_round_count":0,"tool_request_count":0,"tool_execution_count":0,"permission_prompt_count":0,"permission_approved_count":0,"permission_denied_count":0,"synthesis_count":0,"prompt_tokens":12,"completion_tokens":196,"total_tokens":208,"total_provider_duration_millis":4378,"stream":false,"error":false}}"#,
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("Latest turn summary"));
    assert!(rendered.contains("session: terminal-tui-session"));
    assert!(rendered.contains("backend: OpenAiChatCompletions"));
    assert!(rendered.contains("duration: 4.4s"));
    assert!(rendered.contains("provider duration: 4.4s"));
    assert!(rendered.contains("tokens: ↑12 ↓196 = 208"));
    assert!(rendered.contains("provider calls: 1 (api 1, unknown 0)"));
    assert!(rendered.contains("stream: false"));
    assert!(rendered.contains("error: false"));
}

#[test]
fn latest_turn_summary_prefers_harness_summary() {
    let root = test_root("latest-harness-summary");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("cli-runtime.jsonl"),
        [
            r#"{"session_id":"cli-runtime","turn_id":0,"summary":"harness_turn_started","metadata":{"harness_mode":"read_only_primitive_loop"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_provider_call_finished","metadata":{"backend":"OpenAiChatCompletions","prompt_tokens":1000,"completion_tokens":50,"total_tokens":1050}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_model_choice","metadata":{"choice_type":"structured_requests","tools":["read","ls"]}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_repair_finished","metadata":{"repaired_choice_type":"structured_request"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_synthesis_finished","metadata":{"backend":"OpenAiChatCompletions","prompt_tokens":400,"completion_tokens":100,"total_tokens":500}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_mcp_status","metadata":{"mcp_active":true,"server_ids":["project-index","context7"],"source_path":"/Users/yuval/.elgar/config/elgar-mcp.json"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_finished","duration_ms":2500,"metadata":{"rounds":2,"stopped_reason":"model_message_after_evidence","has_final_text":true}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("Latest harness summary"));
    assert!(rendered.contains("session: cli-runtime"));
    assert!(rendered.contains("backend: OpenAiChatCompletions"));
    assert!(rendered.contains("duration: 2.5s"));
    assert!(rendered.contains("rounds: 2"));
    assert!(rendered.contains("stop reason: model_message_after_evidence"));
    assert!(rendered.contains("tokens: ↑1.4k ↓150 = 1.6k"));
    assert!(rendered.contains("provider calls: 2"));
    assert!(rendered.contains("tools: read, ls"));
    assert!(rendered.contains(
        "mcp: active · servers project-index, context7 · source /Users/yuval/.elgar/config/elgar-mcp.json"
    ));
    assert!(rendered.contains("repairs: 1"));
    assert!(rendered.contains("synthesis: 1"));
    assert!(rendered.contains("error: false"));
}

#[test]
fn latest_turn_summary_counts_native_evidence_tools() {
    let root = test_root("latest-harness-native-tools");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("cli-runtime.jsonl"),
        [
            r#"{"session_id":"cli-runtime","turn_id":0,"summary":"harness_turn_started","metadata":{"harness_mode":"read_only_primitive_loop"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_provider_call_finished","metadata":{"backend":"OpenAiChatCompletions","prompt_tokens":426,"completion_tokens":19,"total_tokens":445}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_evidence_collected","metadata":{"evidence_label":"read:package.json","round_index":0}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_provider_call_finished","metadata":{"backend":"OpenAiChatCompletions","prompt_tokens":702,"completion_tokens":197,"total_tokens":899}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_finished","duration_ms":6993,"metadata":{"rounds":1,"stopped_reason":"native_final_text","has_final_text":true}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("Latest harness summary"));
    assert!(rendered.contains("stop reason: native_final_text"));
    assert!(rendered.contains("tools: read"));
    assert!(rendered.contains("provider calls: 2"));
}

#[test]
fn latest_turn_summary_counts_permission_decisions() {
    let root = test_root("latest-harness-permission-decisions");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("cli-runtime.jsonl"),
        [
            r#"{"session_id":"cli-runtime","turn_id":0,"summary":"harness_turn_started","metadata":{"harness_mode":"read_only_primitive_loop"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_permission_decision","metadata":{"tool":"bash","decision":"needs_approval","execution_allowed":false}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_approval_requested","metadata":{"approval_id":"approval-1","tool":"bash","status":"pending","execution_allowed":false}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_finished","duration_ms":1000,"metadata":{"rounds":1,"stopped_reason":"native_final_text","has_final_text":true}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_approval_decision","metadata":{"approval_id":"approval-1","tool":"bash","status":"approved"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_bash_execution_finished","metadata":{"approval_id":"approval-1","tool":"bash","exit_code":0}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("Latest harness summary"));
    assert!(rendered.contains("tools: bash"));
    assert!(rendered.contains("permissions: prompts 1 · approved 1 · denied 0"));
}

#[test]
fn latest_turn_summary_counts_approved_write_and_edit_tools() {
    let root = test_root("latest-harness-write-edit-tools");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join("cli-runtime.jsonl"),
        [
            r#"{"session_id":"cli-runtime","turn_id":0,"summary":"harness_turn_started","metadata":{"harness_mode":"native_tool_loop"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_approval_decision","metadata":{"approval_id":"approval-1","tool":"write","status":"approved"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_write_execution_finished","metadata":{"approval_id":"approval-1","tool":"write","exit_code":0}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_approval_decision","metadata":{"approval_id":"approval-2","tool":"edit","status":"approved"}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_edit_execution_finished","metadata":{"approval_id":"approval-2","tool":"edit","exit_code":0}}"#,
            r#"{"session_id":"cli-runtime","turn_id":1,"summary":"harness_loop_finished","duration_ms":1000,"metadata":{"rounds":1,"stopped_reason":"approved_execution","has_final_text":true}}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("tools: write, edit"));
    assert!(rendered.contains("permissions: prompts 0 · approved 2 · denied 0"));
}

#[test]
fn latest_turn_summary_skips_newer_logs_without_summary() {
    let root = test_root("skip-newer-no-summary");
    let log_dir = root.join(".elgar/log/system");
    fs::create_dir_all(&log_dir).unwrap();
    let older = log_dir.join("terminal-tui-session.jsonl");
    let newer = log_dir.join("cli-provider-smoke.jsonl");

    fs::write(
        &older,
        r#"{"session_id":"terminal-tui-session","turn_id":0,"summary":"turn_perf_summary","duration_ms":4382,"metadata":{"backend":"OpenAiChatCompletions","api_call_count":1,"unknown_provider_call_count":0,"provider_request_count":1,"loop_round_count":0,"tool_request_count":0,"tool_execution_count":0,"permission_prompt_count":0,"permission_approved_count":0,"permission_denied_count":0,"synthesis_count":0,"prompt_tokens":12,"completion_tokens":196,"total_tokens":208,"total_provider_duration_millis":4378,"stream":false,"error":false}}"#,
    )
    .unwrap();
    fs::write(
        &newer,
        r#"{"session_id":"cli-provider-smoke","turn_id":0,"summary":"provider_smoke_started"}"#,
    )
    .unwrap();

    let rendered = render_latest_turn_summary(&root).unwrap();

    assert!(rendered.contains("file:"));
    assert!(rendered.contains("terminal-tui-session.jsonl"));
    assert!(rendered.contains("Latest turn summary"));
}

#[test]
fn latest_turn_summary_reports_missing_directory() {
    let root = test_root("missing-log-directory");
    let error = render_latest_turn_summary(&root).unwrap_err();

    assert!(matches!(error, LogsDiagnosticError::LogDirectoryMissing(_)));
}

fn test_root(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    std::env::temp_dir().join(format!("elgar-cli-logs-test-{name}-{millis}"))
}
