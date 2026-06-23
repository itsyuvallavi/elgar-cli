//! Model-choice contract rendering tests.

use crate::harness::{
    loop_decision_contract, model_choice_contract, PrimitiveToolId, PrimitiveToolRegistry,
};

#[test]
fn loop_contract_encourages_only_independent_batches() {
    let contract = crate::harness::loop_decision_contract(&PrimitiveToolRegistry::stage_3a());

    assert!(contract.contains("multiple independent"));
    assert!(contract.contains("already clearly needs"));
    assert!(contract.contains("Do not batch speculative"));
}

#[test]
fn loop_contract_prefers_user_named_paths() {
    let contract = crate::harness::loop_decision_contract(&PrimitiveToolRegistry::stage_3a());

    assert!(contract.contains("When the user names a path"));
    assert!(contract.contains("For `list <dir>`, request `ls` on that directory"));
    assert!(contract.contains("For `read <dir>`"));
    assert!(contract.contains("Prefer the user-named path over `.`"));
    assert!(contract.contains("`find` pattern such as `README*`"));
}

#[test]
fn model_choice_contract_renders_enabled_stage_3a_tools() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let contract = model_choice_contract(&registry);

    assert!(contract.contains("`read`"));
    assert!(contract.contains("`ls`"));
    assert!(contract.contains("`find`"));
    assert!(contract.contains("`grep`"));
    assert!(contract.contains("`bash`"));
    assert!(contract.contains("`write`"));
    assert!(contract.contains("`edit`"));
    assert!(contract.contains("Available primitive tools"));
}

#[test]
fn loop_contract_guides_broad_requests_without_macro_tools() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let contract = loop_decision_contract(&registry);

    assert!(contract.contains("For broad requests"));
    assert!(contract.contains("gather enough verified evidence"));
    assert!(contract.contains("Do not answer from only a directory listing"));
    assert!(contract.contains("evidence_depth"));
    assert!(contract.contains("If evidence is insufficient"));
    assert!(!contract.contains("review_project"));
    assert!(!contract.contains("inspect_project"));
    assert!(!contract.contains("package.json"));
    assert!(!contract.contains("app/page.tsx"));
}

#[test]
fn stage_3a_executable_tools_are_read_only_primitives() {
    let registry = PrimitiveToolRegistry::stage_3a();
    let executable = registry
        .tools()
        .iter()
        .filter(|tool| tool.executable_in_stage)
        .map(|tool| tool.id)
        .collect::<Vec<_>>();

    assert_eq!(
        executable,
        vec![
            PrimitiveToolId::Read,
            PrimitiveToolId::Ls,
            PrimitiveToolId::Find,
            PrimitiveToolId::Grep,
            PrimitiveToolId::McpCall,
        ]
    );
}
