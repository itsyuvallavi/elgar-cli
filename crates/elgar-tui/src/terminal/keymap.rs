#[cfg(test)]
use std::io;
use std::io::Write;

#[cfg(test)]
use elgar_core::controller::Controller;
use elgar_core::{
    action_gate::ActionGate, agent_runtime::AgentRuntime, provider::ControllerProvider,
    router::normalize_pasted_transcript_input, session::Session,
};

#[cfg(test)]
use crate::input::{TerminalInput, TerminalInputAction};
#[cfg(test)]
use crate::terminal::provider_task::{start_provider_text_turn, start_tool_turn, ProviderTurnTask};
use crate::{
    memory::{
        render_session_created_actions, render_session_memory, render_session_pending_action,
        render_session_plan_preview, render_session_state_snapshot, render_session_status,
    },
    terminal::commands::{
        clear_terminal_conversation, copy_conversation_to_terminal_clipboard,
        copy_raw_details_to_terminal_clipboard, parse_terminal_command, render_terminal_help,
        render_tool_usage, render_unknown_command, TerminalCommand,
    },
    TuiShell,
};

#[cfg(test)]
pub(crate) fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    key.modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL)
        && matches!(
            key.code,
            crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('d')
        )
}

#[cfg(test)]
pub(crate) fn handle_terminal_key<P>(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> bool
where
    P: ControllerProvider + Clone,
{
    handle_terminal_key_with_copy_writer(key, input, controller, session, shell, io::stdout())
}

#[cfg(test)]
pub(crate) fn handle_terminal_key_with_copy_writer<P>(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    copy_writer: impl Write,
) -> bool
where
    P: ControllerProvider + Clone,
{
    if should_exit(key) {
        return true;
    }

    if handle_scroll_key(key, shell) {
        return false;
    }

    match input.handle_key(key) {
        TerminalInputAction::Continue => false,
        TerminalInputAction::Exit => true,
        TerminalInputAction::Submit => {
            let submitted = input.drain();
            shell.input.text.clear();
            let runtime = AgentRuntime::new(controller.provider.clone());
            let action_gate = ActionGate::new(controller.provider.clone());
            handle_submitted_terminal_input(
                &submitted,
                &runtime,
                &action_gate,
                session,
                shell,
                copy_writer,
            )
        }
    }
}

pub(super) fn handle_submitted_terminal_input<P>(
    submitted: &str,
    runtime: &AgentRuntime<P>,
    action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    copy_writer: impl Write,
) -> bool
where
    P: ControllerProvider,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Empty => {}
        TerminalCommand::Help => {
            shell
                .conversation
                .lines
                .push(render_terminal_help().to_string());
            shell.conversation.follow_latest();
        }
        TerminalCommand::Clear => {
            clear_terminal_conversation(shell);
        }
        TerminalCommand::Approve => {
            shell.submit_approval(action_gate, session);
        }
        TerminalCommand::Reject => {
            shell.submit_rejection(action_gate, session);
        }
        TerminalCommand::Cancel => {
            shell
                .conversation
                .push_local_message("No provider request is running.");
            shell.conversation.follow_latest();
        }
        TerminalCommand::Memory => {
            shell
                .conversation
                .push_local_message(render_session_memory(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::State => {
            shell
                .conversation
                .push_local_message(render_session_state_snapshot(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::PlanPreview => {
            shell
                .conversation
                .push_local_message(render_session_plan_preview(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Reasoning => {
            shell
                .conversation
                .push_local_message(crate::render_session_reasoning(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::DetailsLast => {
            shell.push_latest_raw_details();
        }
        TerminalCommand::Status => {
            shell
                .conversation
                .push_local_message(render_session_status(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Tokens => {
            shell
                .conversation
                .push_local_message(crate::render_session_tokens(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Pending => {
            shell
                .conversation
                .push_local_message(render_session_pending_action(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Created => {
            shell
                .conversation
                .push_local_message(render_session_created_actions(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Permissions(argument) => {
            let message = shell.apply_permission_command(argument);
            shell.push_local_message(message);
        }
        TerminalCommand::Tool(text) => {
            handle_terminal_tool_input(text, runtime, action_gate, session, shell);
        }
        TerminalCommand::Copy => {
            let _ = copy_conversation_to_terminal_clipboard(copy_writer, shell);
        }
        TerminalCommand::CopyRaw => {
            let _ = copy_raw_details_to_terminal_clipboard(copy_writer, shell);
        }
        TerminalCommand::Exit => return true,
        TerminalCommand::Unknown(command) => {
            shell.push_local_message(render_unknown_command(command));
        }
        TerminalCommand::Text(text) => {
            handle_terminal_text_input(text, runtime, action_gate, session, shell);
        }
    }
    false
}

pub(super) fn handle_terminal_text_input<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    _action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) where
    P: ControllerProvider,
{
    shell.submit_agent_input(runtime, session, text);
}

pub(super) fn handle_terminal_tool_input<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    _action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) where
    P: ControllerProvider,
{
    if text.trim().is_empty() {
        shell.push_local_message(render_tool_usage());
        return;
    }
    shell.submit_agent_tool_input(runtime, session, text);
}

pub(super) fn terminal_text_should_run_inline_provider_text(_text: &str) -> bool {
    true
}

pub(super) fn normalize_terminal_provider_text_input(text: &str) -> String {
    normalize_pasted_transcript_input(text).trim().to_string()
}

#[cfg(test)]
pub(crate) fn handle_submitted_terminal_input_for_loop<P>(
    submitted: &str,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    pending_turn: &mut Option<ProviderTurnTask>,
) -> bool
where
    P: ControllerProvider + Clone + Send + 'static,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Cancel => {
            if let Some(task) = pending_turn.take() {
                task.cancel();
                shell.conversation.discard_pending_provider_turn();
                shell
                    .conversation
                    .push_local_message("Provider request canceled.");
                shell.conversation.follow_latest();
                shell.status.cancel_provider_turn();
            } else {
                shell
                    .conversation
                    .push_local_message("No provider request is running.");
                shell.conversation.follow_latest();
            }
            false
        }
        TerminalCommand::Text(text) if terminal_text_should_run_inline_provider_text(text) => {
            let runtime = AgentRuntime::new(controller.provider.clone());
            let provider_input = normalize_terminal_provider_text_input(text);
            shell
                .conversation
                .push_pending_provider_turn(&provider_input);
            shell.conversation.follow_latest();
            shell.status.start_thinking_pulse();
            *pending_turn = Some(start_provider_text_turn(
                runtime,
                session.clone(),
                provider_input,
                shell.policy_mode,
            ));
            false
        }
        TerminalCommand::Tool(text) => {
            if text.trim().is_empty() {
                shell.push_local_message(render_tool_usage());
                return false;
            }
            let runtime = AgentRuntime::new(controller.provider.clone());
            shell
                .conversation
                .push_pending_provider_turn(&format!("/tool {text}"));
            shell.conversation.follow_latest();
            shell.status.start_thinking_pulse();
            *pending_turn = Some(start_tool_turn(
                runtime,
                session.clone(),
                text.to_string(),
                shell.policy_mode,
            ));
            false
        }
        _ => {
            let runtime = AgentRuntime::new(controller.provider.clone());
            let action_gate = ActionGate::new(controller.provider.clone());
            handle_submitted_terminal_input(
                submitted,
                &runtime,
                &action_gate,
                session,
                shell,
                io::stdout(),
            )
        }
    }
}

#[cfg(test)]
pub(crate) fn handle_scroll_key(key: crossterm::event::KeyEvent, shell: &mut TuiShell) -> bool {
    match key.code {
        crossterm::event::KeyCode::PageUp => {
            shell.conversation.scroll_up(5);
            true
        }
        crossterm::event::KeyCode::PageDown => {
            shell.conversation.scroll_down(5);
            true
        }
        crossterm::event::KeyCode::End
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            shell.conversation.follow_latest();
            true
        }
        _ => false,
    }
}
