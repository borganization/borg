use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme;

/// Convert a markdown string into styled ratatui Lines.
pub fn render_markdown(input: &str, width: u16) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(input, opts);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut list_depth: usize = 0;
    let mut ordered_index: Option<u64> = None;
    let mut in_blockquote = false;

    let wrap_width = width.saturating_sub(2) as usize;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut current_spans, &mut lines);
                    let style = match level {
                        pulldown_cmark::HeadingLevel::H1 => {
                            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                        }
                        pulldown_cmark::HeadingLevel::H2 => {
                            Style::default().add_modifier(Modifier::BOLD)
                        }
                        _ => Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                    };
                    style_stack.push(style);
                }
                Tag::Paragraph => {
                    flush_line(&mut current_spans, &mut lines);
                }
                Tag::CodeBlock(_) => {
                    flush_line(&mut current_spans, &mut lines);
                    in_code_block = true;
                }
                Tag::Emphasis => {
                    let base = current_style(&style_stack);
                    style_stack.push(base.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    let base = current_style(&style_stack);
                    style_stack.push(base.add_modifier(Modifier::BOLD));
                }
                Tag::Strikethrough => {
                    let base = current_style(&style_stack);
                    style_stack.push(base.add_modifier(Modifier::CROSSED_OUT));
                }
                Tag::BlockQuote(_) => {
                    flush_line(&mut current_spans, &mut lines);
                    in_blockquote = true;
                    style_stack.push(theme::success_style());
                }
                Tag::List(start) => {
                    flush_line(&mut current_spans, &mut lines);
                    ordered_index = start;
                    list_depth += 1;
                }
                Tag::Item => {
                    flush_line(&mut current_spans, &mut lines);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    if let Some(ref mut idx) = ordered_index {
                        let bullet = format!("{indent}{idx}. ");
                        *idx += 1;
                        current_spans.push(Span::styled(bullet, theme::code_style()));
                    } else {
                        let bullet = format!("{indent}- ");
                        current_spans.push(Span::styled(bullet, current_style(&style_stack)));
                    };
                }
                Tag::Link { dest_url, .. } => {
                    let base = current_style(&style_stack);
                    style_stack.push(base.fg(theme::CYAN).add_modifier(Modifier::UNDERLINED));
                    // Store URL to append after text
                    let _ = dest_url;
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_line(&mut current_spans, &mut lines);
                    lines.push(Line::default());
                }
                TagEnd::Paragraph => {
                    flush_line(&mut current_spans, &mut lines);
                    lines.push(Line::default());
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    style_stack.pop();
                }
                TagEnd::BlockQuote(_) => {
                    in_blockquote = false;
                    style_stack.pop();
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    if list_depth == 0 {
                        ordered_index = None;
                    }
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::Item => {
                    flush_line(&mut current_spans, &mut lines);
                }
                _ => {}
            },
            Event::Text(text) => {
                let style = current_style(&style_stack);
                if in_blockquote && current_spans.is_empty() {
                    current_spans.push(Span::styled("│ ", theme::success_style()));
                }
                if in_code_block {
                    // Code blocks: no wrapping, use code style
                    for line in text.lines() {
                        current_spans.push(Span::styled(line.to_string(), theme::code_style()));
                        flush_line(&mut current_spans, &mut lines);
                    }
                } else {
                    // Wrap text
                    let wrapped = textwrap::fill(&text, wrap_width.max(20));
                    let text_lines: Vec<&str> = wrapped.lines().collect();
                    if text_lines.len() <= 1 {
                        current_spans.push(Span::styled(text.to_string(), style));
                    } else {
                        for (i, wl) in text_lines.iter().enumerate() {
                            current_spans.push(Span::styled(wl.to_string(), style));
                            if i < text_lines.len() - 1 {
                                flush_line(&mut current_spans, &mut lines);
                            }
                        }
                    }
                }
            }
            Event::Code(code) => {
                current_spans.push(Span::styled(format!("`{code}`"), theme::code_style()));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut current_spans, &mut lines);
            }
            Event::Rule => {
                flush_line(&mut current_spans, &mut lines);
                lines.push(Line::from(Span::styled(
                    "─".repeat(wrap_width.min(40)),
                    theme::dim(),
                )));
                lines.push(Line::default());
            }
            _ => {}
        }
    }
    flush_line(&mut current_spans, &mut lines);

    // Remove trailing empty lines
    while lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }

    lines
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

fn flush_line(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}
