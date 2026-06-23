//! Handle validated primitive requests inside the harness loop.
//!
//! This module executes already-validated primitive requests and records the
//! verified evidence they produce.

use std::{
    fs,
    path::{Component, Path},
};

use crate::{
    harness::{
        decide_primitive_permission,
        harness_loop::{
            evidence::{
                execution::execute_primitive_request,
                keys::evidence_key_for_request,
                render::{error_evidence, permission_evidence, write_noop_evidence},
            },
            state::{
                budget::{BudgetCheck, PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_harness_approval_requested, log_harness_duplicate_rejected,
                    log_harness_memory_snapshot, log_loop_evidence, log_permission_decision,
                },
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopRound},
            },
        },
        PendingApproval, PermissionDecisionKind, PrimitiveToolRegistry, StructuredRequestKind,
        ValidatedStructuredRequest,
    },
    session::Session,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestHandlingOutcome {
    UsefulEvidence,
    PendingApproval,
    NoProgress,
    ExecutionFailed,
}

/// Execute one validated request and append its verified evidence.
///
/// Returns `ExecutionFailed` when the caller should synthesize from error
/// evidence immediately.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_request_evidence(
    session: &mut Session,
    registry: &PrimitiveToolRegistry,
    request: &ValidatedStructuredRequest,
    round_index: usize,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    memory: &mut HarnessWorkingMemory,
    rounds: &mut Vec<PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<Evidence>,
) -> Result<RequestHandlingOutcome, String> {
    let key = evidence_key_for_request(request, budget_state.mutation_epoch());
    match budget_state.check_request(budget, &key) {
        Err(reason) => return Err(reason),
        Ok(check) => match check {
            BudgetCheck::Accept => {}
            BudgetCheck::RepeatedEvidence(label) => {
                memory.record_duplicate_request(label.clone());
                log_harness_duplicate_rejected(session, round_index, &label, memory);
                log_harness_memory_snapshot(session, round_index, "duplicate_request", memory);
                rounds.push(PrimitiveHarnessLoopRound {
                    round_index,
                    tool: Some("notice".to_string()),
                    evidence_label: Some(format!("duplicate:{label}")),
                });
                if memory.duplicate_streak() >= 2 {
                    return Err("duplicate_loop_detected".to_string());
                }
                return Ok(RequestHandlingOutcome::NoProgress);
            }
        },
    }

    if let Some(evidence_item) = same_content_write_noop(session, request, key.as_label()) {
        log_loop_evidence(session, round_index, &evidence_item);
        budget_state.record(&evidence_item);
        log_harness_memory_snapshot(session, round_index, "noop_request", memory);
        rounds.push(PrimitiveHarnessLoopRound {
            round_index,
            tool: Some(request.kind.as_str().to_string()),
            evidence_label: Some(evidence_item.label.clone()),
        });
        evidence.push(evidence_item);
        return Ok(RequestHandlingOutcome::UsefulEvidence);
    }

    let permission = decide_primitive_permission(registry, request, session.permission_mode());
    log_permission_decision(session, round_index, request, &permission);
    if !permission.allows_execution() {
        let approval_id = if matches!(permission.kind, PermissionDecisionKind::NeedsApproval) {
            let approval_id = session.next_approval_id();
            let approval = PendingApproval::from_request_with_launch_cwd(
                &approval_id,
                request,
                permission.reason.clone(),
                &session.cwd,
            );
            session.set_pending_approval(approval.clone());
            log_harness_approval_requested(session, round_index, &approval);
            Some(approval_id)
        } else {
            None
        };
        let evidence_item =
            permission_evidence(key.as_label(), request, &permission, approval_id.as_deref());
        log_loop_evidence(session, round_index, &evidence_item);
        budget_state.record(&evidence_item);
        log_harness_memory_snapshot(session, round_index, "permission_blocked", memory);
        rounds.push(PrimitiveHarnessLoopRound {
            round_index,
            tool: Some(request.kind.as_str().to_string()),
            evidence_label: Some(evidence_item.label.clone()),
        });
        evidence.push(evidence_item);
        return Ok(if approval_id.is_some() {
            RequestHandlingOutcome::PendingApproval
        } else {
            RequestHandlingOutcome::UsefulEvidence
        });
    }

    let execution_result = execute_primitive_request(session, request);
    let execution_failed = execution_result.is_err();
    let (evidence_item, directory_listing) = match execution_result {
        Ok(executed) => (executed.evidence, executed.directory_listing),
        Err(error) => (error_evidence(key.as_label(), &error.to_string()), None),
    };

    log_loop_evidence(session, round_index, &evidence_item);
    budget_state.record_key(&key);
    memory.record_useful_request(&key);
    if !execution_failed
        && matches!(
            request.kind,
            StructuredRequestKind::Write | StructuredRequestKind::Edit
        )
    {
        budget_state.advance_mutation_epoch();
    }
    if let Some(listing) = directory_listing {
        memory.record_directory_listing(listing);
    }
    log_harness_memory_snapshot(session, round_index, "useful_evidence", memory);
    rounds.push(PrimitiveHarnessLoopRound {
        round_index,
        tool: Some(request.kind.as_str().to_string()),
        evidence_label: Some(evidence_item.label.clone()),
    });
    evidence.push(evidence_item);

    if execution_failed {
        Ok(RequestHandlingOutcome::ExecutionFailed)
    } else {
        Ok(RequestHandlingOutcome::UsefulEvidence)
    }
}

fn same_content_write_noop(
    session: &Session,
    request: &ValidatedStructuredRequest,
    label: String,
) -> Option<Evidence> {
    if request.kind != StructuredRequestKind::Write {
        return None;
    }
    let arguments = request.arguments.as_ref()?;
    let path = arguments.get("path")?.as_str()?.trim();
    let content = arguments.get("content")?.as_str()?;
    if !is_safe_relative_path(path) {
        return None;
    }

    let target = session.cwd.join(path);
    let metadata = fs::symlink_metadata(&target).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let existing = fs::read_to_string(&target).ok()?;
    if existing != content {
        return None;
    }

    Some(write_noop_evidence(
        label,
        "target file already contains the requested content; choose a different missing output if work remains",
    ))
}

fn is_safe_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}
