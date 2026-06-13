//! Tests for approval command flow and approved primitive dispatch.

use std::fs;

use serde_json::json;

use crate::{
    harness::{
        approve_pending_approval, deny_pending_approval, ApprovalCommandError, PendingApproval,
        StructuredRequestKind, ValidatedStructuredRequest,
    },
    session::Session,
};

#[test]
fn deny_pending_approval_clears_session_slot() {
    let root = std::env::temp_dir().join(format!(
        "elgar-deny-pending-approval-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("deny-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &bash_request("echo no"),
        "needs approval",
    ));

    let result = deny_pending_approval(&mut session).unwrap();

    assert_eq!(result.approval_id, "approval-1");
    assert_eq!(result.status, "denied");
    assert!(session.pending_approval().is_none());
}

#[test]
fn approve_pending_bash_executes_stored_command() {
    let root =
        std::env::temp_dir().join(format!("elgar-approve-pending-bash-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("approve-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &bash_request("echo approved-bash"),
        "needs approval",
    ));

    let result = approve_pending_approval(&mut session).unwrap();

    assert_eq!(result.approval_id, "approval-1");
    assert_eq!(result.status, "approved");
    assert!(result.message.contains("VERIFIED_BASH_EXECUTION"));
    assert!(result.message.contains("approved-bash"));
    assert!(result.message.contains("requested_cwd:"));
    assert!(result.message.contains("resolved_cwd:"));
    assert!(session.pending_approval().is_none());
}

#[test]
fn approve_pending_bash_runs_in_resolved_cwd() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-bash-cwd-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("nested")).unwrap();
    let cwd = root.join("nested").join("..").join("nested");
    let expected = cwd.canonicalize().unwrap();
    let mut session = Session::new("approve-bash-cwd-session", &root, &cwd);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &bash_request("pwd"),
        "needs approval",
    ));

    let result = approve_pending_approval(&mut session).unwrap();

    assert!(result
        .message
        .contains(&format!("resolved_cwd: {}", expected.display())));
    assert!(result.message.contains(&expected.display().to_string()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_batch_executes_write_steps_serially() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-batch-write-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let requests = vec![write_request("a.txt", "A"), write_request("b.txt", "B")];
    let mut session = Session::new("approve-batch-write-session", &root, &root);
    session.set_pending_approval(
        PendingApproval::from_requests_with_launch_cwd(
            "approval-1",
            &requests,
            "batch needs approval",
            &root,
        )
        .unwrap(),
    );

    let result = approve_pending_approval(&mut session).unwrap();

    assert_eq!(result.approval_id, "approval-1");
    assert_eq!(result.status, "approved");
    assert!(result.message.contains("VERIFIED_BATCH_EXECUTION"));
    assert!(result.message.contains("approval-1-step-1"));
    assert!(result.message.contains("approval-1-step-2"));
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "A");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "B");
    assert!(session.pending_approval().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_bash_rejects_missing_cwd_before_execution() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-bash-missing-cwd-{}",
        std::process::id()
    ));
    let missing_cwd = root.join("missing");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("approve-bash-missing-cwd-session", &root, &missing_cwd);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &bash_request("touch should-not-exist"),
        "needs approval",
    ));

    let error = approve_pending_approval(&mut session).unwrap_err();

    assert!(matches!(error, ApprovalCommandError::ExecutionFailed(_)));
    assert!(!root.join("should-not-exist").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_write_creates_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-write-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("write-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &write_request("nested/demo.txt", "hello\n"),
        "needs approval",
    ));

    let result = approve_pending_approval(&mut session).unwrap();

    assert_eq!(result.status, "approved");
    assert!(result.message.contains("VERIFIED_WRITE_EXECUTION"));
    assert_eq!(
        fs::read_to_string(root.join("nested/demo.txt")).unwrap(),
        "hello\n"
    );
    assert!(session.pending_approval().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_write_overwrites_file() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-write-overwrite-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("demo.txt"), "old").unwrap();
    let mut session = Session::new("write-overwrite-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &write_request("demo.txt", "new"),
        "needs approval",
    ));

    approve_pending_approval(&mut session).unwrap();

    assert_eq!(fs::read_to_string(root.join("demo.txt")).unwrap(), "new");
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn approve_pending_write_rejects_symlink_target() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-write-symlink-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("real.txt"), "real").unwrap();
    symlink(root.join("real.txt"), root.join("link.txt")).unwrap();
    let mut session = Session::new("write-symlink-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &write_request("link.txt", "no"),
        "needs approval",
    ));

    let error = approve_pending_approval(&mut session).unwrap_err();

    assert!(matches!(error, ApprovalCommandError::PathRejected(_)));
    assert_eq!(fs::read_to_string(root.join("real.txt")).unwrap(), "real");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_edit_replaces_exact_text() {
    let root =
        std::env::temp_dir().join(format!("elgar-approve-pending-edit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("demo.txt"), "alpha beta gamma").unwrap();
    let mut session = Session::new("edit-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &edit_request("demo.txt", "beta", "BETA"),
        "needs approval",
    ));

    let result = approve_pending_approval(&mut session).unwrap();

    assert!(result.message.contains("VERIFIED_EDIT_EXECUTION"));
    assert_eq!(
        fs::read_to_string(root.join("demo.txt")).unwrap(),
        "alpha BETA gamma"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_edit_rejects_missing_old_text() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-edit-missing-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("demo.txt"), "alpha beta gamma").unwrap();
    let mut session = Session::new("edit-missing-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &edit_request("demo.txt", "delta", "DELTA"),
        "needs approval",
    ));

    let error = approve_pending_approval(&mut session).unwrap_err();

    assert!(matches!(error, ApprovalCommandError::InvalidEdit(_)));
    assert_eq!(
        fs::read_to_string(root.join("demo.txt")).unwrap(),
        "alpha beta gamma"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_pending_edit_rejects_multiple_old_text_matches() {
    let root = std::env::temp_dir().join(format!(
        "elgar-approve-pending-edit-multiple-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("demo.txt"), "alpha beta beta").unwrap();
    let mut session = Session::new("edit-multiple-session", &root, &root);
    session.set_pending_approval(PendingApproval::from_request(
        "approval-1",
        &edit_request("demo.txt", "beta", "BETA"),
        "needs approval",
    ));

    let error = approve_pending_approval(&mut session).unwrap_err();

    assert!(matches!(error, ApprovalCommandError::InvalidEdit(_)));
    assert_eq!(
        fs::read_to_string(root.join("demo.txt")).unwrap(),
        "alpha beta beta"
    );
    let _ = fs::remove_dir_all(root);
}

fn bash_request(command: &str) -> ValidatedStructuredRequest {
    ValidatedStructuredRequest {
        kind: StructuredRequestKind::Bash,
        reason: "test".to_string(),
        arguments: Some(json!({ "command": command })),
    }
}

fn write_request(path: &str, content: &str) -> ValidatedStructuredRequest {
    ValidatedStructuredRequest {
        kind: StructuredRequestKind::Write,
        reason: "test".to_string(),
        arguments: Some(json!({ "path": path, "content": content })),
    }
}

fn edit_request(path: &str, old_text: &str, new_text: &str) -> ValidatedStructuredRequest {
    ValidatedStructuredRequest {
        kind: StructuredRequestKind::Edit,
        reason: "test".to_string(),
        arguments: Some(json!({
            "path": path,
            "old_text": old_text,
            "new_text": new_text
        })),
    }
}
