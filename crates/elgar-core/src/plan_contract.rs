use std::{
    fs,
    path::{Component, Path, PathBuf},
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
        let verification_checks = verification_checks_from_items(
            &verification_steps,
            &acceptance_criteria,
            &plan.project_root,
        );
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
                verification_checks,
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

            if has_malformed_scope_path_segment(path) {
                issues.push(PlanContractDraftIssue {
                    severity: PlanContractDraftIssueSeverity::Blocking,
                    kind: PlanContractDraftIssueKind::MalformedScopePath,
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

        let verification_checks = if self.scope.verification_checks.is_empty() {
            verification_checks_from_items(
                &self.scope.verification_steps,
                &self.scope.acceptance_criteria,
                &self.project_root,
            )
        } else {
            self.scope.verification_checks.clone()
        };

        for check in verification_checks {
            match check.kind {
                PlanVerificationCheckKind::PathExists { path }
                | PlanVerificationCheckKind::TestPath { path } => {
                    if path == self.source_plan_path {
                        continue;
                    }
                    if !scope_contains_referenced_path(&scope_paths, &path, &self.project_root) {
                        issues.push(PlanContractDraftIssue {
                            severity: PlanContractDraftIssueSeverity::Blocking,
                            kind: PlanContractDraftIssueKind::ReferencedPathMissingFromScope,
                            path: Some(path),
                        });
                    }
                }
                PlanVerificationCheckKind::PythonModule { module } => {
                    if !is_valid_python_module_reference(&module) {
                        issues.push(PlanContractDraftIssue {
                            severity: PlanContractDraftIssueSeverity::Blocking,
                            kind: PlanContractDraftIssueKind::InvalidPythonModuleReference {
                                module,
                            },
                            path: None,
                        });
                    }
                }
            }
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

fn scope_contains_referenced_path(
    scope_paths: &[&PathBuf],
    path: &Path,
    project_root: &Path,
) -> bool {
    if scope_paths
        .iter()
        .any(|scope_path| scope_path.as_path() == path)
    {
        return true;
    }
    if is_extensionless_reference_satisfied_by_scoped_file(scope_paths, path) {
        return true;
    }

    let Ok(relative) = path.strip_prefix(project_root) else {
        return false;
    };
    if relative.components().count() != 1 {
        return false;
    }
    let Some(file_name) = relative.file_name() else {
        return false;
    };

    scope_paths
        .iter()
        .any(|scope_path| scope_path.file_name() == Some(file_name))
}

fn is_extensionless_reference_satisfied_by_scoped_file(
    scope_paths: &[&PathBuf],
    path: &Path,
) -> bool {
    if path.extension().is_some() {
        return false;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !is_well_known_extensionless_file(file_name) {
        return false;
    }

    scope_paths.iter().any(|scope_path| {
        scope_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == file_name)
    })
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
            in_section = markdown_heading_matches(&current_heading, heading);
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
    Some(normalize_markdown_heading(
        trimmed.trim_start_matches('#').trim(),
    ))
}

fn normalize_markdown_heading(heading: &str) -> String {
    let heading = heading.trim().trim_end_matches(':').trim();
    let heading = heading
        .split_once(". ")
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(_, suffix)| suffix)
        .unwrap_or(heading);

    heading.to_ascii_lowercase()
}

fn markdown_heading_matches(current: &str, expected: &str) -> bool {
    current == expected
        || (expected == "verification"
            && matches!(
                current,
                "verification steps"
                    | "verification approach"
                    | "verification strategy"
                    | "verification checks"
                    | "verification and acceptance criteria"
                    | "verification & acceptance criteria"
            ))
        || (expected == "acceptance criteria"
            && matches!(
                current,
                "verification and acceptance criteria" | "verification & acceptance criteria"
            ))
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

fn has_malformed_scope_path_segment(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(segment) = component else {
            return false;
        };
        let Some(segment) = segment.to_str() else {
            return true;
        };
        let trimmed = segment.trim();
        trimmed.is_empty()
            || trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.starts_with("├")
            || trimmed.starts_with("└")
            || trimmed.starts_with("│")
    })
}

fn verification_checks_from_items(
    verification_steps: &[String],
    acceptance_criteria: &[String],
    project_root: &Path,
) -> Vec<PlanVerificationCheck> {
    let mut checks = Vec::new();
    for item in verification_steps {
        for path in referenced_scope_paths(item, project_root) {
            let kind = if references_test_command_for_path(item, &path) {
                PlanVerificationCheckKind::TestPath { path }
            } else {
                PlanVerificationCheckKind::PathExists { path }
            };
            push_verification_check(&mut checks, kind, item);
        }

        for module in python_module_references(item) {
            push_verification_check(
                &mut checks,
                PlanVerificationCheckKind::PythonModule { module },
                item,
            );
        }
    }
    for item in acceptance_criteria {
        for module in python_module_references(item) {
            push_verification_check(
                &mut checks,
                PlanVerificationCheckKind::PythonModule { module },
                item,
            );
        }
    }
    checks
}

fn push_verification_check(
    checks: &mut Vec<PlanVerificationCheck>,
    kind: PlanVerificationCheckKind,
    source: &str,
) {
    if !checks.iter().any(|check| check.kind == kind) {
        checks.push(PlanVerificationCheck {
            kind,
            source: source.to_string(),
        });
    }
}

fn references_test_command_for_path(item: &str, path: &Path) -> bool {
    item.split_whitespace()
        .any(|token| matches!(token, "pytest" | "cargo" | "npm" | "pnpm" | "bun"))
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("test_")
                    || name.ends_with("_test.rs")
                    || name.ends_with(".test.ts")
            })
        || path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|segment| segment == "tests" || segment == "test")
        })
}

fn referenced_scope_paths(text: &str, project_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut next_token_is_module = false;
    for token in text.split_whitespace().filter_map(clean_reference_token) {
        if next_token_is_module {
            next_token_is_module = false;
            continue;
        }
        if token == "-m" {
            next_token_is_module = true;
            continue;
        }
        if !looks_like_plan_path_reference(&token) {
            continue;
        }
        if is_glob_like_reference(&token) {
            continue;
        }
        let path = PathBuf::from(token);
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) {
            continue;
        }
        let resolved = project_root.join(path);
        if !paths.contains(&resolved) {
            paths.push(resolved);
        }
    }
    paths
}

fn is_glob_like_reference(token: &str) -> bool {
    token.contains('*') || token.contains('?') || token.contains('[') || token.contains(']')
}

fn clean_reference_token(token: &str) -> Option<String> {
    let mut token = token.trim();
    loop {
        let cleaned = token
            .trim()
            .trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}'))
            .trim_end_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':'))
            .trim_matches('`')
            .trim_matches('"')
            .trim_matches('\'');
        if cleaned == token {
            break;
        }
        token = cleaned;
    }
    (!token.is_empty()).then(|| token.to_string())
}

fn looks_like_plan_path_reference(token: &str) -> bool {
    let path = Path::new(token);
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    is_hidden_plan_file_name(file_name)
        || is_well_known_extensionless_file(file_name)
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(is_recognized_plan_file_extension)
}

fn is_hidden_plan_file_name(file_name: &str) -> bool {
    file_name.starts_with('.')
        && file_name.len() > 1
        && file_name
            .chars()
            .skip(1)
            .any(|ch| ch == '_' || ch == '-' || ch.is_ascii_alphanumeric())
}

fn is_recognized_plan_file_extension(extension: &str) -> bool {
    matches!(
        extension,
        "bash"
            | "c"
            | "cc"
            | "cfg"
            | "conf"
            | "cpp"
            | "css"
            | "csv"
            | "env"
            | "go"
            | "h"
            | "hpp"
            | "html"
            | "java"
            | "js"
            | "jsx"
            | "json"
            | "lock"
            | "md"
            | "py"
            | "rs"
            | "scss"
            | "sh"
            | "sql"
            | "toml"
            | "ts"
            | "tsx"
            | "txt"
            | "yaml"
            | "yml"
            | "zsh"
    )
}

fn is_well_known_extensionless_file(file_name: &str) -> bool {
    matches!(
        file_name,
        "Dockerfile" | "Makefile" | "Procfile" | "README" | "LICENSE"
    )
}

fn python_module_references(text: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut tokens = text.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if token != "-m" {
            continue;
        }
        let Some(module) = tokens.next().and_then(clean_reference_token) else {
            continue;
        };
        if !modules.contains(&module) {
            modules.push(module);
        }
    }
    modules
}

fn is_valid_python_module_reference(module: &str) -> bool {
    module
        .split('.')
        .all(|segment| is_valid_python_module_segment(segment))
}

fn is_valid_python_module_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
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
    MalformedScopePath,
    ReferencedPathMissingFromScope,
    InvalidPythonModuleReference { module: String },
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_checks: Vec<PlanVerificationCheck>,
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanVerificationCheck {
    pub kind: PlanVerificationCheckKind,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanVerificationCheckKind {
    PathExists { path: PathBuf },
    TestPath { path: PathBuf },
    PythonModule { module: String },
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
    fn draft_contract_extracts_numbered_review_section_headings() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-numbered-review-sections-{}",
            std::process::id()
        ));
        let project = root.join("advanced-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n```text\ncli.py\n```\n\n## 2. Verification Steps\n- Run `python cli.py --help`.\n\n## 3. Acceptance Criteria\n- The CLI supports add, list, complete, and delete.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![],
            expected_files: vec![project.join("cli.py")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert_eq!(
            contract.scope.verification_steps,
            vec!["Run `python cli.py --help`."]
        );
        assert_eq!(
            contract.scope.acceptance_criteria,
            vec!["The CLI supports add, list, complete, and delete."]
        );
        assert!(contract.review_draft().is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_contract_accepts_verification_approach_heading() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-verification-approach-{}",
            std::process::id()
        ));
        let project = root.join("react-vite-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n```text\npackage.json\nsrc/main.jsx\n```\n\n## Verification Approach\n1. Run `npm run dev`.\n2. Run `npm test`.\n\n## Acceptance Criteria\n- The Vite app starts successfully.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("package.json"), project.join("src/main.jsx")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert_eq!(
            contract.scope.verification_steps,
            vec!["Run `npm run dev`.", "Run `npm test`."]
        );
        assert_eq!(
            contract.scope.acceptance_criteria,
            vec!["The Vite app starts successfully."]
        );
        assert!(contract.review_draft().is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_contract_accepts_combined_verification_acceptance_heading() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-combined-review-heading-{}",
            std::process::id()
        ));
        let project = root.join("combined-review-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n```text\nREADME.md\nsrc/main.py\n```\n\n## Verification and Acceptance Criteria\n- `README.md` explains usage.\n- `src/main.py` runs without syntax errors.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("README.md"), project.join("src/main.py")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert_eq!(
            contract.scope.verification_steps,
            vec![
                "`README.md` explains usage.",
                "`src/main.py` runs without syntax errors."
            ]
        );
        assert_eq!(
            contract.scope.acceptance_criteria,
            contract.scope.verification_steps
        );
        assert!(contract.review_draft().is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_contract_extracts_typed_verification_checks() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-typed-checks-{}",
            std::process::id()
        ));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Plan\n\n```text\ncli.py\ntests/test_cli.py\n```\n\n## Verification\n- Verify that `cli.py` can run with `python -m demo.cli`.\n- Run `pytest tests/test_cli.py`.\n\n## Acceptance Criteria\n- `cli.py` and `tests/test_cli.py` match the approved plan.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("tests")],
            expected_files: vec![project.join("cli.py"), project.join("tests/test_cli.py")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);

        assert!(contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::PathExists {
                    path: project.join("cli.py"),
                }
        }));
        assert!(contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::TestPath {
                    path: project.join("tests/test_cli.py"),
                }
        }));
        assert!(contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::PythonModule {
                    module: "demo.cli".to_string(),
                }
        }));
        assert!(contract.review_draft().is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_contract_allows_bare_filename_reference_when_scope_has_same_basename() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-bare-basename-{}",
            std::process::id()
        ));
        let project = root.join("reasoning-route-test");
        let plan_path = project.join("PROJECT_PLAN.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n```text\nreasoning_route_test/cli.py\n```\n\n## Verification\n- `cli.py` prints a greeting.\n\n## Acceptance Criteria\n- A placeholder CLI in `cli.py` prints a greeting.\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("reasoning_route_test")],
            expected_files: vec![project.join("reasoning_route_test/cli.py")],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);
        let review = contract.review_draft();

        assert!(review.is_approvable());
        assert!(!review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::ReferencedPathMissingFromScope
                && issue.path.as_ref() == Some(&project.join("cli.py"))
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_contract_ignores_module_and_prose_fragments_as_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-prose-fragments-{}",
            std::process::id()
        ));
        let project = root.join("typed-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Plan\n\n```text\ncli.py\n__init__.py\ntyped_plan_test/__main__.py\ntests/test_cli.py\nREADME.md\n```\n\n## Verification\n- `tests/test_cli.py` imports `cli.main` and verifies it can be invoked with sample arguments.\n\n## Acceptance Criteria\n- All listed files are present with non-empty content that compiles/run without errors.\n- Running `python -m typed_plan_test` executes the CLI entry point.\n- The package can be installed locally, e.g. `pip install .`).\n",
        )
        .unwrap();
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path,
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("typed_plan_test"), project.join("tests")],
            expected_files: vec![
                project.join("cli.py"),
                project.join("__init__.py"),
                project.join("typed_plan_test/__main__.py"),
                project.join("tests/test_cli.py"),
                project.join("README.md"),
            ],
        };

        let contract = PlanContract::draft_from_structured_plan("contract-1", &plan);
        let review = contract.review_draft();

        assert!(review.is_approvable());
        assert!(!contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::PathExists {
                    path: project.join("cli.main"),
                }
                || check.kind
                    == PlanVerificationCheckKind::PathExists {
                        path: project.join("compiles/run"),
                    }
                || check.kind
                    == PlanVerificationCheckKind::PathExists {
                        path: project.join("."),
                    }
        }));
        assert!(contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::TestPath {
                    path: project.join("tests/test_cli.py"),
                }
        }));
        assert!(contract.scope.verification_checks.iter().any(|check| {
            check.kind
                == PlanVerificationCheckKind::PythonModule {
                    module: "typed_plan_test".to_string(),
                }
        }));

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
            .push("src/main.py matches the approved plan".to_string());

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
                verification_checks: Vec::new(),
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
                verification_checks: Vec::new(),
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
    fn draft_review_blocks_malformed_scope_paths() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-malformed-{}",
            std::process::id()
        ));
        let project = root.join("plan-review-copy-test");
        let plan_path = project.join("PLAN.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- Ensure all listed files exist.\n\n## Acceptance Criteria\n- The project directory exists with the specified structure.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-malformed".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: vec![project.join("- tests")],
                allowed_files: vec![
                    project.join("- app.py"),
                    project.join("- tests/- test_app.py"),
                ],
                allowed_command_classes: Vec::new(),
                verification_steps: vec!["Ensure all listed files exist.".to_string()],
                verification_checks: Vec::new(),
                acceptance_criteria: vec![
                    "The project directory exists with the specified structure.".to_string(),
                ],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(!review.is_approvable());
        assert!(review.issues.iter().any(|issue| issue.kind
            == PlanContractDraftIssueKind::MalformedScopePath
            && issue.path.as_ref() == Some(&project.join("- tests"))));
        assert!(review.issues.iter().any(|issue| issue.kind
            == PlanContractDraftIssueKind::MalformedScopePath
            && issue.path.as_ref() == Some(&project.join("- tests/- test_app.py"))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_blocks_referenced_paths_missing_from_scope() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-coherence-{}",
            std::process::id()
        ));
        let project = root.join("plan-review-copy-test");
        let plan_path = project.join("PROJECT_PLAN.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- Verify that `cli.py` can be executed with `python -m plan-review-copy-test.cli` and displays help.\n- Run `pytest tests/test_cli.py` to ensure all unit tests pass.\n\n## Acceptance Criteria\n- The project contains a clear `README.md` with usage instructions.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-incoherent".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: vec![project.join("tests")],
                allowed_files: vec![project.join("README.md"), project.join("__init__.py")],
                allowed_command_classes: Vec::new(),
                verification_steps: vec![
                    "Verify that `cli.py` can be executed with `python -m plan-review-copy-test.cli` and displays help.".to_string(),
                    "Run `pytest tests/test_cli.py` to ensure all unit tests pass.".to_string(),
                ],
                verification_checks: Vec::new(),
                acceptance_criteria: vec![
                    "The project contains a clear `README.md` with usage instructions.".to_string(),
                ],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(!review.is_approvable());
        assert!(review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::ReferencedPathMissingFromScope
                && issue.path.as_ref() == Some(&project.join("cli.py"))
        }));
        assert!(review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::ReferencedPathMissingFromScope
                && issue.path.as_ref() == Some(&project.join("tests/test_cli.py"))
        }));
        assert!(review.issues.iter().any(|issue| {
            issue.kind
                == PlanContractDraftIssueKind::InvalidPythonModuleReference {
                    module: "plan-review-copy-test.cli".to_string(),
                }
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_allows_readme_stem_and_ignores_glob_references() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-glob-readme-{}",
            std::process::id()
        ));
        let project = root.join("advanced-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- README explains usage.\n- Tests are named with the `test_*.py` pattern.\n\n## Acceptance Criteria\n- README has a top-level heading.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-readme-glob".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: vec![project.join("tests")],
                allowed_files: vec![project.join("README.md"), project.join("tests/test_cli.py")],
                allowed_command_classes: Vec::new(),
                verification_steps: vec![
                    "README explains usage.".to_string(),
                    "Tests are named with the `test_*.py` pattern.".to_string(),
                ],
                verification_checks: Vec::new(),
                acceptance_criteria: vec!["README has a top-level heading.".to_string()],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(review.is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_still_blocks_real_concrete_path_mismatch() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-concrete-mismatch-{}",
            std::process::id()
        ));
        let project = root.join("advanced-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- `src/tasktracker/cli.py` contains a command parser.\n\n## Acceptance Criteria\n- `src/cli.py` exists.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-concrete-mismatch".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: vec![project.join("src")],
                allowed_files: vec![project.join("src/cli.py")],
                allowed_command_classes: Vec::new(),
                verification_steps: vec![
                    "`src/tasktracker/cli.py` contains a command parser.".to_string()
                ],
                verification_checks: Vec::new(),
                acceptance_criteria: vec!["`src/cli.py` exists.".to_string()],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(!review.is_approvable());
        assert!(review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::ReferencedPathMissingFromScope
                && issue.path.as_ref() == Some(&project.join("src/tasktracker/cli.py"))
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_allows_plan_file_self_reference() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-plan-self-reference-{}",
            std::process::id()
        ));
        let project = root.join("self-reference-plan-test");
        let plan_path = project.join("PLAN.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- `README.md` exists.\n\n## Acceptance Criteria\n- The plan file itself (`PLAN.md`) documents the structure.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-plan-self-reference".to_string(),
            source_plan_path: plan_path.clone(),
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: Vec::new(),
                allowed_files: vec![project.join("README.md")],
                allowed_command_classes: Vec::new(),
                verification_steps: vec!["`README.md` exists.".to_string()],
                verification_checks: Vec::new(),
                acceptance_criteria: vec![
                    "The plan file itself (`PLAN.md`) documents the structure.".to_string(),
                ],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(review
            .issues
            .iter()
            .all(|issue| issue.path.as_ref() != Some(&plan_path)));
        assert!(review.is_approvable());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_review_does_not_block_runtime_data_file_in_acceptance_criteria() {
        let root = std::env::temp_dir().join(format!(
            "elgar-plan-contract-review-runtime-data-reference-{}",
            std::process::id()
        ));
        let project = root.join("runtime-data-plan-test");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            &plan_path,
            "# Project Plan\n\n## Verification\n- `README.md` exists.\n\n## Acceptance Criteria\n- The app stores tasks in `tasks.json` during normal use.\n",
        )
        .unwrap();
        let contract = PlanContract {
            id: "contract-runtime-data-reference".to_string(),
            source_plan_path: plan_path,
            project_root: project.clone(),
            source_action_id: Some("action-plan".to_string()),
            status: PlanContractStatus::Draft,
            scope: PlanContractScope {
                allowed_directories: Vec::new(),
                allowed_files: vec![project.join("README.md")],
                allowed_command_classes: Vec::new(),
                verification_steps: vec!["`README.md` exists.".to_string()],
                verification_checks: Vec::new(),
                acceptance_criteria: vec![
                    "The app stores tasks in `tasks.json` during normal use.".to_string(),
                ],
                revision_reason: None,
            },
            approval: None,
        };

        let review = contract.review_draft();

        assert!(review.is_approvable());
        assert!(!review.issues.iter().any(|issue| {
            issue.kind == PlanContractDraftIssueKind::ReferencedPathMissingFromScope
                && issue.path.as_ref() == Some(&project.join("tasks.json"))
        }));

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
