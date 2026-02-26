use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::markdown::render_markdown_lines;
use super::{ChatMessage, ChatState, SPINNER_FRAMES, THINKING_VERBS, TOOL_RESULT_MAX_LEN};
use crate::ui::styles;

pub(super) fn render_chat(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    lines.extend(app.welcome_lines.clone());

    for (idx, msg) in app.messages.iter().enumerate() {
        let is_current_match = app.search_mode
            && !app.search_matches.is_empty()
            && app.search_matches.get(app.search_match_idx) == Some(&idx);
        let is_match = app.search_mode && app.search_matches.contains(&idx);
        let highlight_style = if is_current_match {
            Style::new().bg(Color::Rgb(100, 100, 0))
        } else if is_match {
            Style::new().bg(Color::Rgb(40, 40, 0))
        } else {
            Style::default()
        };

        match msg {
            ChatMessage::User(text) => {
                lines.push(Line::default());
                for line in text.lines() {
                    let spans = vec![
                        Span::styled("You: ", styles::user_style()),
                        Span::styled(line, highlight_style),
                    ];
                    lines.push(Line::from(spans));
                }
            }
            ChatMessage::Assistant(text) => {
                lines.push(Line::default());
                let md_lines = render_markdown_lines(text);
                let mut first = true;
                for line in md_lines {
                    if first {
                        let mut spans =
                            vec![Span::styled("Assistant: ", styles::assistant_style())];
                        if is_match {
                            let highlighted_spans: Vec<Span> = line
                                .spans
                                .into_iter()
                                .map(|s| Span::styled(s.content, s.style.patch(highlight_style)))
                                .collect();
                            spans.extend(highlighted_spans);
                        } else {
                            spans.extend(line.spans);
                        }
                        lines.push(Line::from(spans));
                        first = false;
                    } else if is_match {
                        let highlighted_spans: Vec<Span> = line
                            .spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.patch(highlight_style)))
                            .collect();
                        lines.push(Line::from(highlighted_spans));
                    } else {
                        lines.push(line);
                    }
                }
            }
            ChatMessage::ToolUse(name, args) => {
                let args_short = if args.len() > 80 {
                    format!("{}...", &args[..77])
                } else {
                    args.clone()
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        "  🔧 ",
                        Style::new().fg(Color::Yellow).patch(highlight_style),
                    ),
                    Span::styled(
                        name.as_str(),
                        Style::new()
                            .add_modifier(Modifier::BOLD)
                            .patch(highlight_style),
                    ),
                    Span::styled(
                        format!("({args_short})"),
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
                ]));
            }
            ChatMessage::ToolResult(name, content) => {
                let truncated = if content.len() > TOOL_RESULT_MAX_LEN {
                    format!("{}...", &content[..TOOL_RESULT_MAX_LEN - 3])
                } else {
                    content.clone()
                };
                let first_line = truncated.lines().next().unwrap_or("").to_string();
                lines.push(Line::from(vec![
                    Span::styled(
                        "  ← ",
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
                    Span::styled(
                        format!("{name}: "),
                        Style::new()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC)
                            .patch(highlight_style),
                    ),
                    Span::styled(
                        first_line,
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
                ]));
            }
            ChatMessage::System(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::system_style_r().patch(highlight_style),
                )));
            }
            ChatMessage::Error(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::error_style_r().patch(highlight_style),
                )));
            }
        }
    }

    // Feedback controls (after last completed assistant message, when not streaming)
    if !app.is_streaming() && !app.is_waiting {
        if let Some(ref _url) = app.metrics.last_feedback_url {
            match app.feedback_submitted {
                None => {
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled(
                            "[+]",
                            Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" thumbs up  ", Style::new().fg(Color::DarkGray)),
                        Span::styled(
                            "[-]",
                            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" thumbs down", Style::new().fg(Color::DarkGray)),
                    ]));
                }
                Some(true) => {
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled(
                            "[+] ",
                            Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("voted!", Style::new().fg(Color::Green)),
                    ]));
                }
                Some(false) => {
                    lines.push(Line::from(vec![
                        Span::raw("           "),
                        Span::styled(
                            "[-] ",
                            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("voted!", Style::new().fg(Color::Red)),
                    ]));
                }
            }
        }
    }

    // Streaming content
    if !app.streaming_text.is_empty() {
        lines.push(Line::default());
        let md_lines = render_markdown_lines(&app.streaming_text);
        let mut first = true;
        for line in md_lines {
            if first {
                let mut spans = vec![Span::styled("Assistant: ", styles::assistant_style())];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
                first = false;
            } else {
                lines.push(line);
            }
        }
        if app.streaming_text.ends_with('\n') {
            lines.push(Line::raw(""));
        }
    } else if app.is_waiting {
        let frame_idx = app.spinner_idx % SPINNER_FRAMES.len();
        let spinner = SPINNER_FRAMES[frame_idx];
        let verb_idx = (app.spinner_idx / 8) % THINKING_VERBS.len();
        let verb = THINKING_VERBS[verb_idx];
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("{spinner} {verb}..."),
            styles::system_style_r(),
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  tip: {}", super::helpers::tip_for_tick(app.spinner_idx)),
            Style::new()
                .fg(Color::Rgb(80, 80, 80))
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let max_scroll = compute_auto_scroll(&lines, area);
    // Clamp scroll_offset so user can't scroll past the top
    if app.scroll_offset > max_scroll {
        app.scroll_offset = max_scroll;
    }
    let scroll = if app.auto_scroll {
        max_scroll
    } else {
        max_scroll.saturating_sub(app.scroll_offset)
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE))
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

fn compute_auto_scroll(lines: &[Line], area: Rect) -> u16 {
    let width = area.width.max(1) as usize;
    let mut total: u16 = 0;
    for line in lines {
        let line_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
        if line_len == 0 {
            total += 1;
        } else {
            total += line_len.div_ceil(width) as u16;
        }
    }
    let visible = area.height;
    if total > visible {
        total.saturating_sub(visible)
    } else {
        0
    }
}

pub(super) fn render_input(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(ratatui::style::Style::new().dark_gray());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(&app.textarea, inner);

    if app.show_complete && !app.completions.is_empty() {
        let items: Vec<Line> = app
            .completions
            .iter()
            .enumerate()
            .map(|(i, item)| {
                if i == app.completion_idx {
                    Line::from(vec![
                        Span::styled(format!(" > {} ", item.label), styles::user_style()),
                        Span::styled(item.desc.clone(), styles::system_style_r()),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(format!("   {} ", item.label)),
                        Span::styled(item.desc.clone(), styles::system_style_r()),
                    ])
                }
            })
            .collect();

        let popup_height = items.len() as u16 + 2;
        let popup_width = 50.min(area.width);
        let popup_area = Rect {
            x: area.x,
            y: area.y.saturating_sub(popup_height),
            width: popup_width,
            height: popup_height,
        };

        let popup = Paragraph::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(ratatui::style::Style::new().dark_gray()),
        );
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        frame.render_widget(popup, popup_area);
    }
}

pub(super) fn render_devtools(frame: &mut ratatui::Frame, app: &ChatState, area: Rect) {
    let bg = Style::new().fg(Color::White).bg(Color::Rgb(40, 40, 40));
    let dim = Style::new().fg(Color::DarkGray).bg(Color::Rgb(40, 40, 40));

    // Line 1: Metrics
    let mut line1: Vec<Span> = vec![Span::styled(" devtools ", styles::user_style())];

    if app.is_streaming() || app.is_waiting {
        if let Some(started) = app.metrics.run_started_at {
            let elapsed = started.elapsed().as_millis();
            if let Some(first) = app.metrics.first_token_at {
                let ttft = first.duration_since(started).as_millis();
                line1.push(Span::raw(format!("TTFT: {}ms ", ttft)));
                let stream_dur = first.elapsed().as_secs_f64();
                if stream_dur > 0.0 && app.metrics.token_count > 1 {
                    let tps = (app.metrics.token_count - 1) as f64 / stream_dur;
                    line1.push(Span::raw(format!("{:.0} tok/s ", tps)));
                }
                line1.push(Span::raw(format!("tokens: {} ", app.metrics.token_count)));
            } else {
                line1.push(Span::raw(format!("waiting: {}ms ", elapsed)));
            }
        }
    } else if app.metrics.last_total_ms.is_some() {
        if let Some(ttft) = app.metrics.last_ttft_ms {
            line1.push(Span::raw(format!("TTFT: {}ms ", ttft)));
        }
        if let Some(tps) = app.metrics.last_tokens_per_sec {
            line1.push(Span::raw(format!("{:.0} tok/s ", tps)));
        }
        if let Some(total) = app.metrics.last_total_ms {
            line1.push(Span::raw(format!("total: {}ms ", total)));
        }
        if let Some(count) = app.metrics.last_token_count {
            line1.push(Span::raw(format!("tokens: {} ", count)));
        }
    }

    if let Some(rid) = app
        .metrics
        .run_id
        .as_deref()
        .or(app.metrics.last_run_id.as_deref())
    {
        let short = if rid.len() > 8 { &rid[..8] } else { rid };
        line1.push(Span::styled(
            format!("run:{short}"),
            styles::system_style_r(),
        ));
    }

    // Line 2: Tool timeline
    let timeline = if app.is_streaming() || app.is_waiting {
        &app.metrics.tool_timeline
    } else {
        &app.metrics.last_tool_timeline
    };
    let mut line2: Vec<Span> = vec![Span::styled(" tools ", dim)];
    if timeline.is_empty() {
        line2.push(Span::styled("none", dim));
    } else {
        for (i, tool) in timeline.iter().enumerate() {
            if i > 0 {
                line2.push(Span::styled(" > ", dim));
            }
            let name_style = Style::new().fg(Color::Cyan).bg(Color::Rgb(40, 40, 40));
            if let Some(ms) = tool.duration_ms {
                line2.push(Span::styled(tool.name.to_string(), name_style));
                line2.push(Span::styled(
                    format!(" {}ms", ms),
                    Style::new().fg(Color::Green).bg(Color::Rgb(40, 40, 40)),
                ));
            } else {
                line2.push(Span::styled(format!("{}...", tool.name), name_style));
            }
        }
    }

    // Line 3: Node + trace link
    let node_history = if app.is_streaming() || app.is_waiting {
        &app.metrics.node_history
    } else {
        &app.metrics.last_node_history
    };
    let mut line3: Vec<Span> = Vec::new();
    if !node_history.is_empty() {
        line3.push(Span::styled(" nodes ", dim));
        line3.push(Span::styled(
            node_history.join(" > "),
            Style::new().fg(Color::Magenta).bg(Color::Rgb(40, 40, 40)),
        ));
        line3.push(Span::raw("  "));
    } else {
        line3.push(Span::styled(" ", dim));
    }
    if let Some(rid) = app
        .metrics
        .run_id
        .as_deref()
        .or(app.metrics.last_run_id.as_deref())
    {
        if let Some(tid) = &app.tenant_id {
            let url = if let Some(sid) = &app.tracer_session_id {
                format!(
                    "https://smith.langchain.com/o/{tid}/projects/p/{sid}/r/{rid}?trace_id={rid}"
                )
            } else {
                format!("https://smith.langchain.com/o/{tid}/r/{rid}")
            };
            line3.push(Span::styled(
                format!("trace: {url}"),
                Style::new().fg(Color::Blue).bg(Color::Rgb(40, 40, 40)),
            ));
        }
    }

    // Line 4: Console (last debug log line)
    let log_lines = crate::debug_log::tail(1);
    let mut line4: Vec<Span> = vec![Span::styled(" console ", dim)];
    if let Some(last) = log_lines.last() {
        line4.push(Span::styled(
            last.to_string(),
            Style::new().fg(Color::DarkGray).bg(Color::Rgb(40, 40, 40)),
        ));
    }

    let text = ratatui::text::Text::from(vec![
        Line::from(line1),
        Line::from(line2),
        Line::from(line3),
        Line::from(line4),
    ]);
    let bar = Paragraph::new(text).style(bg);
    frame.render_widget(bar, area);
}

pub(super) fn render_status(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
    if app.scroll_mode {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

        let scroll_line = Line::from(Span::styled(
            " SCROLL  ↑↓/jk line  PgUp/PgDn page  Home/End  q/Esc exit",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
        let scroll_bar = Paragraph::new(scroll_line).style(styles::status_bar_style());
        frame.render_widget(scroll_bar, chunks[0]);

        render_status_bar(frame, app, chunks[1]);
    } else if app.search_mode {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

        let search_text = if app.search_matches.is_empty() {
            format!(" Search: {} (no matches)", app.search_query)
        } else {
            format!(
                " Search: {} ({}/{} ↑↓ to navigate)",
                app.search_query,
                app.search_match_idx + 1,
                app.search_matches.len()
            )
        };
        let search_line = Line::from(Span::styled(
            search_text,
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        let search_bar = Paragraph::new(search_line).style(styles::status_bar_style());
        frame.render_widget(search_bar, chunks[0]);

        render_status_bar(frame, app, chunks[1]);
    } else {
        render_status_bar(frame, app, area);
    }
}

fn render_status_bar(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
    let face = app.parrot.mini_face();
    let mut left_parts: Vec<Span> = vec![
        Span::raw(" "),
        face,
        Span::raw(" "),
        Span::raw(&app.context_name),
    ];

    left_parts.push(Span::styled(
        format!(" | {}", app.assistant_id),
        Style::new().fg(Color::DarkGray),
    ));

    if app.stream_mode != "messages-tuple" {
        left_parts.push(Span::styled(
            format!(" | mode:{}", app.stream_mode),
            Style::new().fg(Color::Yellow),
        ));
    }

    if !app.pending_attachments.is_empty() {
        left_parts.push(Span::styled(
            format!(" | {} attached", app.pending_attachments.len()),
            Style::new().fg(Color::Yellow),
        ));
    }

    if app.interrupted {
        left_parts.push(Span::styled(
            " | PAUSED",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }

    if !app.pending_messages.is_empty() {
        let n = app.pending_messages.len();
        left_parts.push(Span::raw(format!(" | {} queued", n)));
    }

    if !app.devtools {
        if let Some(rid) = app
            .metrics
            .last_run_id
            .as_deref()
            .or(app.metrics.run_id.as_deref())
        {
            let short = if rid.len() > 8 { &rid[..8] } else { rid };
            left_parts.push(Span::styled(
                format!(" | trace:{short}"),
                Style::new().fg(Color::Blue),
            ));
        }
    }

    let msg_count = app
        .messages
        .iter()
        .filter(|m| {
            matches!(
                m,
                ChatMessage::User(_) | ChatMessage::Assistant(_)
            )
        })
        .count();
    if msg_count > 0 {
        left_parts.push(Span::styled(
            format!(" | {msg_count} msgs"),
            Style::new().fg(Color::DarkGray),
        ));
    }

    let right = if let Some(notice) = &app.update_notice {
        notice.clone()
    } else {
        String::new()
    };

    let left_text: String = left_parts.iter().map(|s| s.content.as_ref()).collect();
    let left_len = left_text.len();
    let right_len = right.len();
    let padding = (area.width as usize).saturating_sub(left_len + right_len + 1);

    let mut spans = left_parts;
    spans.push(Span::raw(" ".repeat(padding)));
    if !right.is_empty() {
        spans.push(Span::raw(right));
        spans.push(Span::raw(" "));
    }

    let line = Line::from(spans);
    let status = Paragraph::new(line).style(styles::status_bar_style());
    frame.render_widget(status, area);
}
