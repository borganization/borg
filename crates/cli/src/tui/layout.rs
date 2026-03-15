use ratatui::layout::{Constraint, Layout, Rect};

pub struct AppLayout {
    pub transcript: Rect,
    pub status: Rect,
    pub queue_preview: Rect,
    pub composer: Rect,
    pub footer: Rect,
}

pub fn compute_layout(
    area: Rect,
    composer_height: u16,
    show_status: bool,
    queue_preview_height: u16,
) -> AppLayout {
    let status_height = if show_status { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Min(3),                       // transcript
        Constraint::Length(status_height),        // status bar
        Constraint::Length(queue_preview_height), // queue preview
        Constraint::Length(composer_height),      // composer
        Constraint::Length(1),                    // footer
    ])
    .split(area);

    AppLayout {
        transcript: chunks[0],
        status: chunks[1],
        queue_preview: chunks[2],
        composer: chunks[3],
        footer: chunks[4],
    }
}
