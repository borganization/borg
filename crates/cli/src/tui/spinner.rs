use std::time::Duration;

use ratatui::text::{Line, Span};

use super::theme;

const TRANSCRIPT_FRAMES: &[&str] = &["(·_·)    ", "(·_·) .  ", "(·_·) .. ", "(·_·) ..."];
const TRANSCRIPT_TICK_MS: u128 = 300;

const STATUS_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const STATUS_TICK_MS: u128 = 100;

pub fn transcript_spinner_lines(elapsed: Duration) -> Vec<Line<'static>> {
    let idx = (elapsed.as_millis() / TRANSCRIPT_TICK_MS) as usize % TRANSCRIPT_FRAMES.len();
    vec![Line::from(Span::styled(
        TRANSCRIPT_FRAMES[idx].to_string(),
        theme::dim(),
    ))]
}

pub fn status_spinner_frame(elapsed: Duration) -> Span<'static> {
    let idx = (elapsed.as_millis() / STATUS_TICK_MS) as usize % STATUS_FRAMES.len();
    Span::styled(STATUS_FRAMES[idx].to_string(), theme::tool_style())
}
