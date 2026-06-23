//! Shared terminal color styles.
//!
//! Ratatui views use these helpers so colors stay consistent across panes.

use ratatui::style::{Color, Modifier, Style};

pub(crate) fn primary() -> Style {
    Style::default().fg(Color::Rgb(214, 219, 224))
}

pub(crate) fn muted() -> Style {
    Style::default().fg(Color::Rgb(117, 126, 138))
}

pub(crate) fn accent() -> Style {
    Style::default()
        .fg(Color::Rgb(117, 196, 187))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn user_input_block() -> Style {
    Style::default()
        .fg(Color::Rgb(142, 210, 201))
        .bg(Color::Rgb(25, 47, 50))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn thinking() -> Style {
    Style::default().fg(Color::Rgb(150, 159, 176))
}

pub(crate) fn model_output() -> Style {
    primary()
}

pub(crate) fn event() -> Style {
    Style::default().fg(Color::Rgb(102, 220, 150))
}

pub(crate) fn context_normal() -> Style {
    muted()
}

pub(crate) fn code_border() -> Style {
    Style::default()
        .fg(Color::Rgb(83, 94, 108))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_header() -> Style {
    Style::default()
        .fg(Color::Rgb(117, 196, 187))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_body() -> Style {
    Style::default()
        .fg(Color::Rgb(224, 229, 235))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_hint() -> Style {
    Style::default()
        .fg(Color::Rgb(150, 159, 176))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_key() -> Style {
    Style::default()
        .fg(Color::Rgb(117, 196, 187))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_string() -> Style {
    Style::default()
        .fg(Color::Rgb(186, 214, 194))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_number() -> Style {
    Style::default()
        .fg(Color::Rgb(214, 181, 110))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_literal() -> Style {
    Style::default()
        .fg(Color::Rgb(218, 154, 118))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn code_comment() -> Style {
    Style::default()
        .fg(Color::Rgb(117, 126, 138))
        .bg(Color::Rgb(18, 22, 28))
}

pub(crate) fn raw_details() -> Style {
    Style::default().fg(Color::Rgb(180, 188, 196))
}

#[cfg(test)]
mod tests;
