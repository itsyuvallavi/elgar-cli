use elgar_core::session::{
    ProjectMemory, ProviderPromptMemoryOmittedFact, ProviderPromptMemorySelectedFact,
    ProviderPromptMemorySelection, Session, StructuredProjectPlanStatus,
};
use std::path::Path;

pub fn render_session_memory(session: &Session) -> String {
    render_memory(
        session.project_memory(),
        session.latest_provider_prompt_memory_selection(),
    )
}

fn render_memory(
    memory: &ProjectMemory,
    provider_selection: Option<&ProviderPromptMemorySelection>,
) -> String {
    let has_provider_selection = provider_selection
        .is_some_and(|selection| !selection.selected.is_empty() || !selection.omitted.is_empty());
    if memory.verified_folders.is_empty()
        && memory.verified_plans.is_empty()
        && memory.structured_plans.is_empty()
        && !has_provider_selection
    {
        return "Memory\n(empty)".to_string();
    }

    let mut lines = vec!["Memory".to_string()];

    if !memory.verified_folders.is_empty() {
        lines.push("folders".to_string());
        for reference in memory.verified_folders.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::Directory),
                reference.path.display(),
                reference.source_action_id
            ));
        }
    }

    if !memory.verified_plans.is_empty() {
        lines.push("plans".to_string());
        for reference in memory.verified_plans.iter().rev() {
            lines.push(format!(
                "- {} {} ({})",
                path_state(&reference.path, PathKind::File),
                reference.path.display(),
                reference.source_action_id
            ));
            lines.push(format!(
                "  root {} {}",
                path_state(&reference.project_root, PathKind::Directory),
                reference.project_root.display()
            ));
        }
    }

    if !memory.structured_plans.is_empty() {
        lines.push("structured plans".to_string());
        for plan in memory.structured_plans.iter().rev() {
            let action = plan.source_action_id.as_deref().unwrap_or("unknown-action");
            lines.push(format!(
                "- {} {} ({})",
                structured_status(plan.status),
                plan.stage,
                action
            ));
            lines.push(format!(
                "  plan {} {}",
                path_state(&plan.source_plan_path, PathKind::File),
                plan.source_plan_path.display()
            ));
            lines.push(format!(
                "  root {} {}",
                path_state(&plan.project_root, PathKind::Directory),
                plan.project_root.display()
            ));
            if !plan.expected_directories.is_empty() {
                lines.push(format!(
                    "  dirs {}",
                    path_count(&plan.expected_directories, PathKind::Directory)
                ));
            }
            if !plan.expected_files.is_empty() {
                lines.push(format!(
                    "  files {}",
                    path_count(&plan.expected_files, PathKind::File)
                ));
            }
        }
    }

    if let Some(selection) = provider_selection {
        render_provider_prompt_memory_selection(selection, &mut lines);
    }

    lines.join("\n")
}

#[derive(Debug, Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

fn path_state(path: &Path, kind: PathKind) -> &'static str {
    match kind {
        PathKind::Directory if path.is_dir() => "ok",
        PathKind::File if path.is_file() => "ok",
        _ => "missing",
    }
}

fn path_count(paths: &[std::path::PathBuf], kind: PathKind) -> String {
    let present = paths
        .iter()
        .filter(|path| match kind {
            PathKind::Directory => path.is_dir(),
            PathKind::File => path.is_file(),
        })
        .count();
    format!("{present}/{}", paths.len())
}

fn structured_status(status: StructuredProjectPlanStatus) -> &'static str {
    match status {
        StructuredProjectPlanStatus::Proposed => "proposed",
        StructuredProjectPlanStatus::Executed => "executed",
    }
}

fn render_provider_prompt_memory_selection(
    selection: &ProviderPromptMemorySelection,
    lines: &mut Vec<String>,
) {
    if selection.selected.is_empty() && selection.omitted.is_empty() {
        return;
    }

    lines.push("provider prompt memory".to_string());
    if !selection.selected.is_empty() {
        lines.push("selected".to_string());
        for fact in &selection.selected {
            render_selected_provider_prompt_memory_fact(fact, lines);
        }
    }
    if !selection.omitted.is_empty() {
        lines.push("omitted".to_string());
        for fact in &selection.omitted {
            render_omitted_provider_prompt_memory_fact(fact, lines);
        }
    }
}

fn render_selected_provider_prompt_memory_fact(
    fact: &ProviderPromptMemorySelectedFact,
    lines: &mut Vec<String>,
) {
    lines.push(format!(
        "- {} {} {} ({})",
        provider_memory_kind_label(&fact.kind),
        provider_memory_path_state(&fact.kind, &fact.path),
        fact.path.display(),
        fact.source_action_id
    ));
    if let Some(project_root) = fact.project_root.as_ref() {
        lines.push(format!(
            "  root {} {}",
            path_state(project_root, PathKind::Directory),
            project_root.display()
        ));
    }
}

fn render_omitted_provider_prompt_memory_fact(
    fact: &ProviderPromptMemoryOmittedFact,
    lines: &mut Vec<String>,
) {
    lines.push(format!(
        "- {} {} {} ({}; {})",
        provider_memory_kind_label(&fact.kind),
        provider_memory_path_state(&fact.kind, &fact.path),
        fact.path.display(),
        fact.source_action_id,
        fact.reason
    ));
    if let Some(project_root) = fact.project_root.as_ref() {
        lines.push(format!(
            "  root {} {}",
            path_state(project_root, PathKind::Directory),
            project_root.display()
        ));
    }
}

fn provider_memory_kind_label(kind: &str) -> String {
    match kind {
        "verified_folder" => "verified folder".to_string(),
        "verified_plan" => "verified plan".to_string(),
        "structured_plan" => "structured plan".to_string(),
        other => other.replace('_', " "),
    }
}

fn provider_memory_path_state(kind: &str, path: &Path) -> &'static str {
    let path_kind = match kind {
        "verified_folder" => PathKind::Directory,
        _ => PathKind::File,
    };
    path_state(path, path_kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgar_core::controller::Controller;
    use std::fs;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("elgar-memory-render-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn renders_empty_memory_compactly() {
        let session = Session::new("memory-empty", "/repo", "/repo");

        assert_eq!(render_session_memory(&session), "Memory\n(empty)");
        assert!(!render_session_memory(&session).contains("provider prompt memory"));
    }

    #[test]
    fn renders_verified_and_stale_memory_without_provider_calls() {
        let root = temp_root("verified-stale");
        let folder = root.join("project");
        let controller = Controller::default();
        let mut session = Session::new("memory-session", &root, &root);

        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
        controller.turn(
            &mut session,
            "create a plan for a small python project inside that folder",
        );
        controller.turn(&mut session, "approve");
        controller.turn(&mut session, "execute the plan inside that folder");
        controller.turn(&mut session, "approve");

        let plan_path = folder.join("small-python-project-plan.md");
        assert!(plan_path.is_file());

        let rendered = render_session_memory(&session);
        assert!(rendered.contains("folders\n- ok "));
        assert!(rendered.contains("plans\n- ok "));
        assert!(rendered.contains("structured plans\n- executed"));
        assert!(rendered.contains("dirs 2/2"));
        assert!(rendered.contains("files 5/5"));

        fs::remove_file(&plan_path).unwrap();
        let rendered = render_session_memory(&session);
        assert!(rendered.contains("- missing "));
        assert!(rendered.contains("plan missing "));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_latest_provider_prompt_selected_memory_trace() {
        let root = temp_root("provider-selected");
        let folder = root.join("project");
        let controller = Controller::default();
        let mut session = Session::new("memory-session", &root, &root);

        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
        controller.turn(
            &mut session,
            "create a plan for a small python project inside that folder",
        );
        controller.turn(&mut session, "approve");

        controller.model_turn(&mut session, "what path is the plan for that project?");

        let rendered = render_session_memory(&session);
        assert!(rendered.contains("provider prompt memory\nselected"));
        assert!(rendered.contains("verified folder ok "));
        assert!(rendered.contains("verified plan ok "));
        assert!(rendered.contains("root ok "));
        assert!(!rendered.contains("Verified memory selected by Elgar controller:"));
        assert!(!rendered.contains("User request:"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renders_latest_provider_prompt_omitted_memory_trace() {
        let root = temp_root("provider-omitted");
        let folder = root.join("project");
        let controller = Controller::default();
        let mut session = Session::new("memory-session", &root, &root);

        controller.turn(
            &mut session,
            &format!("create a folder at {}", folder.display()),
        );
        controller.turn(&mut session, "approve");
        controller.turn(
            &mut session,
            "create a plan for a small python project inside that folder",
        );
        controller.turn(&mut session, "approve");
        fs::remove_dir_all(&folder).unwrap();

        controller.model_turn(&mut session, "what path is the plan for that project?");

        let rendered = render_session_memory(&session);
        assert!(rendered.contains("provider prompt memory\nomitted"));
        assert!(rendered.contains("verified folder missing "));
        assert!(rendered.contains("verified plan missing "));
        assert!(rendered.contains("; missing)"));
        assert!(!rendered.contains("Verified memory selected by Elgar controller:"));
        assert!(!rendered.contains("User request:"));

        let _ = fs::remove_dir_all(root);
    }
}
