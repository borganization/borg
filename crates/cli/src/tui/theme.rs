use ratatui::style::{Color, Modifier, Style};

pub const CYAN: Color = Color::Cyan;
pub const YELLOW: Color = Color::Yellow;
pub const GREEN: Color = Color::Green;
pub const RED: Color = Color::Red;
pub const DIM_WHITE: Color = Color::DarkGray;
pub const BORDER: Color = Color::DarkGray;

pub const BULLET: &str = "•";
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
