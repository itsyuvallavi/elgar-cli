pub mod action;
pub mod action_gate;
pub mod agent_loop;
pub mod agent_runtime;
pub mod context;
pub mod controller;
mod controller_project_memory;
mod controller_provider;
mod controller_reporting;
mod controller_shell_verify;
pub mod event;
pub mod fs;
pub mod model_runtime;
mod path_resolution;
pub mod policy;
pub mod provider;
mod provider_visible;
pub use provider_visible::provider_visible_text_from_text_only_output;
pub mod renderer;
pub mod router;
pub mod session;
pub mod shell;
mod verified_state_answer;
#[cfg(test)]
mod test_env;

pub const CORE_PHILOSOPHY: &str = "Model reasons. Runtime routes. Action gate enforces. Filesystem confirms. UI reports. Tests protect. Extensions wait.";
