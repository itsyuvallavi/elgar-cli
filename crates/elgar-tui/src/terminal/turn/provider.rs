//! Runs one submitted provider turn from the terminal.
//!
//! This file owns the live provider-turn loop: start the background task,
//! redraw progress, accept `/cancel`, and apply completed events to `TuiShell`.

use std::{
    io,
    time::{Duration, Instant},
};

use crossterm::event;
use elgar_core::{provider::ControllerProvider, session::Session};

use crate::{
    input::TerminalInput,
    terminal::{
        display_context::terminal_context,
        input::raw_mode::TerminalModeGuard,
        turn::{
            active::handle_active_provider_event,
            finalize::{decide_finalization, final_lines_after_preserved_preview},
            provider_logging::{
                log_live_preview_finalized, log_live_preview_render, log_tui_provider_turn_started,
                log_ui_render_finished,
            },
            provider_watchdog::{
                interactive_provider_watchdog_timeout, provider_watchdog_timeout_message,
            },
            provider_worker::{start_harness_turn, ProviderTurnUpdate},
        },
        ui::{
            prompt::{InlineWorkingRenderer, LiveProviderOutput},
            render::{
                print_conversation_line, print_new_conversation_lines, print_plain_block,
                print_spacer, print_user_block,
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
    log_tui_provider_turn_started(session, turn_id, text.chars().count());
    let before = shell.conversation.render_lines_with_styles().len();
    print_spacer()?;
    print_user_block(text)?;

    let task = start_harness_turn(provider.clone(), session.clone(), text.to_string());
    let guard = TerminalModeGuard::enter()?;
    let mut working = InlineWorkingRenderer::new(terminal_context(session, provider));
    let mut input = TerminalInput::default();
    let mut live_output = LiveProviderOutput::default();
    let mut tick = 0usize;
    working.render_with_cursor(
        tick,
        turn_started.elapsed().as_secs(),
        input.text(),
        input.cursor(),
        &live_output,
    )?;
    log_live_preview_render(
        session,
        turn_id,
        turn_started,
        "initial",
        &live_output,
        None,
        None,
        None,
    );
    tick = tick.wrapping_add(1);
    let mut last_render = Instant::now();
    let mut logged_unchanged_preview_idle = false;
    let watchdog = interactive_provider_watchdog_timeout();

    let (completed, completion_received_at) = loop {
        match task.try_complete() {
            Ok(Some(ProviderTurnUpdate::Completed(completed))) => {
                break (completed, Instant::now())
            }
            Ok(Some(ProviderTurnUpdate::Stream(event))) => {
                if let elgar_core::event::Event::ProviderStreamChunk(chunk) = &event {
                    let chunk_received_at = Instant::now();
                    live_output.push_stream_chunk(chunk);
                    let render_started = Instant::now();
                    working.render_with_cursor(
                        tick,
                        turn_started.elapsed().as_secs(),
                        input.text(),
                        input.cursor(),
                        &live_output,
                    )?;
                    log_live_preview_render(
                        session,
                        turn_id,
                        turn_started,
                        "stream_chunk",
                        &live_output,
                        Some(chunk),
                        Some(duration_millis(render_started.elapsed())),
                        Some(duration_millis(chunk_received_at.elapsed())),
                    );
                    logged_unchanged_preview_idle = false;
                    tick = tick.wrapping_add(1);
                    last_render = Instant::now();
                }
            }
            Ok(Some(ProviderTurnUpdate::Canceled)) => {
                working.clear()?;
                drop(guard);
                print_plain_block("Provider request canceled.")?;
                return Ok(String::new());
            }
            Ok(None) => {
                if turn_started.elapsed() >= watchdog {
                    working.clear()?;
                    task.cancel_for_watchdog(watchdog.as_millis().min(u128::from(u64::MAX)) as u64);
                    drop(guard);
                    print_plain_block(provider_watchdog_timeout_message())?;
                    return Ok(String::new());
                }

                if last_render.elapsed() >= IDLE_RENDER_INTERVAL {
                    if should_skip_idle_repaint(&live_output) {
                        if !logged_unchanged_preview_idle {
                            log_live_preview_render(
                                session,
                                turn_id,
                                turn_started,
                                "preview_unchanged_idle",
                                &live_output,
                                None,
                                None,
                                None,
                            );
                            logged_unchanged_preview_idle = true;
                        }
                    } else {
                        let render_started = Instant::now();
                        working.render_with_cursor(
                            tick,
                            turn_started.elapsed().as_secs(),
                            input.text(),
                            input.cursor(),
                            &live_output,
                        )?;
                        log_live_preview_render(
                            session,
                            turn_id,
                            turn_started,
                            "idle",
                            &live_output,
                            None,
                            Some(duration_millis(render_started.elapsed())),
                            None,
                        );
                        tick = tick.wrapping_add(1);
                    }
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

    let completed = *completed;
    let preserved_input = input.text().to_string();
    let turn_duration_millis = duration_millis(turn_started.elapsed());
    let turn_usage = aggregate_provider_token_usage(&completed.events);
    let finalization = decide_finalization(&completed.events, &live_output);
    let finalize_started = Instant::now();
    let preserved_preview = if finalization.should_preserve() {
        working.clear_chrome_preserving_response()?
    } else {
        false
    };
    if !preserved_preview {
        working.clear()?;
    }
    let finalize_render_ms = duration_millis(finalize_started.elapsed());
    drop(guard);
    *session = completed.session;
    shell.consume_events(&completed.events);
    shell
        .conversation
        .push_turn_metrics(turn_duration_millis, turn_usage.as_ref());
    shell.conversation.follow_latest();
    if preserved_preview {
        for (line, style) in final_lines_after_preserved_preview(
            &completed.events,
            &finalization,
            include_metrics_after_preserved_preview(),
            turn_duration_millis,
        ) {
            print_conversation_line(&line, style)?;
        }
    } else {
        print_new_conversation_lines(shell, before, true, false)?;
    }
    log_live_preview_finalized(
        session,
        turn_id,
        finalization.should_preserve(),
        preserved_preview,
        finalization.live_preview_chars,
        finalization.final_chars,
        finalize_render_ms,
    );
    log_ui_render_finished(
        session,
        turn_id,
        turn_duration_millis,
        &completed.events,
        completion_received_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        before,
        shell.conversation.render_lines_with_styles().len(),
    );
    Ok(preserved_input)
}

fn should_skip_idle_repaint(live_output: &LiveProviderOutput) -> bool {
    live_output.response_preview_stats().has_preview
}

fn include_metrics_after_preserved_preview() -> bool {
    true
}

#[cfg(test)]
mod tests;
