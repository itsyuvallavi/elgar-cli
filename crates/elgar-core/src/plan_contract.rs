use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::StructuredProjectPlan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContract {
    pub id: String,
    pub source_plan_path: PathBuf,
    pub project_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action_id: Option<String>,
    pub status: PlanContractStatus,
    pub scope: PlanContractScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<PlanContractApproval>,
}

impl PlanContract {
    pub fn draft_from_structured_plan(id: impl Into<String>, plan: &StructuredProjectPlan) -> Self {
        Self {
            id: id.into(),
            source_plan_path: plan.source_plan_path.clone(),
            project_root: plan.project_root.clone(),
            source_action_id: plan.source_action_id.clone(),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: plan.expected_directories.clone(),
                allowed_files: plan.expected_files.clone(),
                allowed_command_classes: Vec::new(),
                verification_steps: Vec::new(),
                acceptance_criteria: Vec::new(),
                revision_reason: None,
            },
            approval: None,
        }
    }

    pub fn runtime_status(&self) -> PlanContractStatus {
        if self.is_stale() {
            return PlanContractStatus::Stale;
        }

        self.status
    }

    pub fn approve(&mut self, source: impl Into<String>, approved_at: impl Into<String>) {
        self.status = PlanContractStatus::Approved;
        self.approval = Some(PlanContractApproval {
            source: source.into(),
            approved_at: approved_at.into(),
        });
    }

    pub fn mark_executing(&mut self) {
        self.status = PlanContractStatus::Executing;
    }

    pub fn mark_needs_revision(&mut self, reason: impl Into<String>) {
        self.status = PlanContractStatus::NeedsRevision;
        self.scope.revision_reason = Some(reason.into());
    }

    pub fn mark_completed(&mut self) {
        self.status = PlanContractStatus::Completed;
    }

    pub fn reject(&mut self) {
        self.status = PlanContractStatus::Rejected;
    }

    fn is_stale(&self) -> bool {
        !self.source_plan_path.is_file()
            || !self.project_root.is_dir()
            || (self.status == PlanContractStatus::Completed && !self.expected_paths_complete())
    }

    fn expected_paths_complete(&self) -> bool {
        self.scope
            .allowed_directories
            .iter()
            .all(|path| path.is_dir())
            && self.scope.allowed_files.iter().all(|path| path.is_file())
    }

    pub fn allows_path(&self, path: &Path) -> bool {
        self.scope
            .allowed_files
            .iter()
            .chain(self.scope.allowed_directories.iter())
            .any(|allowed| allowed == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContractScope {
    pub allowed_directories: Vec<PathBuf>,
    pub allowed_files: Vec<PathBuf>,
    pub allowed_command_classes: Vec<PlanCommandClass>,
    pub verification_steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContractApproval {
    pub source: String,
    pub approved_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanCommandClass {
    ReadOnly,
    Build,
    Test,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanContractStatus {
    Draft,
    Approved,
    Executing,
    NeedsRevision,
    Completed,
    Rejected,
    Stale,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::session::{StructuredProjectPlan, StructuredProjectPlanStatus};

    use super::*;

    #[test]
    fn draft_contract_from_structured_plan_copies_runtime_scope() {
        let root =
            std::env::temp_dir().join(format!("elgar-plan-contract-draft-{}", std::process::id()));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path.clone(),
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("src/main.py"), project.join("README.md")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert_eq!(contract.id, "contract-1");
        assert_eq!(contract.status, PlanContractStatus::Draft);
        assert_eq!(contract.runtime_status(), PlanContractStatus::Draft);
        assert_eq!(contract.source_plan_path, plan_path);
        assert_eq!(contract.project_root, project);
        assert_eq!(contract.source_action_id.as_deref(), Some("action-plan"));
        assert!(contract
            .scope
            .allowed_directories
            .contains(&plan.expected_directories[0]));
        assert!(contract
            .scope
            .allowed_files
            .contains(&plan.expected_files[0]));
        assert!(contract.allows_path(&plan.expected_files[1]));
        assert!(contract.approval.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn contract_lifecycle_tracks_approval_revision_completion_and_stale_state() {
        let root =
            std::env::temp_dir().join(format!("elgar-plan-contract-status-{}", std::process::id()));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("src/main.py")],
        };
        let mut contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        contract.approve("user", "2026-05-28T12:00:00Z");
        assert_eq!(contract.runtime_status(), PlanContractStatus::Approved);
        assert_eq!(
            contract
                .approval
                .as_ref()
                .map(|approval| approval.source.as_str()),
            Some("user")
        );

        contract.mark_executing();
        assert_eq!(contract.runtime_status(), PlanContractStatus::Executing);

        contract.mark_needs_revision("missing dependency decision");
        assert_eq!(contract.runtime_status(), PlanContractStatus::NeedsRevision);
        assert_eq!(
            contract.scope.revision_reason.as_deref(),
            Some("missing dependency decision")
        );

        fs::write(project.join("src/main.py"), "print('hello')\n").unwrap();
        contract.mark_completed();
        assert_eq!(contract.runtime_status(), PlanContractStatus::Completed);

        fs::remove_file(project.join("src/main.py")).unwrap();
        assert_eq!(contract.runtime_status(), PlanContractStatus::Stale);

        let _ = fs::remove_dir_all(root);
    }
}
