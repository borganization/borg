use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlanOption {
    ClearAndProceed,
    ProceedWithContext,
    TypeFeedback,
}

pub struct PlanOverlay {
    pub visible: bool,
    selected: PlanOption,
    context_pct: u8,
    agent_name: String,
}

impl PlanOverlay {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: PlanOption::ClearAndProceed,
            context_pct: 0,
            agent_name: String::new(),
        }
    }

    pub fn show(&mut self, context_pct: u8, agent_name: String) {
        self.visible = true;
        self.selected = PlanOption::ClearAndProceed;
        self.context_pct = context_pct;
        self.agent_name = agent_name;
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
    }

    pub fn cycle(&mut self) {
        self.selected = match self.selected {
            PlanOption::ClearAndProceed => PlanOption::ProceedWithContext,
            PlanOption::ProceedWithContext => PlanOption::TypeFeedback,
            PlanOption::TypeFeedback => PlanOption::ClearAndProceed,
        };
    }

    pub fn select(&mut self, opt: PlanOption) {
        self.selected = opt;
    }

    pub fn selected(&self) -> PlanOption {
        self.selected
    }

    pub fn render(&self, frame: &mut Frame, composer_area: Rect) {
        if !self.visible {
            return;
        }

        let popup_height: u16 = 5;
        let available_above = composer_area.y;
        if available_above < popup_height {
            return;
        }

        let popup_width = composer_area.width.min(60);
        let popup_y = composer_area.y - popup_height;
        let popup_area = Rect::new(composer_area.x, popup_y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let options = [
            (
                PlanOption::ClearAndProceed,
                format!(
                    "1. Yes, clear context ({}% used) and proceed",
                    self.context_pct
                ),
            ),
            (
                PlanOption::ProceedWithContext,
                "2. Yes, proceed with context".to_string(),
            ),
            (
                PlanOption::TypeFeedback,
                format!("3. Type here to tell {} what to change", self.agent_name),
            ),
        ];

        let lines: Vec<Line<'_>> = options
            .iter()
            .map(|(opt, label)| {
                let is_selected = *opt == self.selected;
                let prefix = if is_selected { " \u{276f} " } else { "   " };
                let style = if is_selected {
                    theme::popup_selected()
                } else {
                    theme::dim()
                };
                Line::from(Span::styled(format!("{prefix}{label}"), style))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Plan Review ");

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup_area);
    }
}
