//! Handle validated primitive requests inside the harness loop.
//!
//! This module executes already-validated primitive requests and records the
//! verified evidence they produce.

use crate::{
    harness::{
        decide_primitive_permission,
        harness_loop::{
            evidence::execution::{
                error_evidence, evidence_key_for_request, execute_read_only_request,
                permission_evidence,
            },
            state::{
                budget::{BudgetCheck, PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_harness_duplicate_rejected, log_harness_memory_snapshot, log_loop_evidence,
                    log_permission_decision,
                },
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopRound},
            },
        },
        PrimitiveToolRegistry, ValidatedStructuredRequest,
    },
    session::Session,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestHandlingOutcome {
    UsefulEvidence,
    NoProgress,
    ExecutionFailed,
}

/// Execute one validated request and append its verified evidence.
///
/// Returns `ExecutionFailed` when the caller should synthesize from error
/// evidence immediately.
pub(super) fn collect_request_evidence(
    session: &Session,
    registry: &PrimitiveToolRegistry,
    request: &ValidatedStructuredRequest,
    round_index: usize,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    memory: &mut HarnessWorkingMemory,
    rounds: &mut Vec<PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<Evidence>,
) -> Result<RequestHandlingOutcome, String> {
    let key = evidence_key_for_request(request);
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
                if memory.duplicate_count() >= 2 {
                    return Err("duplicate_loop_detected".to_string());
                }
                return Ok(RequestHandlingOutcome::NoProgress);
            }
        },
    }

    let permission = decide_primitive_permission(registry, request);
    log_permission_decision(session, round_index, request, &permission);
    if !permission.allows_execution() {
        let evidence_item = permission_evidence(key.as_label(), request, &permission);
        log_loop_evidence(session, round_index, &evidence_item);
        budget_state.record(&evidence_item);
        log_harness_memory_snapshot(session, round_index, "permission_blocked", memory);
        rounds.push(PrimitiveHarnessLoopRound {
            round_index,
            tool: Some(request.kind.as_str().to_string()),
            evidence_label: Some(evidence_item.label.clone()),
        });
        evidence.push(evidence_item);
        return Ok(RequestHandlingOutcome::UsefulEvidence);
    }

    let execution_result = execute_read_only_request(session, request);
    let execution_failed = execution_result.is_err();
    let (evidence_item, directory_listing) = match execution_result {
        Ok(executed) => (executed.evidence, executed.directory_listing),
        Err(error) => (error_evidence(key.as_label(), &error.to_string()), None),
    };

    log_loop_evidence(session, round_index, &evidence_item);
    budget_state.record(&evidence_item);
    memory.record_useful_request(&key);
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
