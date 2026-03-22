use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use borg_plugins::catalog::{PluginDef, CATALOG};
use borg_plugins::Category;

use super::theme;

/// State for a single item in the plugins list.
struct PluginItem {
    def: &'static PluginDef,
    is_installed: bool,
    is_selected: bool,
}

/// Phase of the plugins popup.
#[derive(Clone)]
enum PluginPhase {
    Browsing,
    CredentialInput {
        action_queue: Vec<(String, &'static [borg_plugins::CredentialSpec])>,
        current_def_idx: usize,
        current_cred_idx: usize,
        buffer: String,
        collected: Vec<(String, String)>,
        all_credentials: Vec<(String, Vec<(String, String)>)>,
    },
}

pub struct PluginsPopup {
    visible: bool,
    items: Vec<PluginItem>,
    cursor: usize,
    phase: PluginPhase,
    status_message: Option<(String, bool)>,
}

/// Actions that the plugins popup can request from the event loop.
pub enum PluginAction {
    Install {
        id: String,
        credentials: Vec<(String, String)>,
    },
    Uninstall {
        id: String,
    },
}

impl PluginsPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            cursor: 0,
            phase: PluginPhase::Browsing,
            status_message: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the popup, scanning filesystem for installed state.
    pub fn show(&mut self, data_dir: &std::path::Path) {
        self.visible = true;
        self.cursor = 0;
        self.phase = PluginPhase::Browsing;
        self.status_message = None;

        self.items = CATALOG
            .iter()
            .map(|def| {
                let installed = borg_plugins::installer::is_installed(def, data_dir);
                PluginItem {
                    def,
                    is_installed: installed,
                    is_selected: installed,
                }
            })
            .collect();
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.phase = PluginPhase::Browsing;
        self.status_message = None;
    }

    /// Handle a key event. Returns actions to execute if Enter is pressed.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Vec<PluginAction>> {
        use crossterm::event::KeyCode;

        if !self.visible {
            return None;
        }

        match &mut self.phase {
            PluginPhase::Browsing => match key.code {
                KeyCode::Esc => {
                    self.dismiss();
                    None
                }
                KeyCode::Up => {
                    if self.items.is_empty() {
                        return None;
                    }
                    if self.cursor == 0 {
                        self.cursor = self.items.len() - 1;
                    } else {
                        self.cursor -= 1;
                    }
                    self.status_message = None;
                    None
                }
                KeyCode::Down => {
                    if self.items.is_empty() {
                        return None;
                    }
                    self.cursor = (self.cursor + 1) % self.items.len();
                    self.status_message = None;
                    None
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if let Some(item) = self.items.get_mut(self.cursor) {
                        if !item.def.platform.is_available() {
                            self.status_message = Some((
                                format!(
                                    "{} requires {}",
                                    item.def.name,
                                    item.def.platform.label().unwrap_or("a different platform")
                                ),
                                false,
                            ));
                            return None;
                        }
                        item.is_selected = !item.is_selected;
                        self.status_message = None;
                    }
                    None
                }
                KeyCode::Tab => {
                    let actions = self.compute_pending_actions();
                    if actions.is_empty() {
                        self.status_message = Some(("No changes to apply.".to_string(), false));
                        return None;
                    }

                    // Check if any install actions need credentials
                    let mut needs_creds: Vec<(String, &'static [borg_plugins::CredentialSpec])> =
                        Vec::new();
                    let mut immediate_actions: Vec<PluginAction> = Vec::new();

                    for (id, is_install) in &actions {
                        if *is_install {
                            if let Some(def) = borg_plugins::catalog::find_by_id(id) {
                                let has_required =
                                    def.required_credentials.iter().any(|c| !c.is_optional);
                                if !has_required {
                                    immediate_actions.push(PluginAction::Install {
                                        id: id.clone(),
                                        credentials: Vec::new(),
                                    });
                                } else {
                                    needs_creds.push((id.clone(), def.required_credentials));
                                }
                            }
                        } else {
                            immediate_actions.push(PluginAction::Uninstall { id: id.clone() });
                        }
                    }

                    if needs_creds.is_empty() {
                        self.dismiss();
                        return Some(immediate_actions);
                    }

                    // Transition to credential input phase
                    let mut all_credentials: Vec<(String, Vec<(String, String)>)> = Vec::new();
                    // Pre-populate with immediate actions' empty credential lists
                    for action in &immediate_actions {
                        if let PluginAction::Install { id, credentials } = action {
                            all_credentials.push((id.clone(), credentials.clone()));
                        }
                    }

                    self.phase = PluginPhase::CredentialInput {
                        action_queue: needs_creds,
                        current_def_idx: 0,
                        current_cred_idx: 0,
                        buffer: String::new(),
                        collected: Vec::new(),
                        all_credentials,
                    };
                    self.status_message = None;
                    None
                }
                _ => None,
            },
            PluginPhase::CredentialInput {
                ref mut action_queue,
                ref mut current_def_idx,
                ref mut current_cred_idx,
                ref mut buffer,
                ref mut collected,
                ref mut all_credentials,
            } => match key.code {
                KeyCode::Esc => {
                    self.phase = PluginPhase::Browsing;
                    self.status_message = None;
                    None
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    None
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    None
                }
                KeyCode::Enter => {
                    if buffer.is_empty() {
                        self.status_message =
                            Some(("Credential value cannot be empty.".to_string(), false));
                        return None;
                    }

                    let cred_specs = action_queue[*current_def_idx].1;
                    let key = cred_specs[*current_cred_idx].key.to_string();
                    collected.push((key, buffer.clone()));
                    buffer.clear();
                    self.status_message = None;

                    *current_cred_idx += 1;

                    // Check if we've collected all creds for this plugin
                    if *current_cred_idx >= cred_specs.len() {
                        let id = action_queue[*current_def_idx].0.clone();
                        all_credentials.push((id, collected.clone()));
                        collected.clear();
                        *current_def_idx += 1;
                        *current_cred_idx = 0;
                    }

                    // Check if we've finished all plugins
                    if *current_def_idx >= action_queue.len() {
                        let mut final_actions: Vec<PluginAction> = Vec::new();

                        // Add uninstall actions from original pending
                        for item in &self.items {
                            if !item.is_selected && item.is_installed {
                                final_actions.push(PluginAction::Uninstall {
                                    id: item.def.id.to_string(),
                                });
                            }
                        }

                        // Add install actions with collected credentials
                        for (id, creds) in all_credentials.drain(..) {
                            final_actions.push(PluginAction::Install {
                                id,
                                credentials: creds,
                            });
                        }

                        self.dismiss();
                        return Some(final_actions);
                    }

                    None
                }
                _ => None,
            },
        }
    }

    /// Returns (id, is_install) pairs for pending changes.
    fn compute_pending_actions(&self) -> Vec<(String, bool)> {
        let mut actions = Vec::new();
        for item in &self.items {
            if item.is_selected && !item.is_installed {
                actions.push((item.def.id.to_string(), true));
            } else if !item.is_selected && item.is_installed {
                actions.push((item.def.id.to_string(), false));
            }
        }
        actions
    }

    pub fn render(&self, frame: &mut Frame) {
        if !self.visible {
            return;
        }

        let area = frame.area();
        let popup_width = (area.width * 60 / 100)
            .max(44)
            .min(area.width.saturating_sub(4));
        let popup_height = (area.height * 80 / 100)
            .max(12)
            .min(area.height.saturating_sub(2));
        let x = 1; // left-aligned with a small margin
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Plugins ");

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 5 || inner.width < 12 {
            return;
        }

        let content_height = (inner.height as usize).saturating_sub(2);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut row_indices: Vec<usize> = Vec::new();

        let mut last_category: Option<Category> = None;
        for (i, item) in self.items.iter().enumerate() {
            if last_category != Some(item.def.category) {
                if last_category.is_some() {
                    lines.push(Line::default());
                }
                lines.push(Line::from(Span::styled(
                    format!(" {}", item.def.category),
                    theme::bold(),
                )));
                last_category = Some(item.def.category);
            }

            row_indices.push(lines.len());

            let check = if item.is_selected { "x" } else { " " };
            let status = "";

            let platform_note = if let Some(label) = item.def.platform.label() {
                if !item.def.platform.is_available() {
                    format!("  ({label})")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let python_note = if item.def.required_bins.contains(&"python3")
                && which::which("python3").is_err()
            {
                "  (needs python3)".to_string()
            } else {
                String::new()
            };

            let label = format!(
                "  [{check}] {}{status}{platform_note}{python_note}",
                item.def.name,
            );

            let is_selected = i == self.cursor;
            let style = if is_selected {
                theme::popup_selected()
            } else {
                ratatui::style::Style::default()
            };

            lines.push(Line::from(Span::styled(label, style)));
        }

        // Scroll to keep cursor visible
        let selected_line = row_indices.get(self.cursor).copied().unwrap_or(0);
        let scroll_offset = if selected_line >= content_height {
            selected_line - content_height + 1
        } else {
            0
        };

        let visible_lines: Vec<Line<'static>> = lines
            .into_iter()
            .skip(scroll_offset)
            .take(content_height)
            .collect();

        let content_area = Rect::new(inner.x, inner.y, inner.width, content_height as u16);
        frame.render_widget(Paragraph::new(visible_lines), content_area);

        // Status line
        if let Some((ref msg, is_success)) = self.status_message {
            let style = if is_success {
                theme::success_style()
            } else {
                theme::error_style()
            };
            let status_y = inner.y + inner.height - 2;
            let status_area = Rect::new(inner.x, status_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(format!(" {msg}"), style))),
                status_area,
            );
        }

        // Credential input overlay
        if let PluginPhase::CredentialInput {
            ref action_queue,
            current_def_idx,
            current_cred_idx,
            ref buffer,
            ..
        } = self.phase
        {
            if let Some((ref id, cred_specs)) = action_queue.get(current_def_idx) {
                if let Some(cred) = cred_specs.get(current_cred_idx) {
                    let label_line = Line::from(vec![
                        Span::styled(" Enter ", theme::bold()),
                        Span::styled(cred.label.to_string(), theme::popup_selected()),
                        Span::styled(format!(" for {id}:"), theme::dim()),
                    ]);
                    let help_line = Line::from(Span::styled(
                        format!("   Get it at: {}", cred.help_url),
                        theme::dim(),
                    ));
                    let masked: String = "*".repeat(buffer.len()) + "_";
                    let input_line = Line::from(Span::styled(
                        format!("   > {masked}"),
                        theme::popup_selected(),
                    ));

                    let cred_y = inner.y + inner.height.saturating_sub(5);
                    let cred_area = Rect::new(inner.x, cred_y, inner.width, 3);
                    frame.render_widget(Clear, cred_area);
                    frame.render_widget(
                        Paragraph::new(vec![label_line, help_line, input_line]),
                        cred_area,
                    );
                }
            }
        }

        // Footer hint
        let hint = if matches!(self.phase, PluginPhase::CredentialInput { .. }) {
            " Enter: submit  Esc: cancel"
        } else {
            " Enter: toggle  Tab: apply  Esc: close"
        };
        let footer_y = inner.y + inner.height - 1;
        let footer_area = Rect::new(inner.x, footer_y, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(hint.to_string(), theme::dim()))),
            footer_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_popup_not_visible() {
        let popup = PluginsPopup::new();
        assert!(!popup.is_visible());
    }

    #[test]
    fn show_and_dismiss() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test");
        popup.show(&tmp);
        assert!(popup.is_visible());
        assert!(!popup.items.is_empty());
        popup.dismiss();
        assert!(!popup.is_visible());
    }

    #[test]
    fn navigation_wraps() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-nav");
        popup.show(&tmp);

        let count = popup.items.len();
        assert_eq!(popup.cursor, 0);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        popup.handle_key(up);
        assert_eq!(popup.cursor, count - 1);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        popup.handle_key(down);
        assert_eq!(popup.cursor, 0);
    }

    #[test]
    fn toggle_and_compute_actions() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-toggle");
        popup.show(&tmp);

        // Force initial state regardless of host env/keychain
        popup.items[0].is_installed = false;
        popup.items[0].is_selected = false;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Enter toggles selection
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        popup.handle_key(enter);
        assert!(popup.items[0].is_selected);

        // Space also toggles selection
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        popup.handle_key(space);
        assert!(!popup.items[0].is_selected);

        // Toggle back on for action check
        popup.handle_key(enter);
        assert!(popup.items[0].is_selected);

        let actions = popup.compute_pending_actions();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].1); // is_install = true
    }

    #[test]
    fn no_credential_input_for_zero_cred_defs() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-nocred");
        popup.show(&tmp);

        // Find iMessage (no required credentials) and select it
        let imessage_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "messaging/imessage")
            .expect("iMessage should be in catalog");
        popup.cursor = imessage_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        popup.handle_key(enter);
        assert!(popup.items[imessage_idx].is_selected);

        // Tab should produce actions immediately (no credential input phase)
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let result = popup.handle_key(tab);
        assert!(result.is_some());
        let actions = result.expect("should have actions");
        assert!(!actions.is_empty());
        assert!(matches!(
            &actions[0],
            PluginAction::Install {
                credentials,
                ..
            } if credentials.is_empty()
        ));
    }

    #[test]
    fn credential_input_phase_transitions() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-cred-phase");
        popup.show(&tmp);

        // Gmail requires credentials — select it
        let gmail_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "email/gmail")
            .expect("Gmail should be in catalog");
        popup.cursor = gmail_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        popup.handle_key(enter);

        // Tab should transition to CredentialInput
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let result = popup.handle_key(tab);
        assert!(result.is_none()); // No actions yet
        assert!(matches!(popup.phase, PluginPhase::CredentialInput { .. }));
    }

    #[test]
    fn credential_input_esc_cancels() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-cred-esc");
        popup.show(&tmp);

        let gmail_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "email/gmail")
            .expect("Gmail should be in catalog");
        popup.cursor = gmail_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(popup.phase, PluginPhase::CredentialInput { .. }));

        popup.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(popup.phase, PluginPhase::Browsing));
    }

    #[test]
    fn credential_input_backspace() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-cred-bs");
        popup.show(&tmp);

        let gmail_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "email/gmail")
            .expect("Gmail should be in catalog");
        popup.cursor = gmail_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        // Type some characters
        popup.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        if let PluginPhase::CredentialInput { ref buffer, .. } = popup.phase {
            assert_eq!(buffer, "abc");
        } else {
            panic!("expected CredentialInput phase");
        }

        popup.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        if let PluginPhase::CredentialInput { ref buffer, .. } = popup.phase {
            assert_eq!(buffer, "ab");
        } else {
            panic!("expected CredentialInput phase");
        }
    }

    #[test]
    fn credential_input_enter_produces_install_action() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-cred-enter");
        popup.show(&tmp);

        // Gmail has 1 required credential
        let gmail_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "email/gmail")
            .expect("Gmail should be in catalog");
        popup.cursor = gmail_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        // Type credential value
        for c in "my-bot-token".chars() {
            popup.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        // Submit with Enter
        let result = popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        // Gmail has 1 credential, so this should complete
        assert!(result.is_some());
        let actions = result.expect("should have actions");
        let install_action = actions
            .iter()
            .find(|a| matches!(a, PluginAction::Install { .. }))
            .expect("should have install action");
        if let PluginAction::Install { credentials, .. } = install_action {
            assert_eq!(credentials.len(), 1);
            assert_eq!(credentials[0].0, "GMAIL_API_KEY");
            assert_eq!(credentials[0].1, "my-bot-token");
        }
    }

    #[test]
    fn native_plugins_appear_in_list() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-native-list");
        popup.show(&tmp);

        let native_ids = [
            "messaging/telegram",
            "messaging/slack",
            "messaging/discord",
            "messaging/teams",
            "messaging/google-chat",
        ];
        for id in &native_ids {
            assert!(
                popup
                    .items
                    .iter()
                    .any(|i| i.def.id == *id && i.def.is_native),
                "native plugin {id} should appear in list"
            );
        }
    }

    #[test]
    fn native_plugins_count() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-native-count");
        popup.show(&tmp);

        let native_count = popup.items.iter().filter(|i| i.def.is_native).count();
        assert_eq!(native_count, 5);
    }

    #[test]
    fn native_plugin_credential_input_flow() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-native-cred");
        popup.show(&tmp);

        let telegram_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "messaging/telegram")
            .expect("Telegram should be in catalog");

        // Force initial state regardless of host env/keychain
        popup.items[telegram_idx].is_installed = false;
        popup.items[telegram_idx].is_selected = false;
        popup.cursor = telegram_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(popup.items[telegram_idx].is_selected);

        popup.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(popup.phase, PluginPhase::CredentialInput { .. }));

        for c in "test-token".chars() {
            popup.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        let result = popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(result.is_some());
        let actions = result.unwrap();
        let install_action = actions
            .iter()
            .find(|a| matches!(a, PluginAction::Install { id, .. } if id == "messaging/telegram"))
            .expect("should have telegram install action");
        if let PluginAction::Install { credentials, .. } = install_action {
            assert_eq!(credentials.len(), 1);
            assert_eq!(credentials[0].0, "TELEGRAM_BOT_TOKEN");
            assert_eq!(credentials[0].1, "test-token");
        }
    }

    #[test]
    fn native_plugin_multi_credential_flow() {
        let mut popup = PluginsPopup::new();
        let tmp = std::env::temp_dir().join("borg-plugins-test-native-multi");
        unsafe {
            std::env::remove_var("SLACK_BOT_TOKEN");
            std::env::remove_var("SLACK_SIGNING_SECRET");
        }
        popup.show(&tmp);

        let slack_idx = popup
            .items
            .iter()
            .position(|i| i.def.id == "messaging/slack")
            .expect("Slack should be in catalog");
        popup.cursor = slack_idx;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        popup.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(popup.phase, PluginPhase::CredentialInput { .. }));

        for c in "xoxb-token".chars() {
            popup.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let result = popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(result.is_none());

        for c in "signing-secret".chars() {
            popup.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let result = popup.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(result.is_some());
        let actions = result.unwrap();
        let install_action = actions
            .iter()
            .find(|a| matches!(a, PluginAction::Install { id, .. } if id == "messaging/slack"))
            .expect("should have slack install action");
        if let PluginAction::Install { credentials, .. } = install_action {
            assert_eq!(credentials.len(), 2);
            assert_eq!(credentials[0].0, "SLACK_BOT_TOKEN");
            assert_eq!(credentials[1].0, "SLACK_SIGNING_SECRET");
        }
    }
}
