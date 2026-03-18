use ratatui::text::{Line, Span};
use throbber_widgets_tui::{Throbber, ThrobberState, BRAILLE_EIGHT};

use super::markdown;
use super::theme;

#[derive(Clone)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

#[derive(Clone)]
pub enum HistoryCell {
    User {
        text: String,
    },
    Assistant {
        text: String,
        streaming: bool,
    },
    ToolStart {
        name: String,
        args: String,
    },
    ToolResult {
        #[allow(dead_code)]
        name: String,
        output: String,
        is_error: bool,
    },
    ShellApproval {
        command: String,
        status: ApprovalStatus,
    },
    ToolApproval {
        tool_name: String,
        reason: String,
        status: ApprovalStatus,
    },
    Heartbeat {
        text: String,
    },
    System {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolStreaming {
        #[allow(dead_code)]
        name: String,
        lines: Vec<(String, bool)>,
    },
}

/// Truncate a string to at most `max_bytes` bytes at a valid UTF-8 boundary.
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl HistoryCell {
    pub fn render(&self, width: u16, throbber_state: Option<&ThrobberState>) -> Vec<Line<'static>> {
        match self {
            HistoryCell::User { text } => {
                let bg = theme::user_message_style();
                let prefix_style =
                    bg.add_modifier(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::DIM);
                let mut lines = Vec::new();
                // Top padding
                lines.push(Line::from("").style(bg));
                // Content lines
                for (i, line) in text.lines().enumerate() {
                    let prefix = if i == 0 {
                        Span::styled(format!("{} ", theme::CHEVRON), prefix_style)
                    } else {
                        Span::styled("  ", bg)
                    };
                    lines.push(
                        Line::from(vec![prefix, Span::styled(line.to_string(), bg)]).style(bg),
                    );
                }
                // Bottom padding
                lines.push(Line::from("").style(bg));
                lines
            }
            HistoryCell::Assistant { text, streaming } => {
                let mut lines = if text.is_empty() && *streaming {
                    if let Some(state) = throbber_state {
                        let throbber = Throbber::default()
                            .throbber_set(BRAILLE_EIGHT)
                            .throbber_style(theme::dim());
                        vec![Line::from(throbber.to_symbol_span(state))]
                    } else {
                        vec![Line::from(Span::styled(
                            format!("{} ...", theme::BULLET),
                            theme::dim(),
                        ))]
                    }
                } else {
                    let prefix_span = Span::styled(format!("{} ", theme::BULLET), theme::dim());
                    let md_lines = markdown::render_markdown(text, width.saturating_sub(2));
                    if md_lines.is_empty() {
                        vec![Line::from(prefix_span)]
                    } else {
                        let mut result = Vec::with_capacity(md_lines.len());
                        for (i, mut line) in md_lines.into_iter().enumerate() {
                            if i == 0 {
                                let mut spans = vec![prefix_span.clone()];
                                spans.extend(line.spans);
                                result.push(Line::from(spans));
                            } else {
                                let mut spans = vec![Span::raw("  ")];
                                spans.append(&mut line.spans);
                                result.push(Line::from(spans));
                            }
                        }
                        result
                    }
                };
                if *streaming {
                    lines.push(Line::from(Span::styled("▊", theme::dim())));
                } else {
                    lines.push(Line::default());
                }
                lines
            }
            HistoryCell::ToolStart { name, args } => {
                let preview = if name == "apply_patch" || name == "apply_skill_patch" {
                    // Hide verbose patch content; just show a short summary
                    let file_count = args.matches("*** Add File:").count()
                        + args.matches("*** Update File:").count()
                        + args.matches("*** Delete File:").count();
                    if file_count > 0 {
                        format!("({file_count} file(s))")
                    } else {
                        String::new()
                    }
                } else if args.len() > 80 {
                    format!("{}...", truncate_str(args, 77))
                } else {
                    args.clone()
                };
                vec![Line::from(vec![
                    Span::styled(format!("  {} Running ", theme::BULLET), theme::tool_style()),
                    Span::styled(name.clone(), theme::tool_style()),
                    Span::styled(format!(" {preview}"), theme::dim()),
                ])]
            }
            HistoryCell::ToolResult {
                output, is_error, ..
            } => {
                let style = if *is_error {
                    theme::error_style()
                } else {
                    theme::success_style()
                };
                let preview_lines: Vec<&str> = output.lines().take(5).collect();
                let truncated = output.lines().count() > 5;
                let mut lines: Vec<Line<'static>> = Vec::new();
                for (i, pl) in preview_lines.iter().enumerate() {
                    let prefix = if i == 0 {
                        format!("  {} ", theme::TREE_END)
                    } else {
                        "    ".to_string()
                    };
                    let text = if pl.len() > 200 {
                        format!("{}...", truncate_str(pl, 197))
                    } else {
                        pl.to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(text, style),
                    ]));
                }
                if truncated {
                    lines.push(Line::from(Span::styled("    ...", theme::dim())));
                }
                lines
            }
            HistoryCell::ShellApproval { command, status } => {
                let status_text = match status {
                    ApprovalStatus::Pending => "[y/N]",
                    ApprovalStatus::Approved => "[approved]",
                    ApprovalStatus::Denied => "[denied]",
                };
                let status_style = match status {
                    ApprovalStatus::Pending => theme::error_style(),
                    ApprovalStatus::Approved => theme::success_style(),
                    ApprovalStatus::Denied => theme::dim(),
                };
                vec![
                    Line::from(vec![
                        Span::styled("  [run_shell] ", theme::error_style()),
                        Span::styled(command.clone(), theme::error_style()),
                    ]),
                    Line::from(vec![
                        Span::raw("  Allow? "),
                        Span::styled(status_text.to_string(), status_style),
                    ]),
                ]
            }
            HistoryCell::ToolApproval {
                tool_name,
                reason,
                status,
            } => {
                let status_text = match status {
                    ApprovalStatus::Pending => "[y/N]",
                    ApprovalStatus::Approved => "[approved]",
                    ApprovalStatus::Denied => "[denied]",
                };
                let status_style = match status {
                    ApprovalStatus::Pending => theme::error_style(),
                    ApprovalStatus::Approved => theme::success_style(),
                    ApprovalStatus::Denied => theme::dim(),
                };
                vec![
                    Line::from(vec![
                        Span::styled(format!("  [{tool_name}] "), theme::error_style()),
                        Span::styled(reason.clone(), theme::error_style()),
                    ]),
                    Line::from(vec![
                        Span::raw("  Allow? "),
                        Span::styled(status_text.to_string(), status_style),
                    ]),
                ]
            }
            HistoryCell::Heartbeat { text } => {
                vec![
                    Line::from(vec![
                        Span::styled("[heartbeat] ", theme::code_style()),
                        Span::styled(text.clone(), theme::code_style()),
                    ]),
                    Line::default(),
                ]
            }
            HistoryCell::System { text } => {
                let mut lines: Vec<Line<'static>> = text
                    .lines()
                    .map(|l| Line::from(Span::styled(l.to_string(), theme::dim())))
                    .collect();
                lines.push(Line::default());
                lines
            }
            HistoryCell::Thinking { text } => {
                let style = ratatui::style::Style::default()
                    .fg(ratatui::style::Color::DarkGray)
                    .add_modifier(ratatui::style::Modifier::ITALIC);
                let mut lines = vec![Line::from(Span::styled("thinking...", style))];
                for l in text.lines() {
                    lines.push(Line::from(Span::styled(l.to_string(), style)));
                }
                lines
            }
            HistoryCell::ToolStreaming {
                lines: tool_lines, ..
            } => {
                let _ = width;
                let mut rendered: Vec<Line<'static>> = Vec::new();
                let total = tool_lines.len();
                let max_visible = 8;
                let skip = total.saturating_sub(max_visible);
                if skip > 0 {
                    rendered.push(Line::from(Span::styled(
                        format!("  ... ({skip} lines above)"),
                        theme::dim(),
                    )));
                }
                for (line_text, is_stderr) in tool_lines.iter().skip(skip) {
                    let style = if *is_stderr {
                        theme::error_style()
                    } else {
                        theme::dim()
                    };
                    let display = if line_text.len() > 200 {
                        format!("{}...", truncate_str(line_text, 197))
                    } else {
                        line_text.clone()
                    };
                    let prefix = if *is_stderr { "! " } else { "\u{2502} " };
                    rendered.push(Line::from(vec![
                        Span::styled(format!("  {prefix}"), style),
                        Span::styled(display, style),
                    ]));
                }
                rendered
            }
        }
    }
}
