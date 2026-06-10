//! Runs one submitted provider turn from the terminal.
//!
//! This file owns the live provider-turn loop: start the background task,
//! redraw progress, accept `/cancel`, and apply completed events to `TuiShell`.

use std::{
    io,
    time::{Duration, Instant},
};

use crossterm::event;
use elgar_core::{
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ControllerProvider,
    session::Session,
};

use crate::{
    input::TerminalInput,
    terminal::{
        display_context::terminal_context,
        input::raw_mode::TerminalModeGuard,
        turn::{
            active::handle_active_provider_event,
            provider_worker::{start_harness_turn, ProviderTurnUpdate},
        },
        ui::{
            approval::print_pending_approval,
            prompt::{InlineWorkingRenderer, LiveProviderOutput},
            render::{
                print_new_conversation_lines, print_plain_block, print_spacer, print_user_block,
            },
        },
        IDLE_RENDER_INTERVAL,
    },
    turn_metrics::{aggregate_provider_token_usage, duration_millis},
    TuiShell,
};

/// Runs normal plain text through the harness after TUI input normalization.
pub(super) fn run_inline_provider_text_turn<P>(
    text: &str,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<String>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    run_inline_provider_turn(text, provider, session, shell)
}

/// Runs one harness-controlled provider turn while keeping the terminal responsive.
fn run_inline_provider_turn<P>(
    text: &str,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<String>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let turn_started = Instant::now();
    let turn_id = session.next_turn_id();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Tui,
            file!(),
            "run_inline_provider_turn",
            "tui_provider_turn_started",
        )
        .with_metadata(serde_json::json!({
            "input_chars": text.chars().count(),
            "turn_kind": "harness"
        })),
    );
    let before = shell.conversation.render_lines_with_styles().len();
    print_spacer()?;
    print_user_block(text)?;

    let task = start_harness_turn(provider.clone(), session.clone(), text.to_string());
    let guard = TerminalModeGuard::enter()?;
    let mut working = InlineWorkingRenderer::new(terminal_context(session, provider));
    let mut input = TerminalInput::default();
    let mut live_output = LiveProviderOutput::default();
    live_output.suppress_response_preview();
    let mut tick = 0usize;
    working.render_with_cursor(
        tick,
        turn_started.elapsed().as_secs(),
        input.text(),
        input.cursor(),
        &live_output,
    )?;
    tick = tick.wrapping_add(1);
    let mut last_render = Instant::now();

    let completed = loop {
        match task.try_complete() {
            Ok(Some(ProviderTurnUpdate::Completed(completed))) => break completed,
            Ok(Some(ProviderTurnUpdate::Canceled)) => {
                working.clear()?;
                drop(guard);
                print_plain_block("Provider request canceled.")?;
                return Ok(String::new());
            }
            Ok(None) => {
                if last_render.elapsed() >= IDLE_RENDER_INTERVAL {
                    working.render_with_cursor(
                        tick,
                        turn_started.elapsed().as_secs(),
                        input.text(),
                        input.cursor(),
                        &live_output,
                    )?;
                    tick = tick.wrapping_add(1);
                    last_render = Instant::now();
                }

                if event::poll(Duration::from_millis(60))? {
                    handle_active_provider_event(
                        &task,
                        &mut input,
                        &mut working,
                        tick,
                        turn_started.elapsed().as_secs(),
                        &live_output,
                    )?;
                }
            }
            Err(message) => {
                working.clear()?;
                drop(guard);
                print_plain_block(&format!("Provider error: {message}"))?;
                return Ok(String::new());
            }
        }
    };

    let preserved_input = input.text().to_string();
    working.clear()?;
    drop(guard);
    let completed = *completed;
    let turn_duration_millis = duration_millis(turn_started.elapsed());
    let turn_usage = aggregate_provider_token_usage(&completed.events);
    *session = completed.session;
    shell.consume_events(&completed.events);
    shell
        .conversation
        .push_turn_metrics(turn_duration_millis, turn_usage.as_ref());
    shell.conversation.follow_latest();
    print_new_conversation_lines(shell, before, true, false)?;
    print_pending_approval(session.pending_approval())?;
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Render,
            file!(),
            "run_inline_provider_turn",
            "ui_render_finished",
        )
        .with_duration_ms(turn_duration_millis)
        .with_metadata(serde_json::json!({
            "events_applied": completed.events.len(),
            "conversation_lines_before": before,
            "conversation_lines_after": shell.conversation.render_lines_with_styles().len()
        })),
    );
    Ok(preserved_input)
}
