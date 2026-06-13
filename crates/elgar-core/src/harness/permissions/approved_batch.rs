//! Approved batch primitive execution.
//!
//! A batch approval executes exact stored risky primitive requests one at a
//! time after a single user approval decision.

use crate::{
    harness::{PendingApproval, PendingApprovalStep, StructuredRequestKind},
    session::Session,
};

use super::{
    approval_flow::{ApprovalCommandError, ApprovalCommandResult},
    approval_logging::{log_batch_step_failed, log_batch_step_finished, log_batch_step_started},
    approved_bash::execute_approved_bash,
    approved_edit::execute_approved_edit,
    approved_write::execute_approved_write,
};

pub(super) fn execute_approved_batch(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let step_count = approval.steps.len();
    let mut output = format!(
        "VERIFIED_BATCH_EXECUTION\napproval_id: {}\nsteps: {}\n",
        approval.id, step_count
    );

    for (index, step) in approval.steps.iter().enumerate() {
        let step_number = index + 1;
        log_batch_step_started(session, &approval, step_number, step_count, &step.tool);
        match execute_batch_step(session, &approval, step, step_number) {
            Ok(result) => {
                log_batch_step_finished(
                    session,
                    &approval,
                    step_number,
                    step_count,
                    &step.tool,
                    "executed",
                );
                output.push_str(&format!(
                    "\n--- batch step {step_number}/{step_count} ---\n"
                ));
                output.push_str(&result.message);
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
            Err(error) => {
                let rendered = error.to_string();
                log_batch_step_failed(
                    session,
                    &approval,
                    step_number,
                    step_count,
                    &step.tool,
                    &rendered,
                );
                output.push_str(&format!(
                    "\n--- batch step {step_number}/{step_count} failed ---\n{rendered}\n"
                ));
                return Err(ApprovalCommandError::ExecutionFailed(output));
            }
        }
    }

    Ok(ApprovalCommandResult {
        approval_id: approval.id.clone(),
        status: approval.status.as_str(),
        message: output,
    })
}

fn execute_batch_step(
    session: &mut Session,
    batch: &PendingApproval,
    step: &PendingApprovalStep,
    step_number: usize,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let step_approval = PendingApproval::from_request_with_launch_cwd(
        format!("{}-step-{}", batch.id, step_number),
        &step.request,
        batch.reason.clone(),
        &session.cwd,
    )
    .approve();

    match step.request.kind {
        StructuredRequestKind::Bash => execute_approved_bash(session, step_approval),
        StructuredRequestKind::Write => execute_approved_write(session, step_approval),
        StructuredRequestKind::Edit => execute_approved_edit(session, step_approval),
        StructuredRequestKind::Read
        | StructuredRequestKind::Ls
        | StructuredRequestKind::Find
        | StructuredRequestKind::Grep
        | StructuredRequestKind::McpCall => Err(ApprovalCommandError::UnsupportedApprovedTool(
            step.tool.clone(),
        )),
    }
}
