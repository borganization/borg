use ratatui::style::{Color, Modifier, Style};

pub const CYAN: Color = Color::Rgb(0, 185, 174);
pub const YELLOW: Color = Color::Rgb(2, 195, 189);
pub const GREEN: Color = Color::Rgb(0, 159, 147);
pub const RED: Color = Color::Red;
pub const DIM_WHITE: Color = Color::DarkGray;
pub const BORDER: Color = Color::Rgb(3, 113, 113);

pub const BULLET: &str = "•";
pub const CHEVRON: &str = "›";
pub const TREE_END: &str = "└";
pub const INPUT_PROMPT: &str = "› ";

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(DIM_WHITE)
}

pub fn code_style() -> Style {
    Style::default().fg(CYAN)
}

pub fn tool_style() -> Style {
    Style::default().fg(YELLOW)
}

pub fn success_style() -> Style {
    Style::default().fg(GREEN)
}

pub fn error_style() -> Style {
    Style::default().fg(RED)
}

pub fn popup_selected() -> Style {
    Style::default().bg(Color::Rgb(3, 49, 46)).fg(Color::White)
}

/// Style for user message lines — subtle background tint when terminal bg is known.
pub fn user_message_style() -> Style {
    match super::colors::user_message_bg() {
        Some(bg) => Style::default().bg(bg),
        None => Style::default(),
    }
}
