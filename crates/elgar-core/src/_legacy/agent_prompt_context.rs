use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

use crate::{
    context::ContextBundle,
    event::{Event, VerifiedActionResult},
    session::{
        ProviderPromptMemorySelectedFact, ProviderPromptMemorySelection, Session,
        VerifiedFolderReference,
    },
    session_log_memory::{latest_durable_verified_artifacts, DurableVerifiedArtifactFact},
    verified_artifact_memory::{
        earliest_verified_artifacts, latest_action_turn_artifacts, latest_verified_artifacts,
        verified_artifacts_under_folder, CappedVerifiedArtifacts, VerifiedArtifactFact,
    },
};

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentVerifiedMemoryContext {
    pub(crate) prompt_context: Option<String>,
}

pub(crate) fn agent_local_runtime_context(session: &mut Session) -> Option<String> {
    let project_root = session.project_root.clone();
    let cwd = session.cwd.clone();
    let max_window_tokens = session.context_accounting().max_window_tokens;
    let bundle = ContextBundle::from_default_local_files(project_root, cwd, max_window_tokens);
    session.set_context_accounting(bundle.accounting.clone());
    let cwd_relative = session
        .cwd
        .strip_prefix(&session.project_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    let runtime_context = format!(
        "Elgar runtime session:\n- project_root: {}\n- cwd: {}\n- cwd_relative_to_project_root: {}\n- current/root/this folder/project refers to cwd; use cwd `.` for shell commands targeting it.",
        session.project_root.display(),
        session.cwd.display(),
        cwd_relative
    );

    Some(match bundle.system_context() {
        Some(context) => format!("{runtime_context}\n\n{context}"),
        None => runtime_context,
    })
}

pub(crate) fn agent_route_location_context(session: &Session) -> String {
    let cwd_relative = session
        .cwd
        .strip_prefix(&session.project_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string());

    format!(
        "Runtime location: project_root={} cwd={} cwd_relative={}. Current/root/this folder/project means cwd.",
        session.project_root.display(),
        session.cwd.display(),
        cwd_relative
    )
}

pub(crate) fn agent_verified_memory_context(
    session: &mut Session,
    include_durable: bool,
) -> AgentVerifiedMemoryContext {
    let mut selected = Vec::new();
    let mut lines = Vec::new();
    let latest_folder = latest_verified_folder_for_prompt(session).cloned();
    if let Some(folder) = latest_folder.as_ref() {
        lines.push(format!(
            "- latest verified folder: {}",
            display_agent_context_path(session, &folder.path)
        ));
        selected.push(ProviderPromptMemorySelectedFact::new(
            "verified_folder",
            folder.path.clone(),
            None,
            folder.source_action_id.clone(),
        ));
    }
    if let Some(plan) = session.project_memory().latest_verified_plan() {
        lines.push(format!(
            "- latest verified plan: {}",
            display_agent_context_path(session, &plan.path)
        ));
        if let Some(excerpt) = verified_plan_excerpt(&plan.path) {
            lines.push(format!("- latest verified plan excerpt:\n{excerpt}"));
        }
        selected.push(ProviderPromptMemorySelectedFact::new(
            "verified_plan",
            plan.path.clone(),
            Some(plan.project_root.clone()),
            plan.source_action_id.clone(),
        ));
    }
    if let Some(plan) = session.project_memory().latest_structured_plan() {
        lines.push(format!(
            "- latest structured plan root: {}",
            display_agent_context_path(session, &plan.project_root)
        ));
        let missing_directories = plan
            .expected_directories
            .iter()
            .filter(|path| !path.is_dir())
            .map(|path| display_agent_context_path(session, path))
            .collect::<Vec<_>>();
        let missing_files = plan
            .expected_files
            .iter()
            .filter(|path| !path.is_file())
            .map(|path| display_agent_context_path(session, path))
            .collect::<Vec<_>>();
        if !missing_directories.is_empty() {
            lines.push(format!(
                "- missing expected directories:\n{}",
                missing_directories
                    .iter()
                    .map(|path| format!("  - {path}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !missing_files.is_empty() {
            lines.push(format!(
                "- missing expected files:\n{}",
                missing_files
                    .iter()
                    .map(|path| format!("  - {path}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !missing_directories.is_empty() || !missing_files.is_empty() {
            lines.push(
                "- when applying this incomplete structured plan, create all missing expected paths in one tool response when possible"
                    .to_string(),
            );
        }
        if missing_directories.is_empty() && missing_files.is_empty() {
            lines.push("- latest structured plan expected paths are complete".to_string());
            if !plan.expected_files.is_empty() {
                lines.push(format!(
                    "- completed structured plan expected files:\n{}",
                    plan.expected_files
                        .iter()
                        .take(12)
                        .map(|path| format!("  - {}", display_agent_context_path(session, path)))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            lines.push(
                "- completed structured plan files are still editable when the user requests changes; do not refuse edits solely because a path is part of a completed plan; runtime validation and policy decide whether the tool call is allowed".to_string(),
            );
        }
        selected.push(ProviderPromptMemorySelectedFact::new(
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            plan.source_action_id.clone().unwrap_or_default(),
        ));
    }
    append_verified_artifact_memory_context(
        session,
        latest_folder.as_ref().map(|folder| folder.path.as_path()),
        include_durable,
        &mut lines,
        &mut selected,
    );

    if selected.is_empty() {
        session.set_latest_provider_prompt_memory_selection(None);
    } else {
        session.set_latest_provider_prompt_memory_selection(Some(
            ProviderPromptMemorySelection::new(selected, Vec::new()),
        ));
    }

    let prompt_context = if lines.is_empty() {
        None
    } else {
        let mut context = vec![
            "Verified filesystem context for this session:".to_string(),
            "Use these verified paths only when the current user turn refers to prior work."
                .to_string(),
            "Displayed paths are relative to the current working directory when possible."
                .to_string(),
        ];
        context.extend(lines);
        Some(context.join("\n"))
    };

    AgentVerifiedMemoryContext { prompt_context }
}

const VERIFIED_ARTIFACT_LATEST_TURN_LIMIT: usize = 4;
const VERIFIED_ARTIFACT_LATEST_LIMIT: usize = 6;
const VERIFIED_ARTIFACT_EARLIEST_LIMIT: usize = 3;
const VERIFIED_ARTIFACT_FOLDER_LIMIT: usize = 4;
const DURABLE_VERIFIED_ARTIFACT_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VerifiedArtifactPromptKey {
    action_id: String,
    path: PathBuf,
}

impl VerifiedArtifactPromptKey {
    fn from_artifact(artifact: &VerifiedArtifactFact) -> Self {
        Self {
            action_id: artifact.action_id.clone(),
            path: artifact.path.clone(),
        }
    }
}

fn append_verified_artifact_memory_context(
    session: &Session,
    latest_folder: Option<&Path>,
    include_durable: bool,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
) {
    let latest_turn = latest_action_turn_artifacts(session, VERIFIED_ARTIFACT_LATEST_TURN_LIMIT);
    let latest = latest_verified_artifacts(session, VERIFIED_ARTIFACT_LATEST_LIMIT);
    let earliest = earliest_verified_artifacts(session, VERIFIED_ARTIFACT_EARLIEST_LIMIT);
    let under_latest_folder = latest_folder
        .map(|folder| {
            (
                folder,
                verified_artifacts_under_folder(session, folder, VERIFIED_ARTIFACT_FOLDER_LIMIT),
            )
        })
        .filter(|(_folder, artifacts)| !artifacts.artifacts.is_empty());

    if latest_turn.artifacts.is_empty()
        && latest.artifacts.is_empty()
        && earliest.artifacts.is_empty()
        && under_latest_folder.is_none()
    {
        if include_durable {
            append_durable_verified_artifact_memory_context(
                session,
                lines,
                selected,
                &HashSet::new(),
            );
        }
        return;
    }

    lines.push("- verified artifacts from prior actions:".to_string());
    let mut emitted = HashSet::new();
    append_artifact_group(
        session,
        lines,
        selected,
        "latest action turn",
        &latest_turn,
        &mut emitted,
    );
    append_artifact_group(
        session,
        lines,
        selected,
        "latest session artifacts",
        &latest,
        &mut emitted,
    );
    append_artifact_group(
        session,
        lines,
        selected,
        "earliest session artifacts",
        &earliest,
        &mut emitted,
    );
    if let Some((folder, artifacts)) = under_latest_folder {
        append_artifact_group(
            session,
            lines,
            selected,
            &format!(
                "artifacts under latest folder {}",
                display_agent_context_path(session, folder)
            ),
            &artifacts,
            &mut emitted,
        );
    }
    if include_durable {
        append_durable_verified_artifact_memory_context(session, lines, selected, &emitted);
    }
}

fn append_artifact_group(
    session: &Session,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
    label: &str,
    artifacts: &CappedVerifiedArtifacts,
    emitted: &mut HashSet<VerifiedArtifactPromptKey>,
) {
    if artifacts.artifacts.is_empty() {
        return;
    }

    let artifacts_to_emit = artifacts
        .artifacts
        .iter()
        .filter(|artifact| emitted.insert(VerifiedArtifactPromptKey::from_artifact(artifact)))
        .collect::<Vec<_>>();
    if artifacts_to_emit.is_empty() {
        return;
    }

    lines.push(format!("  - {label}:"));
    for artifact in artifacts_to_emit {
        lines.push(format!(
            "    - {}",
            verified_artifact_context_line(session, artifact)
        ));
        selected.push(ProviderPromptMemorySelectedFact::new(
            "verified_artifact",
            artifact.path.clone(),
            artifact.project_root.clone(),
            artifact.action_id.clone(),
        ));
    }
    if artifacts.omitted_count > 0 {
        lines.push(format!(
            "    - omitted {} older verified artifact(s) due to prompt cap",
            artifacts.omitted_count
        ));
    }
}

fn verified_artifact_context_line(session: &Session, artifact: &VerifiedArtifactFact) -> String {
    let mut line = format!(
        "{} turn {} {} {}",
        artifact.action_id,
        artifact.turn_index,
        artifact.operation,
        display_agent_context_path(session, &artifact.path)
    );
    if let Some(source_path) = artifact.source_path.as_ref() {
        line.push_str(&format!(
            " from {}",
            display_agent_context_path(session, source_path)
        ));
    }
    if let Some(project_root) = artifact.project_root.as_ref() {
        line.push_str(&format!(
            " under {}",
            display_agent_context_path(session, project_root)
        ));
    }
    line
}

fn append_durable_verified_artifact_memory_context(
    session: &Session,
    lines: &mut Vec<String>,
    selected: &mut Vec<ProviderPromptMemorySelectedFact>,
    in_memory_artifacts: &HashSet<VerifiedArtifactPromptKey>,
) {
    let durable = latest_durable_verified_artifacts(session, DURABLE_VERIFIED_ARTIFACT_LIMIT);
    let artifacts = durable
        .artifacts
        .iter()
        .filter(|artifact| {
            !in_memory_artifacts.contains(&VerifiedArtifactPromptKey {
                action_id: artifact.action_id.clone(),
                path: artifact.path.clone(),
            })
        })
        .collect::<Vec<_>>();
    if artifacts.is_empty() {
        return;
    }

    lines.push("- durable verified artifacts from local session logs:".to_string());
    for artifact in artifacts {
        lines.push(format!(
            "  - {}",
            durable_verified_artifact_context_line(session, artifact)
        ));
        selected.push(ProviderPromptMemorySelectedFact::new(
            "durable_verified_artifact",
            artifact.path.clone(),
            artifact.project_root.clone(),
            format!("{}:{}", artifact.session_id, artifact.action_id),
        ));
    }
    if durable.omitted_count > 0 {
        lines.push(format!(
            "  - omitted {} older durable verified artifact(s) due to prompt cap",
            durable.omitted_count
        ));
    }
}

fn durable_verified_artifact_context_line(
    session: &Session,
    artifact: &DurableVerifiedArtifactFact,
) -> String {
    let mut line = format!(
        "{}:{} turn {} {} {}",
        artifact.session_id,
        artifact.action_id,
        artifact.turn_index,
        artifact.operation,
        display_agent_context_path(session, &artifact.path)
    );
    if let Some(source_path) = artifact.source_path.as_ref() {
        line.push_str(&format!(
            " from {}",
            display_agent_context_path(session, source_path)
        ));
    }
    if let Some(project_root) = artifact.project_root.as_ref() {
        line.push_str(&format!(
            " under {}",
            display_agent_context_path(session, project_root)
        ));
    }
    line
}

pub(crate) fn latest_verified_folder_for_prompt(
    session: &Session,
) -> Option<&VerifiedFolderReference> {
    let folders = &session.project_memory().verified_folders;
    let latest = folders.last()?;

    folders
        .iter()
        .rev()
        .skip(1)
        .find(|candidate| {
            latest.path != candidate.path && path_is_within(&latest.path, &candidate.path)
        })
        .or(Some(latest))
}

pub(crate) fn agent_recent_conversation_context(
    session: &Session,
    end_index: usize,
) -> Option<String> {
    let mut lines = Vec::new();
    for event in session.events()[..end_index].iter().rev() {
        match event {
            Event::UserMessage(message) => {
                lines.push(format!("User: {}", compact_context_line(&message.content)));
            }
            Event::AssistantMessage(message) => {
                lines.push(format!("Elgar: {}", compact_context_line(&message.content)));
            }
            Event::ActionApplied(applied) => {
                lines.push(format!(
                    "Verified action: {}",
                    compact_context_line(&verified_result_context(&applied.result))
                ));
            }
            Event::ActionFailed(failed) => {
                lines.push(format!(
                    "Failed action: {}",
                    compact_context_line(&failed.reason)
                ));
            }
            _ => {}
        }

        if lines.len() >= 12 {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    Some(format!(
        "Recent conversation context for the explicit tool request:\n{}",
        lines.join("\n")
    ))
}

fn verified_result_context(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => format!("wrote {path}"),
        VerifiedActionResult::File(verification) => match verification {
            crate::event::FileActionVerification::FileCreated { path } => {
                format!("created file {path}")
            }
            crate::event::FileActionVerification::FilePatched { path } => {
                format!("patched file {path}")
            }
            crate::event::FileActionVerification::FileOverwritten { path } => {
                format!("overwrote file {path}")
            }
            crate::event::FileActionVerification::FileDeleted { path } => {
                format!("deleted file {path}")
            }
            crate::event::FileActionVerification::FileMoved {
                source_path,
                target_path,
            } => format!("moved file {source_path} to {target_path}"),
            crate::event::FileActionVerification::DirectoryCreated { path } => {
                format!("created directory {path}")
            }
        },
        VerifiedActionResult::Shell(verification) => verification
            .verified_effect
            .clone()
            .unwrap_or_else(|| format!("shell command finished in {}", verification.cwd)),
    }
}

fn compact_context_line(value: &str) -> String {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 260;
    truncate_utf8(&line, LIMIT)
}

fn verified_plan_excerpt(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let contents = contents.trim();
    if contents.is_empty() {
        return None;
    }

    const LIMIT: usize = 1200;
    Some(truncate_utf8(contents, LIMIT))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let suffix = "...";
    let max_content = max_bytes.saturating_sub(suffix.len());
    let mut end = max_content.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], suffix)
}

pub(crate) fn display_agent_context_path(session: &Session, path: &Path) -> String {
    let display_path = path
        .strip_prefix(&session.cwd)
        .or_else(|_| path.strip_prefix(&session.project_root))
        .unwrap_or(path);
    if display_path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        display_path.display().to_string()
    }
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    normalize_path(path).starts_with(normalize_path(root))
}

fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_context_line_truncates_unicode_at_char_boundary() {
        let input = format!("{} {}", "plan", "│".repeat(200));
        let line = compact_context_line(&input);

        assert!(line.ends_with("..."));
        assert!(line.is_char_boundary(line.len()));
    }

    #[test]
    fn verified_plan_excerpt_truncates_unicode_at_char_boundary() {
        let root = std::env::temp_dir().join(format!(
            "elgar-agent-loop-{}-unicode-plan",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let plan = root.join("plan.md");
        std::fs::write(&plan, format!("# Plan\n\n{}\n", "├─ src/│".repeat(300))).unwrap();

        let excerpt = verified_plan_excerpt(&plan).unwrap();

        assert!(excerpt.ends_with("..."));
        assert!(excerpt.is_char_boundary(excerpt.len()));

        let _ = std::fs::remove_dir_all(&root);
    }
}
