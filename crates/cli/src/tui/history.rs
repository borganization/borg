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
        completed: bool,
        start_time: Option<std::time::Instant>,
    },
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
        duration_ms: Option<u64>,
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
    Separator,
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
                let mention_style = theme::file_mention_style();
                let mut lines = Vec::new();
                lines.push(Line::from("").style(bg));
                for (i, line) in text.lines().enumerate() {
                    let prefix = if i == 0 {
                        Span::styled(format!("{} ", theme::CHEVRON), prefix_style)
                    } else {
                        Span::styled("  ", bg)
                    };
                    let spans = parse_at_mentions(line, bg, mention_style);
                    let mut all_spans = vec![prefix];
                    all_spans.extend(spans);
                    lines.push(Line::from(all_spans).style(bg));
                }
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
            HistoryCell::ToolStart {
                name,
                args,
                completed,
                ..
            } => {
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
                let bullet_style = if *completed {
                    theme::tool_bullet_done()
                } else {
                    theme::tool_bullet_active()
                };
                let name_style = theme::code_style().add_modifier(ratatui::style::Modifier::BOLD);
                vec![Line::from(vec![
                    Span::styled(format!("{} ", theme::BULLET), bullet_style),
                    Span::styled(name.clone(), name_style),
                    Span::styled(format!(" {preview}"), theme::dim()),
                ])]
            }
            HistoryCell::ToolResult {
                name,
                output,
                is_error,
                duration_ms,
            } => {
                let style = if *is_error {
                    theme::error_style()
                } else {
                    theme::dim()
                };
                let preview_lines: Vec<&str> = output.lines().take(5).collect();
                let total_count = output.lines().count();
                let truncated = total_count > 5;
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
                    let extra = total_count - 5;
                    lines.push(Line::from(Span::styled(
                        format!("    {} +{extra} more lines", theme::ELLIPSIS),
                        theme::dim(),
                    )));
                }
                // Status line with check/cross and duration
                let (indicator, ind_style) = if *is_error {
                    (theme::CROSS, theme::cross_style())
                } else {
                    (theme::CHECK, theme::check_style())
                };
                let duration_str = match duration_ms {
                    Some(ms) => {
                        let secs_f = *ms as f64 / 1000.0;
                        format!(" • {secs_f:.1}s")
                    }
                    None => String::new(),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {indicator} "), ind_style),
                    Span::styled(format!("Ran {name}{duration_str}"), theme::dim()),
                ]));
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
                // Don't render an empty thinking box
                if text.is_empty() {
                    return vec![];
                }
                let border = theme::thinking_border_style();
                let content_style = ratatui::style::Style::default()
                    .fg(ratatui::style::Color::DarkGray)
                    .add_modifier(ratatui::style::Modifier::ITALIC);
                let inner_w = (width as usize).saturating_sub(4);
                let label = " thinking ";
                let top_rule_len = inner_w.saturating_sub(label.len());
                let top = format!(
                    "{}{}{label}{}{}",
                    theme::BOX_TOP_LEFT,
                    theme::SEPARATOR.repeat(2),
                    theme::SEPARATOR.repeat(top_rule_len),
                    theme::BOX_TOP_RIGHT,
                );
                let mut lines = vec![Line::from(Span::styled(top, border))];
                for l in text.lines() {
                    let display = truncate_str(l, inner_w);
                    let display_len = unicode_width::UnicodeWidthStr::width(display);
                    let pad = inner_w.saturating_sub(display_len);
                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", theme::BOX_VERTICAL), border),
                        Span::styled(display.to_string(), content_style),
                        Span::styled(
                            format!("{}{}", " ".repeat(pad), theme::BOX_VERTICAL),
                            border,
                        ),
                    ]));
                }
                let bottom = format!(
                    "{}{}{}",
                    theme::BOX_BOTTOM_LEFT,
                    theme::SEPARATOR.repeat(inner_w + 2),
                    theme::BOX_BOTTOM_RIGHT,
                );
                lines.push(Line::from(Span::styled(bottom, border)));
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
            HistoryCell::Separator => {
                let rule_width = ((width as usize) * 2 / 3).min(80);
                vec![
                    Line::default(),
                    Line::from(Span::styled(
                        theme::SEPARATOR.repeat(rule_width),
                        theme::dim(),
                    )),
                    Line::default(),
                ]
            }
        }
    }
}

/// Split a line into spans, highlighting `@path` tokens with the mention style.
fn parse_at_mentions(
    line: &str,
    normal: ratatui::style::Style,
    mention: ratatui::style::Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = line;

    while let Some(at_pos) = rest.find('@') {
        // Check word boundary: must be at start or preceded by whitespace
        let at_word_boundary = at_pos == 0 || rest.as_bytes()[at_pos - 1] == b' ';
        if !at_word_boundary {
            // Not a mention, consume up to and including the '@'
            spans.push(Span::styled(rest[..at_pos + 1].to_string(), normal));
            rest = &rest[at_pos + 1..];
            continue;
        }

        // Push text before the @
        if at_pos > 0 {
            spans.push(Span::styled(rest[..at_pos].to_string(), normal));
        }

        // Find end of mention (next space or end of string)
        let after_at = &rest[at_pos + 1..];
        let end = after_at.find(' ').unwrap_or(after_at.len());
        if end == 0 {
            // Bare '@' with no path
            spans.push(Span::styled("@".to_string(), normal));
            rest = after_at;
            continue;
        }

        let mention_text = format!("@{}", &after_at[..end]);
        spans.push(Span::styled(mention_text, mention));
        rest = &after_at[end..];
    }

    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), normal));
    }

    spans
}
