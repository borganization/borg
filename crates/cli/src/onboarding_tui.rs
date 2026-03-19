use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use crate::onboarding::{
    format_number, keychain_available, models_for_provider, provider_id_to_display,
    provider_key_url, KeyStorage, OnboardingResult, PROVIDERS, STYLES,
};
use crate::tui::theme;
use borg_core::config::Config;
use borg_core::provider::Provider;
use std::str::FromStr;

const LOGO: &str = r#"
oooooooooo.    .oooooo.   ooooooooo.     .oooooo.
`888'   `Y8b  d8P'  `Y8b  `888   `Y88.  d8P'  `Y8b
 888     888 888      888  888   .d88' 888
 888oooo888' 888      888  888ooo88P'  888
 888    `88b 888      888  888`88b.    888     ooooo
 888    .88P `88b    d88'  888  `88b.  `88.    .88'
o888bood8P'   `Y8bood8P'  o888o  o888o  `Y8bood8P'"#;

const LOGO_HEIGHT: u16 = 8; // includes leading blank line

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Welcome,
    Style,
    Provider,
    Model,
    ApiKey,
    Budget,
    Review,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Welcome,
        Tab::Style,
        Tab::Provider,
        Tab::Model,
        Tab::ApiKey,
        Tab::Budget,
        Tab::Review,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Welcome => "Welcome",
            Tab::Style => "Style",
            Tab::Provider => "Provider",
            Tab::Model => "Model",
            Tab::ApiKey => "API Key",
            Tab::Budget => "Budget",
            Tab::Review => "Review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WelcomeFocus {
    UserName,
    AgentName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewFocus {
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    TextInput,
}

struct OnboardingState {
    tab: Tab,
    input_mode: InputMode,

    // Welcome
    user_name: String,
    agent_name: String,
    welcome_focus: WelcomeFocus,

    // Style
    style_cursor: usize,

    // Provider
    provider_cursor: usize,
    provider_keys_detected: Vec<bool>,

    // Model
    model_cursor: usize,

    // API Key
    api_key_input: String,
    api_key_existing: bool,

    // Budget
    budget_cursor: usize,
    custom_budget_input: String,
    custom_budget_editing: bool,

    // Review
    review_focus: ReviewFocus,

    // Result
    done: bool,
    cancelled: bool,
}

impl OnboardingState {
    fn new() -> Self {
        let provider_keys_detected: Vec<bool> = PROVIDERS
            .iter()
            .map(|(id, _, _)| {
                Provider::from_str(id)
                    .ok()
                    .and_then(|p| std::env::var(p.default_env_var()).ok())
                    .is_some()
            })
            .collect();

        let api_key_existing = Self::check_existing_api_key("openrouter");

        Self {
            tab: Tab::Welcome,
            input_mode: InputMode::TextInput,
            user_name: String::new(),
            agent_name: "Borg".to_string(),
            welcome_focus: WelcomeFocus::UserName,
            style_cursor: 0,
            provider_cursor: 0,
            provider_keys_detected,
            model_cursor: 0,
            api_key_input: String::new(),
            api_key_existing,
            budget_cursor: 1, // default to 1M tokens
            custom_budget_input: String::new(),
            custom_budget_editing: false,
            review_focus: ReviewFocus::Confirm,
            done: false,
            cancelled: false,
        }
    }

    fn check_existing_api_key(provider_id: &str) -> bool {
        let Ok(provider) = Provider::from_str(provider_id) else {
            return false;
        };
        let env_var_name = provider.default_env_var();
        let Ok(data_dir) = Config::data_dir() else {
            return false;
        };
        let env_path = data_dir.join(".env");
        if !env_path.exists() {
            return false;
        }
        std::fs::read_to_string(&env_path)
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix(&format!("{env_var_name}="))
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
            })
            .is_some_and(|k| !k.is_empty())
    }

    fn current_provider_id(&self) -> &'static str {
        PROVIDERS[self.provider_cursor].0
    }

    fn current_models(&self) -> &'static [(&'static str, &'static str)] {
        models_for_provider(self.current_provider_id())
    }

    fn tab_completed(&self, tab: Tab) -> bool {
        match tab {
            Tab::Welcome => !self.user_name.is_empty(),
            Tab::Style => true, // always has a default selection
            Tab::Provider => true,
            Tab::Model => true,
            Tab::ApiKey => true, // optional
            Tab::Budget => true,
            Tab::Review => false,
        }
    }

    fn monthly_token_limit(&self) -> u64 {
        match self.budget_cursor {
            0 => 500_000,
            1 => 1_000_000,
            2 => 5_000_000,
            3 => 0, // unlimited
            4 => {
                // custom
                self.custom_budget_input
                    .trim()
                    .replace(',', "")
                    .parse::<u64>()
                    .unwrap_or(1_000_000)
            }
            _ => 1_000_000,
        }
    }

    fn build_result(&self) -> OnboardingResult {
        let provider_id = self.current_provider_id();
        let models = self.current_models();
        let model_id = models
            .get(self.model_cursor)
            .map(|(id, _)| id.to_string())
            .unwrap_or_else(|| models[0].0.to_string());

        let api_key = if self.api_key_input.trim().is_empty() || self.api_key_existing {
            None
        } else {
            Some(self.api_key_input.trim().to_string())
        };

        let key_storage = if api_key.is_some() && keychain_available() {
            KeyStorage::Keychain
        } else {
            KeyStorage::EnvFile
        };

        OnboardingResult {
            user_name: self.user_name.clone(),
            agent_name: if self.agent_name.is_empty() {
                "Borg".to_string()
            } else {
                self.agent_name.clone()
            },
            style_index: self.style_cursor,
            model_id,
            api_key,
            key_storage,
            provider: provider_id.to_string(),
            monthly_token_limit: self.monthly_token_limit(),
        }
    }

    fn next_tab(&mut self) {
        let idx = self.tab.index();
        if idx < Tab::ALL.len() - 1 {
            // Validate before advancing from Welcome
            if self.tab == Tab::Welcome && self.user_name.trim().is_empty() {
                return;
            }
            self.tab = Tab::ALL[idx + 1];
            self.update_input_mode();
        }
    }

    fn prev_tab(&mut self) {
        let idx = self.tab.index();
        if idx > 0 {
            self.tab = Tab::ALL[idx - 1];
            self.update_input_mode();
        }
    }

    fn update_input_mode(&mut self) {
        self.input_mode = match self.tab {
            Tab::Welcome => InputMode::TextInput,
            Tab::ApiKey if !self.api_key_existing => InputMode::TextInput,
            Tab::Budget if self.budget_cursor == 4 && self.custom_budget_editing => {
                InputMode::TextInput
            }
            _ => InputMode::Normal,
        };
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Global: Esc cancels
        if key.code == KeyCode::Esc {
            self.cancelled = true;
            return;
        }

        // Route based on tab and input mode
        match self.tab {
            Tab::Welcome => self.handle_welcome_key(key),
            Tab::Style => self.handle_list_key(key, STYLES.len()),
            Tab::Provider => self.handle_provider_key(key),
            Tab::Model => {
                let len = self.current_models().len();
                self.handle_list_key(key, len);
            }
            Tab::ApiKey => self.handle_api_key_key(key),
            Tab::Budget => self.handle_budget_key(key),
            Tab::Review => self.handle_review_key(key),
        }
    }

    fn handle_welcome_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    match self.welcome_focus {
                        WelcomeFocus::UserName => {
                            self.welcome_focus = WelcomeFocus::AgentName;
                        }
                        WelcomeFocus::AgentName => {
                            self.next_tab();
                        }
                    }
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // do nothing
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Left => {
                self.prev_tab();
            }
            KeyCode::Enter => match self.welcome_focus {
                WelcomeFocus::UserName => {
                    self.welcome_focus = WelcomeFocus::AgentName;
                }
                WelcomeFocus::AgentName => {
                    self.next_tab();
                }
            },
            KeyCode::Backspace => {
                let buf = match self.welcome_focus {
                    WelcomeFocus::UserName => &mut self.user_name,
                    WelcomeFocus::AgentName => &mut self.agent_name,
                };
                buf.pop();
            }
            KeyCode::Char(c) => {
                let buf = match self.welcome_focus {
                    WelcomeFocus::UserName => &mut self.user_name,
                    WelcomeFocus::AgentName => &mut self.agent_name,
                };
                buf.push(c);
            }
            _ => {}
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent, len: usize) {
        match key.code {
            KeyCode::Up => {
                let cursor = self.active_list_cursor_mut();
                if *cursor == 0 {
                    *cursor = len - 1;
                } else {
                    *cursor -= 1;
                }
            }
            KeyCode::Down => {
                let cursor = self.active_list_cursor_mut();
                *cursor = (*cursor + 1) % len;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.next_tab();
            }
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Left => {
                self.prev_tab();
            }
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < Tab::ALL.len() {
                    self.tab = Tab::ALL[idx];
                    self.update_input_mode();
                }
            }
            _ => {}
        }
    }

    fn handle_provider_key(&mut self, key: KeyEvent) {
        let len = PROVIDERS.len();
        match key.code {
            KeyCode::Up => {
                if self.provider_cursor == 0 {
                    self.provider_cursor = len - 1;
                } else {
                    self.provider_cursor -= 1;
                }
                self.on_provider_change();
            }
            KeyCode::Down => {
                self.provider_cursor = (self.provider_cursor + 1) % len;
                self.on_provider_change();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.next_tab();
            }
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Left => {
                self.prev_tab();
            }
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < Tab::ALL.len() {
                    self.tab = Tab::ALL[idx];
                    self.update_input_mode();
                }
            }
            _ => {}
        }
    }

    fn handle_api_key_key(&mut self, key: KeyEvent) {
        if self.api_key_existing {
            // Already configured, just navigate
            match key.code {
                KeyCode::Tab | KeyCode::Right => {
                    if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.prev_tab();
                    } else {
                        self.next_tab();
                    }
                }
                KeyCode::Left => self.prev_tab(),
                KeyCode::Enter => self.next_tab(),
                KeyCode::Char(c @ '1'..='7') => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < Tab::ALL.len() {
                        self.tab = Tab::ALL[idx];
                        self.update_input_mode();
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Right => self.next_tab(),
            KeyCode::Left => self.prev_tab(),
            KeyCode::Enter => self.next_tab(),
            KeyCode::Backspace => {
                self.api_key_input.pop();
            }
            KeyCode::Char(c) => {
                self.api_key_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_budget_key(&mut self, key: KeyEvent) {
        if self.custom_budget_editing {
            match key.code {
                KeyCode::Enter => {
                    self.custom_budget_editing = false;
                    self.input_mode = InputMode::Normal;
                    self.next_tab();
                }
                KeyCode::Backspace => {
                    self.custom_budget_input.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() || c == ',' => {
                    self.custom_budget_input.push(c);
                }
                KeyCode::Tab => {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.custom_budget_editing = false;
                        self.input_mode = InputMode::Normal;
                        self.prev_tab();
                    } else {
                        self.custom_budget_editing = false;
                        self.input_mode = InputMode::Normal;
                        self.next_tab();
                    }
                }
                _ => {}
            }
            return;
        }

        let len = 5; // budget options count
        match key.code {
            KeyCode::Up => {
                if self.budget_cursor == 0 {
                    self.budget_cursor = len - 1;
                } else {
                    self.budget_cursor -= 1;
                }
            }
            KeyCode::Down => {
                self.budget_cursor = (self.budget_cursor + 1) % len;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.budget_cursor == 4 {
                    self.custom_budget_editing = true;
                    self.input_mode = InputMode::TextInput;
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Left => self.prev_tab(),
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < Tab::ALL.len() {
                    self.tab = Tab::ALL[idx];
                    self.update_input_mode();
                }
            }
            _ => {}
        }
    }

    fn handle_review_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                self.review_focus = match self.review_focus {
                    ReviewFocus::Confirm => ReviewFocus::Cancel,
                    ReviewFocus::Cancel => ReviewFocus::Confirm,
                };
            }
            KeyCode::Enter => match self.review_focus {
                ReviewFocus::Confirm => {
                    self.done = true;
                }
                ReviewFocus::Cancel => {
                    self.cancelled = true;
                }
            },
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    // no next tab from Review
                }
            }
            KeyCode::Left => self.prev_tab(),
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < Tab::ALL.len() {
                    self.tab = Tab::ALL[idx];
                    self.update_input_mode();
                }
            }
            _ => {}
        }
    }

    fn on_provider_change(&mut self) {
        let models = self.current_models();
        if self.model_cursor >= models.len() {
            self.model_cursor = 0;
        }
        self.api_key_existing = Self::check_existing_api_key(self.current_provider_id());
        self.api_key_input.clear();
    }

    fn active_list_cursor_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Style => &mut self.style_cursor,
            Tab::Provider => &mut self.provider_cursor,
            Tab::Model => &mut self.model_cursor,
            Tab::Budget => &mut self.budget_cursor,
            _ => &mut self.style_cursor, // fallback
        }
    }
}

// ── Rendering ──

fn render(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &OnboardingState,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();

        // Check minimum size
        if area.width < 60 || area.height < 20 {
            let msg = Paragraph::new("Please resize your terminal to at least 60x20")
                .alignment(Alignment::Center);
            let y = area.height / 2;
            let msg_area = Rect::new(0, y, area.width, 1);
            frame.render_widget(msg, msg_area);
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(LOGO_HEIGHT),
            Constraint::Length(1), // tab bar
            Constraint::Min(8),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

        render_logo(frame, chunks[0]);
        render_tab_bar(frame, chunks[1], state);
        render_tab_content(frame, chunks[2], state);
        render_footer(frame, chunks[3], state);
    })?;
    Ok(())
}

fn render_logo(frame: &mut ratatui::Frame, area: Rect) {
    let lines: Vec<Line> = LOGO
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    let logo = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(logo, area);
}

fn render_tab_bar(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw("  "));

    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", theme::dim()));
        }

        let completed = state.tab_completed(*tab) && *tab != state.tab;
        let prefix = if completed { "✓ " } else { "" };
        let label = format!("{prefix}{}", tab.label());

        let style = if *tab == state.tab {
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD)
        } else if completed {
            Style::default().fg(theme::GREEN)
        } else {
            theme::dim()
        };

        spans.push(Span::styled(label, style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_tab_content(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    // Add some padding
    let content_area = Rect::new(
        area.x + 4,
        area.y + 1,
        area.width.saturating_sub(8),
        area.height.saturating_sub(2),
    );

    match state.tab {
        Tab::Welcome => render_welcome(frame, content_area, state),
        Tab::Style => render_style(frame, content_area, state),
        Tab::Provider => render_provider(frame, content_area, state),
        Tab::Model => render_model(frame, content_area, state),
        Tab::ApiKey => render_api_key(frame, content_area, state),
        Tab::Budget => render_budget(frame, content_area, state),
        Tab::Review => render_review(frame, content_area, state),
    }
}

fn render_welcome(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Let's set up your personal AI assistant.",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::default());
    lines.push(Line::default());

    // User name field
    let user_name_focused = state.welcome_focus == WelcomeFocus::UserName;
    let user_label_style = if user_name_focused {
        Style::default()
            .fg(theme::CYAN)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    lines.push(Line::from(Span::styled(
        "  What's your name?",
        user_label_style,
    )));
    let user_cursor = if user_name_focused { "_" } else { "" };
    let user_border_style = if user_name_focused {
        theme::CYAN
    } else {
        theme::DIM_WHITE
    };
    lines.push(Line::from(vec![
        Span::styled("  ┃ ", Style::default().fg(user_border_style)),
        Span::raw(&state.user_name),
        Span::styled(user_cursor, Style::default().fg(theme::CYAN)),
    ]));

    lines.push(Line::default());
    lines.push(Line::default());

    // Agent name field
    let agent_name_focused = state.welcome_focus == WelcomeFocus::AgentName;
    let agent_label_style = if agent_name_focused {
        Style::default()
            .fg(theme::CYAN)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    lines.push(Line::from(Span::styled(
        "  What's your agent's name?",
        agent_label_style,
    )));
    let agent_cursor = if agent_name_focused { "_" } else { "" };
    let agent_border_style = if agent_name_focused {
        theme::CYAN
    } else {
        theme::DIM_WHITE
    };
    lines.push(Line::from(vec![
        Span::styled("  ┃ ", Style::default().fg(agent_border_style)),
        Span::raw(&state.agent_name),
        Span::styled(agent_cursor, Style::default().fg(theme::CYAN)),
    ]));

    if state.user_name.trim().is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "  Enter your name to continue",
            theme::dim(),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_style(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Pick a personality style:",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::default());

    for (i, style) in STYLES.iter().enumerate() {
        let selected = i == state.style_cursor;
        let marker = if selected { "▸ " } else { "  " };
        let item_style = if selected {
            theme::popup_selected()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {marker}{} — {}", style.name, style.description),
            item_style,
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_provider(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Choose your LLM provider:",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::from(Span::styled(
        "You can change this later in config.toml",
        theme::dim(),
    )));
    lines.push(Line::default());

    for (i, (_, name, desc)) in PROVIDERS.iter().enumerate() {
        let selected = i == state.provider_cursor;
        let marker = if selected { "▸ " } else { "  " };
        let item_style = if selected {
            theme::popup_selected()
        } else {
            Style::default()
        };

        let badge = if state.provider_keys_detected[i] {
            Span::styled(" [key detected]", Style::default().fg(theme::GREEN))
        } else {
            Span::raw("")
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {marker}{name} — {desc}"), item_style),
            badge,
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_model(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();
    let provider_name = provider_id_to_display(state.current_provider_id());

    lines.push(Line::from(Span::styled(
        format!("Choose your default model ({provider_name}):"),
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::from(Span::styled(
        "You can change this later in config.toml",
        theme::dim(),
    )));
    lines.push(Line::default());

    let models = state.current_models();
    for (i, (id, label)) in models.iter().enumerate() {
        let selected = i == state.model_cursor;
        let marker = if selected { "▸ " } else { "  " };
        let item_style = if selected {
            theme::popup_selected()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {marker}{label}"),
            item_style,
        )));

        // Show model ID for selected item
        if selected {
            lines.push(Line::from(Span::styled(
                format!("      {id}"),
                theme::dim(),
            )));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_api_key(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();
    let provider_name = provider_id_to_display(state.current_provider_id());
    let key_url = provider_key_url(state.current_provider_id());

    lines.push(Line::from(Span::styled(
        format!("{provider_name} API Key:"),
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::default());

    if state.api_key_existing {
        lines.push(Line::from(Span::styled(
            "  ✓ API key already configured",
            Style::default().fg(theme::GREEN),
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "  Press Tab or Enter to continue",
            theme::dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  Get yours at: {key_url}"),
            theme::dim(),
        )));
        lines.push(Line::from(Span::styled(
            "  Leave empty to skip",
            theme::dim(),
        )));
        lines.push(Line::default());

        let masked: String = "*".repeat(state.api_key_input.len());
        lines.push(Line::from(vec![
            Span::styled("  ┃ ", Style::default().fg(theme::CYAN)),
            Span::raw(masked),
            Span::styled("_", Style::default().fg(theme::CYAN)),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_budget(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Set a monthly token budget (hard limit):",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::from(Span::styled(
        "Prevents runaway costs — you can change this later in /settings",
        theme::dim(),
    )));
    lines.push(Line::default());

    let options = [
        "500,000 tokens",
        "1,000,000 tokens",
        "5,000,000 tokens",
        "Unlimited",
        "Custom",
    ];

    for (i, label) in options.iter().enumerate() {
        let selected = i == state.budget_cursor;
        let marker = if selected { "▸ " } else { "  " };
        let item_style = if selected {
            theme::popup_selected()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("  {marker}{label}"),
            item_style,
        )));
    }

    if state.custom_budget_editing {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "  Enter monthly token limit:",
            Style::default().fg(theme::CYAN),
        )));
        lines.push(Line::from(vec![
            Span::styled("  ┃ ", Style::default().fg(theme::CYAN)),
            Span::raw(&state.custom_budget_input),
            Span::styled("_", Style::default().fg(theme::CYAN)),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_review(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();
    let provider_id = state.current_provider_id();
    let models = state.current_models();
    let model_id = models
        .get(state.model_cursor)
        .map(|(id, _)| *id)
        .unwrap_or(models[0].0);

    lines.push(Line::from(Span::styled(
        "Review your choices:",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::default());

    let check = Span::styled("  ✓ ", Style::default().fg(theme::GREEN));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Your name:  ", theme::dim()),
        Span::raw(&state.user_name),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Agent name: ", theme::dim()),
        Span::raw(if state.agent_name.is_empty() {
            "Borg"
        } else {
            &state.agent_name
        }),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Style:      ", theme::dim()),
        Span::raw(STYLES[state.style_cursor].name),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Provider:   ", theme::dim()),
        Span::raw(provider_id_to_display(provider_id)),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Model:      ", theme::dim()),
        Span::raw(model_id),
    ]));

    let api_key_display = if state.api_key_existing {
        "(already set)".to_string()
    } else if state.api_key_input.trim().is_empty() {
        "(skipped)".to_string()
    } else {
        let k = state.api_key_input.trim();
        if k.len() > 8 {
            format!("{}...{}", &k[..5], &k[k.len() - 4..])
        } else {
            "****".to_string()
        }
    };
    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("API key:    ", theme::dim()),
        Span::raw(api_key_display),
    ]));

    let budget_display = if state.monthly_token_limit() == 0 {
        "Unlimited".to_string()
    } else {
        format!("{} tokens", format_number(state.monthly_token_limit()))
    };
    lines.push(Line::from(vec![
        check,
        Span::styled("Budget:     ", theme::dim()),
        Span::raw(budget_display),
    ]));

    lines.push(Line::default());
    lines.push(Line::default());

    // Confirm / Cancel buttons
    let confirm_style = if state.review_focus == ReviewFocus::Confirm {
        Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let cancel_style = if state.review_focus == ReviewFocus::Cancel {
        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };

    let confirm_marker = if state.review_focus == ReviewFocus::Confirm {
        "▸ "
    } else {
        "  "
    };
    let cancel_marker = if state.review_focus == ReviewFocus::Cancel {
        "▸ "
    } else {
        "  "
    };

    lines.push(Line::from(Span::styled(
        format!("  {confirm_marker}[Confirm]"),
        confirm_style,
    )));
    lines.push(Line::from(Span::styled(
        format!("  {cancel_marker}[Cancel]"),
        cancel_style,
    )));

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let hint = match state.tab {
        Tab::Welcome => " Tab: next field  Enter: next  Esc: cancel",
        Tab::Review => " ↑↓: select  Enter: confirm  Esc: cancel",
        _ if state.input_mode == InputMode::TextInput => " Type to enter  Tab: next  Esc: cancel",
        _ => " Tab/→: next  Shift+Tab/←: back  ↑↓: navigate  Enter: select  Esc: cancel",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, theme::dim()))),
        area,
    );
}

// ── Terminal guard ──

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

// ── Public entry point ──

pub fn run() -> Result<Option<OnboardingResult>> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = OnboardingState::new();

    loop {
        render(&mut terminal, &state)?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Ignore key release events on some platforms
                if key.kind == crossterm::event::KeyEventKind::Release {
                    continue;
                }
                state.handle_key(key);
            }
        }

        if state.done {
            return Ok(Some(state.build_result()));
        }
        if state.cancelled {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::ANTHROPIC_MODELS;

    #[test]
    fn tab_navigation_wraps_forward() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string(); // so we can advance past Welcome
        assert_eq!(state.tab, Tab::Welcome);
        state.next_tab();
        assert_eq!(state.tab, Tab::Style);
        state.next_tab();
        assert_eq!(state.tab, Tab::Provider);
    }

    #[test]
    fn tab_navigation_wraps_backward() {
        let mut state = OnboardingState::new();
        state.tab = Tab::Style;
        state.prev_tab();
        assert_eq!(state.tab, Tab::Welcome);
        // Can't go before first
        state.prev_tab();
        assert_eq!(state.tab, Tab::Welcome);
    }

    #[test]
    fn provider_change_resets_model_cursor() {
        let mut state = OnboardingState::new();
        state.provider_cursor = 0; // openrouter
        state.model_cursor = 5; // some model in openrouter list
        state.provider_cursor = 2; // anthropic (only 3 models)
        state.on_provider_change();
        assert!(state.model_cursor < ANTHROPIC_MODELS.len());
    }

    #[test]
    fn empty_name_blocks_advance() {
        let mut state = OnboardingState::new();
        state.user_name.clear();
        state.next_tab();
        assert_eq!(state.tab, Tab::Welcome); // should not advance
    }

    #[test]
    fn review_builds_correct_result() {
        let mut state = OnboardingState::new();
        state.user_name = "Alice".to_string();
        state.agent_name = "Buddy".to_string();
        state.style_cursor = 1;
        state.provider_cursor = 2; // anthropic
        state.model_cursor = 0; // first anthropic model
        state.budget_cursor = 0; // 500k

        let result = state.build_result();
        assert_eq!(result.user_name, "Alice");
        assert_eq!(result.agent_name, "Buddy");
        assert_eq!(result.style_index, 1);
        assert_eq!(result.provider, "anthropic");
        assert_eq!(result.model_id, ANTHROPIC_MODELS[0].0);
        assert_eq!(result.monthly_token_limit, 500_000);
    }

    #[test]
    fn budget_parsing() {
        let mut state = OnboardingState::new();

        state.budget_cursor = 0;
        assert_eq!(state.monthly_token_limit(), 500_000);

        state.budget_cursor = 1;
        assert_eq!(state.monthly_token_limit(), 1_000_000);

        state.budget_cursor = 2;
        assert_eq!(state.monthly_token_limit(), 5_000_000);

        state.budget_cursor = 3;
        assert_eq!(state.monthly_token_limit(), 0);

        state.budget_cursor = 4;
        state.custom_budget_input = "2,500,000".to_string();
        assert_eq!(state.monthly_token_limit(), 2_500_000);

        state.custom_budget_input.clear();
        assert_eq!(state.monthly_token_limit(), 1_000_000); // default for empty
    }

    #[test]
    fn empty_agent_name_defaults_to_borg() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string();
        state.agent_name.clear();
        let result = state.build_result();
        assert_eq!(result.agent_name, "Borg");
    }

    #[test]
    fn esc_sets_cancelled() {
        let mut state = OnboardingState::new();
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.cancelled);
    }

    #[test]
    fn tab_completed_checks() {
        let mut state = OnboardingState::new();
        assert!(!state.tab_completed(Tab::Welcome)); // empty name
        state.user_name = "Test".to_string();
        assert!(state.tab_completed(Tab::Welcome));
        assert!(state.tab_completed(Tab::Style));
        assert!(state.tab_completed(Tab::Provider));
        assert!(!state.tab_completed(Tab::Review)); // never "complete"
    }
}
