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

#[cfg(test)]
pub(crate) fn success() -> Style {
    Style::default().fg(Color::Rgb(143, 188, 143))
}

pub(crate) fn warning_action() -> Style {
    Style::default()
        .fg(Color::Rgb(214, 181, 110))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
pub(crate) fn error() -> Style {
    Style::default()
        .fg(Color::Rgb(218, 118, 118))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::{
        accent, error, model_output, muted, primary, success, thinking, user_input_block,
        warning_action,
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
        assert_eq!(success().fg, Some(Color::Rgb(143, 188, 143)));
        assert_eq!(warning_action().fg, Some(Color::Rgb(214, 181, 110)));
        assert_eq!(error().fg, Some(Color::Rgb(218, 118, 118)));
    }

    #[test]
    fn emphasis_is_limited_to_interactive_or_stateful_styles() {
        assert!(!primary().add_modifier.contains(Modifier::BOLD));
        assert!(!muted().add_modifier.contains(Modifier::BOLD));
        assert!(!thinking().add_modifier.contains(Modifier::BOLD));
        assert!(accent().add_modifier.contains(Modifier::BOLD));
        assert!(user_input_block().add_modifier.contains(Modifier::BOLD));
        assert!(warning_action().add_modifier.contains(Modifier::BOLD));
        assert!(error().add_modifier.contains(Modifier::BOLD));
    }
}
