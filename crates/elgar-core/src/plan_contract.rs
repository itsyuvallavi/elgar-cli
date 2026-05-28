use std::{
    fs,
    path::{Path, PathBuf},
};

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
        let (verification_steps, acceptance_criteria) =
            review_metadata_from_source_plan(&plan.source_plan_path);
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
                verification_steps,
                acceptance_criteria,
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

    pub fn review_draft(&self) -> PlanContractDraftReview {
        let mut issues = Vec::new();
        let status = self.runtime_status();

        if status != PlanContractStatus::Draft {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::ContractNotDraft { status },
                path: None,
            });
        }

        if !self.source_plan_path.is_file() {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::MissingSourcePlan,
                path: Some(self.source_plan_path.clone()),
            });
        } else if !self.source_plan_path.starts_with(&self.project_root) {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::SourcePlanOutsideProjectRoot,
                path: Some(self.source_plan_path.clone()),
            });
        }

        if !self.project_root.is_dir() {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::MissingProjectRoot,
                path: Some(self.project_root.clone()),
            });
        }

        let scope_paths: Vec<&PathBuf> = self
            .scope
            .allowed_files
            .iter()
            .chain(self.scope.allowed_directories.iter())
            .collect();

        if scope_paths.is_empty() {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::EmptyExecutableScope,
                path: None,
            });
        }

        for path in &scope_paths {
            if !path.starts_with(&self.project_root) {
                issues.push(PlanContractDraftIssue {
                    severity: PlanContractDraftIssueSeverity::Blocking,
                    kind: PlanContractDraftIssueKind::PathOutsideProjectRoot,
                    path: Some((*path).clone()),
                });
            }
        }

        for (index, path) in scope_paths.iter().enumerate() {
            if scope_paths
                .iter()
                .skip(index + 1)
                .any(|other| other == path)
            {
                issues.push(PlanContractDraftIssue {
                    severity: PlanContractDraftIssueSeverity::Blocking,
                    kind: PlanContractDraftIssueKind::DuplicateScopePath,
                    path: Some((*path).clone()),
                });
            }
        }

        if self.scope.verification_steps.is_empty() {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::MissingVerificationSteps,
                path: None,
            });
        }

        if self.scope.acceptance_criteria.is_empty() {
            issues.push(PlanContractDraftIssue {
                severity: PlanContractDraftIssueSeverity::Blocking,
                kind: PlanContractDraftIssueKind::MissingAcceptanceCriteria,
                path: None,
            });
        }

        PlanContractDraftReview { issues }
    }

    pub fn validate_path_execution(&self, path: &Path) -> Result<(), PlanContractViolation> {
        match self.runtime_status() {
            PlanContractStatus::Approved | PlanContractStatus::Executing => {}
            PlanContractStatus::Stale => {
                return Err(PlanContractViolation {
                    kind: PlanContractViolationKind::Stale,
                    path: path.to_path_buf(),
                });
            }
            status => {
                return Err(PlanContractViolation {
                    kind: PlanContractViolationKind::NotApproved { status },
                    path: path.to_path_buf(),
                });
            }
        }

        if !self.allows_path(path) {
            return Err(PlanContractViolation {
                kind: PlanContractViolationKind::OutOfScope,
                path: path.to_path_buf(),
            });
        }

        Ok(())
    }
}

fn review_metadata_from_source_plan(path: &Path) -> (Vec<String>, Vec<String>) {
    fs::read_to_string(path)
        .map(|contents| {
            (
                extract_markdown_section_items(&contents, "verification"),
                extract_markdown_section_items(&contents, "acceptance criteria"),
            )
        })
        .unwrap_or_else(|_| (Vec::new(), Vec::new()))
}

fn extract_markdown_section_items(contents: &str, heading: &str) -> Vec<String> {
    let mut in_section = false;
    let mut items = Vec::new();

    for line in contents.lines() {
        if let Some(current_heading) = markdown_heading_text(line) {
            in_section = current_heading == heading;
            continue;
        }

        if !in_section {
            continue;
        }

        if let Some(item) = markdown_list_item_text(line) {
            if !items.iter().any(|existing| existing == &item) {
                items.push(item);
            }
        }
    }

    items
}

fn markdown_heading_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    Some(trimmed.trim_start_matches('#').trim().to_ascii_lowercase())
}

fn markdown_list_item_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let item = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| numbered_markdown_list_item(trimmed))?
        .trim();

    (!item.is_empty()).then(|| item.to_string())
}

fn numbered_markdown_list_item(trimmed: &str) -> Option<&str> {
    let (prefix, suffix) = trimmed.split_once(". ")?;
    (!prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())).then_some(suffix)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContractDraftReview {
    pub issues: Vec<PlanContractDraftIssue>,
}

impl PlanContractDraftReview {
    pub fn is_approvable(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|issue| issue.severity == PlanContractDraftIssueSeverity::Blocking)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContractDraftIssue {
    pub severity: PlanContractDraftIssueSeverity,
    pub kind: PlanContractDraftIssueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanContractDraftIssueSeverity {
    Blocking,
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanContractDraftIssueKind {
    ContractNotDraft { status: PlanContractStatus },
    MissingSourcePlan,
    MissingProjectRoot,
    SourcePlanOutsideProjectRoot,
    EmptyExecutableScope,
    PathOutsideProjectRoot,
    DuplicateScopePath,
    MissingVerificationSteps,
    MissingAcceptanceCriteria,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanContractViolation {
    pub kind: PlanContractViolationKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanContractViolationKind {
    NotApproved { status: PlanContractStatus },
    OutOfScope,
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
    fn draft_contract_from_structured_plan_extracts_review_sections() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-draft-review-sections-{}",
            std::process::id()
        ));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Plan\n\n```text\nsrc/main.py\n```\n\n## Verification\n- Run the CLI smoke check.\n\n## Acceptance Criteria\n1. The expected file exists.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("src/main.py")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert_eq!(
            contract.scope.verification_steps,
            vec!["Run the CLI smoke check."]
        );
        assert_eq!(
            contract.scope.acceptance_criteria,
            vec!["The expected file exists."]
        );
        assert!(contract.review_draft().is_approvable());

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

    #[test]
    fn draft_review_requires_verification_and_acceptance_before_approval() {
        let root =
            std::env::temp_dir().join(format!("elgar-plan-contract-review-{}", std::process::id()));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
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

        let review = contract.review_draft();
        assert!(!review.is_approvable());
        assert!(review
            .issues
            .iter()
            .any(|issue| issue.kind == PlanContractDraftIssueKind::MissingVerificationSteps));
        assert!(review
            .issues
            .iter()
            .any(|issue| issue.kind == PlanContractDraftIssueKind::MissingAcceptanceCriteria));

        contract
            .scope
            .verification_steps
            .push("run the CLI help command".to_string());
        contract
            .scope
            .acceptance_criteria
            .push("src/main.py and requirements.txt match the approved plan".to_string());

        assert!(contract.review_draft().is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_blocks_empty_duplicate_and_out_of_root_scope() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-invalid-{}",
            std::process::id()
        ));
        let project = root.join("demo");
        let other = root.join("other");
        let plan_path = project.join("plan.md");
        let external_plan_path = other.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();
        fs::write(&external_plan_path, "# Other plan\n").unwrap();

        let empty_contract = PlanContract {
            id: "contract-empty".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: None,
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: Vec::new(),
                allowed_files: Vec::new(),
                allowed_command_classes: Vec::new(),
                verification_steps: vec!["inspect expected files".to_string()],
                acceptance_criteria: vec!["all expected files are present".to_string()],
                revision_reason: None,
            },
            approval: None,
        };
        let empty_review = empty_contract.review_draft();
        assert!(!empty_review.is_approvable());
        assert!(empty_review
            .issues
            .iter()
            .any(|issue| issue.kind == PlanContractDraftIssueKind::EmptyExecutableScope));

        let duplicate_path = project.join("src/main.py");
        let scoped_contract = PlanContract {
            id: "contract-scoped".to_string(),
            source_plan_path: external_plan_path.clone(),
            project_root: project,
            source_action_id: None,
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: Vec::new(),
                allowed_files: vec![
                    duplicate_path.clone(),
                    duplicate_path,
                    other.join("outside.py"),
                ],
                allowed_command_classes: Vec::new(),
                verification_steps: vec!["inspect expected files".to_string()],
                acceptance_criteria: vec!["all expected files are present".to_string()],
                revision_reason: None,
            },
            approval: None,
        };
        let scoped_review = scoped_contract.review_draft();
        assert!(!scoped_review.is_approvable());
        assert!(scoped_review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::SourcePlanOutsideProjectRoot
                && issue.path.as_ref() == Some(&external_plan_path)
        }));
        assert!(scoped_review
            .issues
            .iter()
            .any(|issue| issue.kind == PlanContractDraftIssueKind::DuplicateScopePath));
        assert!(scoped_review
            .issues
            .iter()
            .any(|issue| issue.kind == PlanContractDraftIssueKind::PathOutsideProjectRoot));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn path_execution_validation_requires_approved_in_scope_contract() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-validate-{}",
            std::process::id()
        ));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
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
        let allowed_file = project.join("src/main.py");
        let unexpected_file = project.join("README.md");

        assert_eq!(
            contract.validate_path_execution(&allowed_file),
            Err(PlanContractViolation {
                kind: PlanContractViolationKind::NotApproved {
                    status: PlanContractStatus::Draft,
                },
                path: allowed_file.clone(),
            })
        );

        contract.approve("user", "2026-05-28T12:00:00Z");
        assert_eq!(contract.validate_path_execution(&allowed_file), Ok(()));
        assert_eq!(
            contract.validate_path_execution(&unexpected_file),
            Err(PlanContractViolation {
                kind: PlanContractViolationKind::OutOfScope,
                path: unexpected_file,
            })
        );

        fs::remove_file(project.join("plan.md")).unwrap();
        assert_eq!(
            contract.validate_path_execution(&allowed_file),
            Err(PlanContractViolation {
                kind: PlanContractViolationKind::Stale,
                path: allowed_file,
            })
        );

        let _ = fs::remove_dir_all(root);
    }
}
