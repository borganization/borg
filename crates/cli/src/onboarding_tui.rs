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
    keychain_available, models_for_provider, provider_id_to_display, provider_key_url, KeyStorage,
    OnboardingResult, PROVIDERS,
};
use crate::plugins::{self, PluginDef};
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

const SECURITY_WARNING: &str = "\
Security warning — please read.

Borg is a personal AI agent that can execute tools and shell commands.
By default, it operates as a single trusted-operator system.

If you expose Borg to multiple users (e.g. via gateway channels),
each user shares the agent's delegated tool authority.

Recommended baseline:
  • Sandbox mode enabled (strict by default)
  • Least-privilege tools — don't grant unnecessary fs/network access
  • Keep secrets out of the agent's reachable filesystem
  • Shared use: isolate sessions per user/channel

Run diagnostics anytime: borg doctor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Welcome,
    Security,
    Provider,
    ApiKey,
    Channels,
    Summary,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Welcome,
        Tab::Security,
        Tab::Provider,
        Tab::ApiKey,
        Tab::Channels,
        Tab::Summary,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Welcome => "Welcome",
            Tab::Security => "Security",
            Tab::Provider => "Provider",
            Tab::ApiKey => "API Key",
            Tab::Channels => "Channels",
            Tab::Summary => "Summary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WelcomeFocus {
    UserName,
    AgentName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecurityFocus {
    Accept,
    Decline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryFocus {
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    TextInput,
}

/// Tracks which channel credential is being actively entered.
#[derive(Debug, Clone)]
struct ChannelCredEntry {
    /// Index into the channel plugins list
    plugin_index: usize,
    /// Which credential within that plugin
    cred_index: usize,
    /// Current text input for this credential
    input: String,
    /// All credentials collected so far for this plugin (key, value)
    collected: Vec<(String, String)>,
}

struct OnboardingState {
    tab: Tab,
    input_mode: InputMode,

    // Welcome
    user_name: String,
    agent_name: String,
    welcome_focus: WelcomeFocus,

    // Security
    security_focus: SecurityFocus,
    security_accepted: bool,

    // Provider
    provider_cursor: usize,
    provider_keys_detected: Vec<bool>,

    // API Key
    api_key_input: String,
    api_key_existing: bool,

    // Channels
    channel_cursor: usize,
    channel_configuring: Option<ChannelCredEntry>,
    configured_channels: Vec<String>, // plugin names that were configured

    // Summary
    summary_focus: SummaryFocus,

    // Validation hints
    api_key_required_hint: bool,

    // Result
    done: bool,
    cancelled: bool,
}

fn channel_plugins() -> Vec<&'static PluginDef> {
    plugins::PLUGINS.iter().filter(|p| p.is_channel).collect()
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
            security_focus: SecurityFocus::Accept,
            security_accepted: false,
            provider_cursor: 0,
            provider_keys_detected,
            api_key_input: String::new(),
            api_key_existing,
            channel_cursor: 0,
            channel_configuring: None,
            configured_channels: Vec::new(),
            summary_focus: SummaryFocus::Confirm,
            api_key_required_hint: false,
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

    fn tab_completed(&self, tab: Tab) -> bool {
        match tab {
            Tab::Welcome => !self.user_name.is_empty(),
            Tab::Security => self.security_accepted,
            Tab::Provider => true,
            Tab::ApiKey => true,
            Tab::Channels => true, // optional
            Tab::Summary => false,
        }
    }

    fn build_result(&self) -> OnboardingResult {
        let provider_id = self.current_provider_id();
        let models = models_for_provider(provider_id);
        let model_id = models[0].0.to_string(); // always use recommended model

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
            style_index: 0, // Professional (default)
            model_id,
            api_key,
            key_storage,
            provider: provider_id.to_string(),
            monthly_token_limit: 1_000_000, // default
            configured_channels: self.configured_channels.clone(),
            channel_credentials: self.collect_channel_credentials(),
        }
    }

    /// Collect all channel credentials that were entered during onboarding.
    fn collect_channel_credentials(&self) -> Vec<(String, Vec<(String, String)>)> {
        // This is populated during channel configuration and stored on the result
        // The actual credentials are stored as the user enters them, tracked via
        // configured_channels. We don't keep the raw credentials in state after
        // they're stored — they get persisted to keychain/.env during apply.
        Vec::new()
    }

    fn next_tab(&mut self) {
        let idx = self.tab.index();
        if idx < Tab::ALL.len() - 1 {
            // Validate before advancing
            match self.tab {
                Tab::Welcome if self.user_name.trim().is_empty() => return,
                Tab::Security if !self.security_accepted => return,
                _ => {}
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
            _ => InputMode::Normal,
        };
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Global: Esc cancels (unless configuring a channel credential)
        if key.code == KeyCode::Esc {
            if self.channel_configuring.is_some() {
                // Cancel channel credential entry
                self.channel_configuring = None;
                self.input_mode = InputMode::Normal;
                return;
            }
            self.cancelled = true;
            return;
        }

        match self.tab {
            Tab::Welcome => self.handle_welcome_key(key),
            Tab::Security => self.handle_security_key(key),
            Tab::Provider => self.handle_provider_key(key),
            Tab::ApiKey => self.handle_api_key_key(key),
            Tab::Channels => self.handle_channels_key(key),
            Tab::Summary => self.handle_summary_key(key),
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
            KeyCode::Right => self.next_tab(),
            KeyCode::Left => self.prev_tab(),
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

    fn handle_security_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                self.security_focus = match self.security_focus {
                    SecurityFocus::Accept => SecurityFocus::Decline,
                    SecurityFocus::Decline => SecurityFocus::Accept,
                };
            }
            KeyCode::Enter => match self.security_focus {
                SecurityFocus::Accept => {
                    self.security_accepted = true;
                    self.next_tab();
                }
                SecurityFocus::Decline => {
                    self.cancelled = true;
                }
            },
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else if self.security_accepted {
                    self.next_tab();
                }
            }
            KeyCode::Left => self.prev_tab(),
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
            KeyCode::Enter | KeyCode::Char(' ') => self.next_tab(),
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else {
                    self.next_tab();
                }
            }
            KeyCode::Left => self.prev_tab(),
            _ => {}
        }
    }

    fn handle_api_key_key(&mut self, key: KeyEvent) {
        if self.api_key_existing {
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
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                } else if self.api_key_input.trim().is_empty() {
                    self.api_key_required_hint = true;
                } else {
                    self.api_key_required_hint = false;
                    self.next_tab();
                }
            }
            KeyCode::Right => {
                if self.api_key_input.trim().is_empty() {
                    self.api_key_required_hint = true;
                } else {
                    self.api_key_required_hint = false;
                    self.next_tab();
                }
            }
            KeyCode::Left => self.prev_tab(),
            KeyCode::Enter => {
                if self.api_key_input.trim().is_empty() {
                    self.api_key_required_hint = true;
                } else {
                    self.api_key_required_hint = false;
                    self.next_tab();
                }
            }
            KeyCode::Backspace => {
                self.api_key_input.pop();
                self.api_key_required_hint = false;
            }
            KeyCode::Char(c) => {
                self.api_key_input.push(c);
                self.api_key_required_hint = false;
            }
            _ => {}
        }
    }

    fn handle_channels_key(&mut self, key: KeyEvent) {
        // If actively entering a credential
        if let Some(ref mut entry) = self.channel_configuring {
            match key.code {
                KeyCode::Enter => {
                    if !entry.input.trim().is_empty() {
                        let plugins = channel_plugins();
                        let plugin = plugins[entry.plugin_index];
                        let cred = &plugin.credentials[entry.cred_index];
                        entry
                            .collected
                            .push((cred.key.to_string(), entry.input.trim().to_string()));
                        entry.input.clear();
                        entry.cred_index += 1;

                        // Check if all credentials for this plugin are collected
                        if entry.cred_index >= plugin.credentials.len() {
                            // Store credentials
                            let plugin_name = plugin.name.to_string();
                            let collected = entry.collected.clone();
                            self.channel_configuring = None;
                            self.input_mode = InputMode::Normal;

                            // Store credentials immediately
                            if let Err(e) = store_channel_credentials(&plugin_name, &collected) {
                                // Silently continue — will show as unconfigured
                                tracing::warn!("Failed to store channel credentials: {e}");
                            } else {
                                self.configured_channels.push(plugin_name);
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    entry.input.pop();
                }
                KeyCode::Char(c) => {
                    entry.input.push(c);
                }
                _ => {}
            }
            return;
        }

        let plugins = channel_plugins();
        let len = plugins.len();
        match key.code {
            KeyCode::Up => {
                if self.channel_cursor == 0 {
                    self.channel_cursor = len.saturating_sub(1);
                } else {
                    self.channel_cursor -= 1;
                }
            }
            KeyCode::Down => {
                if len > 0 {
                    self.channel_cursor = (self.channel_cursor + 1) % len;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.channel_cursor < len {
                    let plugin = plugins[self.channel_cursor];
                    if !self.configured_channels.contains(&plugin.name.to_string())
                        && !plugin.credentials.is_empty()
                    {
                        self.channel_configuring = Some(ChannelCredEntry {
                            plugin_index: self.channel_cursor,
                            cred_index: 0,
                            input: String::new(),
                            collected: Vec::new(),
                        });
                        self.input_mode = InputMode::TextInput;
                    }
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
            _ => {}
        }
    }

    fn handle_summary_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                self.summary_focus = match self.summary_focus {
                    SummaryFocus::Confirm => SummaryFocus::Cancel,
                    SummaryFocus::Cancel => SummaryFocus::Confirm,
                };
            }
            KeyCode::Enter => match self.summary_focus {
                SummaryFocus::Confirm => {
                    self.done = true;
                }
                SummaryFocus::Cancel => {
                    self.cancelled = true;
                }
            },
            KeyCode::Tab | KeyCode::Right => {
                if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_tab();
                }
            }
            KeyCode::Left => self.prev_tab(),
            _ => {}
        }
    }

    fn on_provider_change(&mut self) {
        self.api_key_existing = Self::check_existing_api_key(self.current_provider_id());
        self.api_key_input.clear();
    }
}

/// Store channel credentials to keychain or .env file.
fn store_channel_credentials(plugin_name: &str, credentials: &[(String, String)]) -> Result<()> {
    let use_keychain = keychain_available();
    let data_dir = Config::data_dir()?;
    let service_name = format!("borg-{plugin_name}");

    for (key, value) in credentials {
        if use_keychain {
            borg_plugins::keychain::store(&service_name, key, value)
                .map_err(|e| anyhow::anyhow!("Keychain store failed: {e}"))?;
        } else {
            let env_path = data_dir.join(".env");
            let mut env_content = if env_path.exists() {
                std::fs::read_to_string(&env_path)?
            } else {
                String::new()
            };
            let prefix = format!("{key}=");
            let filtered: String = env_content
                .lines()
                .filter(|line| !line.starts_with(&prefix))
                .collect::<Vec<_>>()
                .join("\n");
            env_content = if filtered.is_empty() {
                String::new()
            } else {
                filtered + "\n"
            };
            let clean = value.trim().replace(['\n', '\r'], "");
            env_content.push_str(&format!("{key}={clean}\n"));
            std::fs::write(&env_path, &env_content)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
}

// ── Rendering ──

fn render(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &OnboardingState,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();

        if area.width < 60 || area.height < 20 {
            let msg = Paragraph::new("Please resize your terminal to at least 60x20")
                .alignment(Alignment::Center);
            let y = area.height / 2;
            let msg_area = Rect::new(0, y, area.width, 1);
            frame.render_widget(msg, msg_area);
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(LOGO_HEIGHT + 1),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
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
    let max_width = LOGO.lines().map(str::len).max().unwrap_or(0) as u16;
    let logo_width = max_width.min(area.width);
    let x_offset = area.x + area.width.saturating_sub(logo_width) / 2;
    let centered_area = Rect::new(x_offset, area.y, logo_width, area.height);
    let logo = Paragraph::new(lines).alignment(Alignment::Left);
    frame.render_widget(logo, centered_area);
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
    let content_area = Rect::new(
        area.x + 4,
        area.y + 1,
        area.width.saturating_sub(8),
        area.height.saturating_sub(2),
    );

    match state.tab {
        Tab::Welcome => render_welcome(frame, content_area, state),
        Tab::Security => render_security(frame, content_area, state),
        Tab::Provider => render_provider(frame, content_area, state),
        Tab::ApiKey => render_api_key(frame, content_area, state),
        Tab::Channels => render_channels(frame, content_area, state),
        Tab::Summary => render_summary(frame, content_area, state),
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
    let user_cursor = if user_name_focused { "▊" } else { "" };
    let user_border_style = if user_name_focused {
        theme::CYAN
    } else {
        theme::DIM_WHITE
    };
    lines.push(Line::from(vec![
        Span::styled("  › ", Style::default().fg(user_border_style)),
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
    let agent_cursor = if agent_name_focused { "▊" } else { "" };
    let agent_border_style = if agent_name_focused {
        theme::CYAN
    } else {
        theme::DIM_WHITE
    };
    lines.push(Line::from(vec![
        Span::styled("  › ", Style::default().fg(agent_border_style)),
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

fn render_security(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    // Security warning box
    for line in SECURITY_WARNING.lines() {
        lines.push(Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(theme::YELLOW),
        )));
    }

    lines.push(Line::default());
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "  I understand. Continue?",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::default());

    // Accept / Decline
    let accept_style = if state.security_focus == SecurityFocus::Accept {
        Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let decline_style = if state.security_focus == SecurityFocus::Decline {
        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };

    let accept_marker = if state.security_focus == SecurityFocus::Accept {
        "▸ "
    } else {
        "  "
    };
    let decline_marker = if state.security_focus == SecurityFocus::Decline {
        "▸ "
    } else {
        "  "
    };

    lines.push(Line::from(Span::styled(
        format!("  {accept_marker}Yes"),
        accept_style,
    )));
    lines.push(Line::from(Span::styled(
        format!("  {decline_marker}No"),
        decline_style,
    )));

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_provider(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Choose your LLM provider:",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::from(Span::styled(
        "Customize later with: borg settings",
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
            "  Required to continue",
            theme::dim(),
        )));
        lines.push(Line::default());

        let masked: String = "*".repeat(state.api_key_input.len());
        lines.push(Line::from(vec![
            Span::styled("  › ", Style::default().fg(theme::CYAN)),
            Span::raw(masked),
            Span::styled("▊", Style::default().fg(theme::CYAN)),
        ]));

        if state.api_key_required_hint {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "  ⚠ Please enter your API key to continue",
                Style::default().fg(theme::YELLOW),
            )));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_channels(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Connect a messaging channel (optional):",
        Style::default().fg(theme::CYAN),
    )));
    lines.push(Line::from(Span::styled(
        "Select a channel to configure, or Tab to skip",
        theme::dim(),
    )));
    lines.push(Line::default());

    let plugins = channel_plugins();

    // If actively configuring a channel
    if let Some(ref entry) = state.channel_configuring {
        let plugin = plugins[entry.plugin_index];
        let cred = &plugin.credentials[entry.cred_index];

        lines.push(Line::from(Span::styled(
            format!("  Configuring: {}", plugin.name),
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  {} ({})", cred.label, cred.help),
            theme::dim(),
        )));
        lines.push(Line::default());

        let masked: String = "*".repeat(entry.input.len());
        lines.push(Line::from(vec![
            Span::styled("  › ", Style::default().fg(theme::CYAN)),
            Span::raw(masked),
            Span::styled("▊", Style::default().fg(theme::CYAN)),
        ]));

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(
                "  Credential {} of {}",
                entry.cred_index + 1,
                plugin.credentials.len()
            ),
            theme::dim(),
        )));
    } else {
        // Channel list
        for (i, plugin) in plugins.iter().enumerate() {
            let selected = i == state.channel_cursor;
            let marker = if selected { "▸ " } else { "  " };
            let item_style = if selected {
                theme::popup_selected()
            } else {
                Style::default()
            };

            let status = if state.configured_channels.contains(&plugin.name.to_string()) {
                Span::styled(" ✓ configured", Style::default().fg(theme::GREEN))
            } else {
                let cred_count = plugin.credentials.len();
                let label = if cred_count == 1 {
                    " needs token".to_string()
                } else {
                    format!(" needs {cred_count} tokens")
                };
                Span::styled(label, theme::dim())
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {marker}{} — {}", plugin.name, plugin.description),
                    item_style,
                ),
                status,
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_summary(frame: &mut ratatui::Frame, area: Rect, state: &OnboardingState) {
    let mut lines: Vec<Line> = Vec::new();
    let provider_id = state.current_provider_id();
    let models = models_for_provider(provider_id);
    let model_id = models[0].0;

    lines.push(Line::from(Span::styled(
        "✓ Setup complete!",
        Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::default());

    let check = Span::styled("  ✓ ", Style::default().fg(theme::GREEN));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("User:      ", theme::dim()),
        Span::raw(&state.user_name),
    ]));

    let agent_name = if state.agent_name.is_empty() {
        "Borg"
    } else {
        &state.agent_name
    };
    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Agent:     ", theme::dim()),
        Span::raw(format!("{agent_name} (Professional)")),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Provider:  ", theme::dim()),
        Span::raw(provider_id_to_display(provider_id)),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Model:     ", theme::dim()),
        Span::raw(model_id),
    ]));

    let api_key_display = if state.api_key_existing {
        "(already set)".to_string()
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
        Span::styled("API key:   ", theme::dim()),
        Span::raw(api_key_display),
    ]));

    lines.push(Line::from(vec![
        check.clone(),
        Span::styled("Budget:    ", theme::dim()),
        Span::raw("1,000,000 tokens/month"),
    ]));

    lines.push(Line::default());

    // Defaults
    lines.push(Line::from(Span::styled("  Defaults:", theme::dim())));
    lines.push(Line::from(Span::styled(
        "    Gateway:  127.0.0.1:7842",
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        "    Sandbox:  strict",
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        "    Memory:   8,000 token context",
        theme::dim(),
    )));

    // Channels
    if !state.configured_channels.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("  Channels:", theme::dim())));
        for name in &state.configured_channels {
            lines.push(Line::from(vec![
                Span::styled("    ✓ ", Style::default().fg(theme::GREEN)),
                Span::raw(name.as_str()),
            ]));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  Customize: borg settings  |  Diagnostics: borg doctor  |  Gateway: borg gateway",
        theme::dim(),
    )));

    lines.push(Line::default());
    lines.push(Line::default());

    // Confirm / Cancel buttons
    let confirm_style = if state.summary_focus == SummaryFocus::Confirm {
        Style::default()
            .fg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let cancel_style = if state.summary_focus == SummaryFocus::Cancel {
        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };

    let confirm_marker = if state.summary_focus == SummaryFocus::Confirm {
        "▸ "
    } else {
        "  "
    };
    let cancel_marker = if state.summary_focus == SummaryFocus::Cancel {
        "▸ "
    } else {
        "  "
    };

    lines.push(Line::from(Span::styled(
        format!("  {confirm_marker}[Confirm & Launch]"),
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
        Tab::Security => " ↑↓: select  Enter: confirm  Esc: cancel",
        Tab::Channels if state.channel_configuring.is_some() => {
            " Type to enter  Enter: submit  Esc: cancel"
        }
        Tab::Channels => " ↑↓: navigate  Enter: configure  Tab: skip  Esc: cancel",
        Tab::Summary => " ↑↓: select  Enter: confirm  Esc: cancel",
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
    fn tab_flow_is_correct() {
        assert_eq!(
            Tab::ALL,
            [
                Tab::Welcome,
                Tab::Security,
                Tab::Provider,
                Tab::ApiKey,
                Tab::Channels,
                Tab::Summary,
            ]
        );
    }

    #[test]
    fn tab_navigation_forward() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string();
        state.security_accepted = true;
        assert_eq!(state.tab, Tab::Welcome);
        state.next_tab();
        assert_eq!(state.tab, Tab::Security);
        state.next_tab();
        assert_eq!(state.tab, Tab::Provider);
    }

    #[test]
    fn tab_navigation_backward() {
        let mut state = OnboardingState::new();
        state.tab = Tab::Security;
        state.prev_tab();
        assert_eq!(state.tab, Tab::Welcome);
        state.prev_tab();
        assert_eq!(state.tab, Tab::Welcome);
    }

    #[test]
    fn security_blocks_without_acceptance() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string();
        state.tab = Tab::Security;
        state.security_accepted = false;
        state.next_tab();
        assert_eq!(state.tab, Tab::Security); // should not advance
    }

    #[test]
    fn security_allows_with_acceptance() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string();
        state.tab = Tab::Security;
        state.security_accepted = true;
        state.next_tab();
        assert_eq!(state.tab, Tab::Provider);
    }

    #[test]
    fn empty_name_blocks_advance() {
        let mut state = OnboardingState::new();
        state.user_name.clear();
        state.next_tab();
        assert_eq!(state.tab, Tab::Welcome);
    }

    #[test]
    fn build_result_uses_defaults() {
        let mut state = OnboardingState::new();
        state.user_name = "Alice".to_string();
        state.agent_name = "Buddy".to_string();
        state.provider_cursor = 0; // openrouter

        let result = state.build_result();
        assert_eq!(result.user_name, "Alice");
        assert_eq!(result.agent_name, "Buddy");
        assert_eq!(result.style_index, 0); // Professional
        assert_eq!(result.provider, "openrouter");
        assert_eq!(result.monthly_token_limit, 1_000_000);
    }

    #[test]
    fn build_result_uses_recommended_model() {
        let mut state = OnboardingState::new();
        state.user_name = "Test".to_string();
        state.provider_cursor = 2; // anthropic
        state.on_provider_change();
        let result = state.build_result();
        assert_eq!(result.model_id, ANTHROPIC_MODELS[0].0);
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
        assert!(!state.tab_completed(Tab::Security)); // not accepted
        state.security_accepted = true;
        assert!(state.tab_completed(Tab::Security));
        assert!(state.tab_completed(Tab::Provider));
        assert!(state.tab_completed(Tab::Channels));
        assert!(!state.tab_completed(Tab::Summary)); // never "complete"
    }

    #[test]
    fn provider_change_resets_api_key() {
        let mut state = OnboardingState::new();
        state.api_key_input = "some-key".to_string();
        state.provider_cursor = 2; // anthropic
        state.on_provider_change();
        assert!(state.api_key_input.is_empty());
    }
}
