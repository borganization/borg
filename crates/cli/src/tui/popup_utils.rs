use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

/// Compute centered popup geometry (60% width, 80% height, clamped).
pub fn popup_area(area: Rect) -> Rect {
    let popup_width = (area.width * 60 / 100)
        .max(44)
        .min(area.width.saturating_sub(4));
    let popup_height = (area.height * 80 / 100)
        .max(12)
        .min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    Rect::new(x, y, popup_width, popup_height)
}

/// Render a popup frame with a bordered block and title. Returns the inner area.
/// Clears the background, draws the border, and returns the usable inner rect.
pub fn render_popup_frame(frame: &mut Frame, popup_area: Rect, title: &str) -> Rect {
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::dim())
        .title(format!(" {title} "));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);
    inner
}

/// Render a footer hint bar at the bottom of the inner area.
pub fn render_footer(frame: &mut Frame, inner: Rect, hint: &str) {
    let footer_y = inner.y + inner.height - 1;
    let footer_area = Rect::new(inner.x, footer_y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint.to_string(), theme::dim()))),
        footer_area,
    );
}

/// Render an optional status message above the footer.
pub fn render_status_message(
    frame: &mut Frame,
    inner: Rect,
    message: Option<&(String, bool)>,
    offset_from_bottom: u16,
) {
    if let Some((msg, is_success)) = message {
        let style = if *is_success {
            theme::success_style()
        } else {
            theme::error_style()
        };
        let status_y = inner.y + inner.height - offset_from_bottom;
        let status_area = Rect::new(inner.x, status_y, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(format!(" {msg}"), style))),
            status_area,
        );
    }
}
