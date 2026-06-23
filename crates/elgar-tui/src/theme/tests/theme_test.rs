//! Tests for active TUI theme styles.

use ratatui::style::{Color, Modifier};

use super::super::{
    accent, code_body, code_border, code_comment, code_header, code_hint, code_key, code_literal,
    code_number, code_string, context_normal, model_output, muted, primary, raw_details, thinking,
    user_input_block,
};

#[test]
fn named_theme_styles_use_calm_dark_terminal_colors() {
    assert_eq!(primary().fg, Some(Color::Rgb(214, 219, 224)));
    assert_eq!(muted().fg, Some(Color::Rgb(117, 126, 138)));
    assert_eq!(accent().fg, Some(Color::Rgb(117, 196, 187)));
    assert_eq!(user_input_block().fg, Some(Color::Rgb(142, 210, 201)));
    assert_eq!(user_input_block().bg, Some(Color::Rgb(25, 47, 50)));
    assert_eq!(thinking().fg, Some(Color::Rgb(150, 159, 176)));
    assert_eq!(model_output().fg, primary().fg);
    assert_eq!(context_normal().fg, muted().fg);
    assert_eq!(code_border().fg, Some(Color::Rgb(83, 94, 108)));
    assert_eq!(code_border().bg, Some(Color::Rgb(18, 22, 28)));
    assert_eq!(code_header().fg, Some(Color::Rgb(117, 196, 187)));
    assert_eq!(code_header().bg, code_border().bg);
    assert_eq!(code_body().fg, Some(Color::Rgb(224, 229, 235)));
    assert_eq!(code_body().bg, code_border().bg);
    assert_eq!(code_hint().fg, Some(Color::Rgb(150, 159, 176)));
    assert_eq!(code_hint().bg, code_border().bg);
    assert_eq!(code_key().fg, Some(Color::Rgb(117, 196, 187)));
    assert_eq!(code_key().bg, code_border().bg);
    assert_eq!(code_string().fg, Some(Color::Rgb(186, 214, 194)));
    assert_eq!(code_string().bg, code_border().bg);
    assert_eq!(code_number().fg, Some(Color::Rgb(214, 181, 110)));
    assert_eq!(code_number().bg, code_border().bg);
    assert_eq!(code_literal().fg, Some(Color::Rgb(218, 154, 118)));
    assert_eq!(code_literal().bg, code_border().bg);
    assert_eq!(code_comment().fg, Some(Color::Rgb(117, 126, 138)));
    assert_eq!(code_comment().bg, code_border().bg);
    assert_eq!(raw_details().fg, Some(Color::Rgb(180, 188, 196)));
}

#[test]
fn emphasis_is_limited_to_active_interactive_styles() {
    assert!(!primary().add_modifier.contains(Modifier::BOLD));
    assert!(!muted().add_modifier.contains(Modifier::BOLD));
    assert!(!thinking().add_modifier.contains(Modifier::BOLD));
    assert!(!context_normal().add_modifier.contains(Modifier::BOLD));
    assert!(!code_border().add_modifier.contains(Modifier::BOLD));
    assert!(!code_header().add_modifier.contains(Modifier::BOLD));
    assert!(!code_body().add_modifier.contains(Modifier::BOLD));
    assert!(!code_hint().add_modifier.contains(Modifier::BOLD));
    assert!(!code_key().add_modifier.contains(Modifier::BOLD));
    assert!(!code_string().add_modifier.contains(Modifier::BOLD));
    assert!(!code_number().add_modifier.contains(Modifier::BOLD));
    assert!(!code_literal().add_modifier.contains(Modifier::BOLD));
    assert!(!code_comment().add_modifier.contains(Modifier::BOLD));
    assert!(!raw_details().add_modifier.contains(Modifier::BOLD));
    assert!(accent().add_modifier.contains(Modifier::BOLD));
    assert!(user_input_block().add_modifier.contains(Modifier::BOLD));
}
