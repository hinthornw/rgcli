use chrono::{DateTime, Local, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::api::Client;
use crate::api::types::{Message, ThreadState, get_messages};
use crate::ui::screen::{Screen, ScreenAction, ScreenContext};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Rows(Vec<Vec<String>>),
    Error(String),
    Detail(ThreadState),
    DetailError(String),
}

pub struct ThreadsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
    thread_ids: Vec<String>,

    // Detail pane state
    detail_thread_id: Option<String>,
    detail_state: Option<ThreadState>,
    detail_messages: Vec<Message>,
    detail_loading: bool,
    detail_error: Option<String>,
    detail_scroll: u16,
}

impl ThreadsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new(
                "Threads",
                vec![
                    Column {
                        name: "ID".to_string(),
                        width_pct: 15,
                    },
                    Column {
                        name: "Status".to_string(),
                        width_pct: 10,
                    },
                    Column {
                        name: "Created".to_string(),
                        width_pct: 18,
                    },
                    Column {
                        name: "Updated".to_string(),
                        width_pct: 18,
                    },
                    Column {
                        name: "Last Message".to_string(),
                        width_pct: 39,
                    },
                ],
            ),
            loaded: false,
            async_rx: None,
            thread_ids: Vec::new(),
            detail_thread_id: None,
            detail_state: None,
            detail_messages: Vec::new(),
            detail_loading: false,
            detail_error: None,
            detail_scroll: 0,
        }
    }

    pub fn on_enter(&mut self, client: &Client) {
        if self.loaded {
            return;
        }
        self.table.loading = true;
        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        let client = client.clone();
        tokio::spawn(async move {
            match client.search_threads(50).await {
                Ok(threads) => {
                    let rows: Vec<Vec<String>> = threads
                        .iter()
                        .map(|t| {
                            let id_short: String = t.thread_id.chars().take(12).collect();
                            let status = t
                                .status
                                .as_deref()
                                .unwrap_or("-")
                                .to_string();
                            let created = format_local_time(t.created_at.as_deref());
                            let updated = format_local_time(t.updated_at.as_deref());
                            let last_msg = extract_last_message(&t.values);
                            // Append full ID as hidden column
                            vec![id_short, status, created, updated, last_msg, t.thread_id.clone()]
                        })
                        .collect();
                    let _ = tx.send(AsyncResult::Rows(rows));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    fn open_detail(&mut self, thread_id: &str, client: &Client) {
        self.detail_thread_id = Some(thread_id.to_string());
        self.detail_loading = true;
        self.detail_state = None;
        self.detail_messages.clear();
        self.detail_error = None;
        self.detail_scroll = 0;

        let client = client.clone();
        let tid = thread_id.to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        // Drop old receiver if any
        self.async_rx = Some(rx);

        tokio::spawn(async move {
            match client.get_thread_state(&tid).await {
                Ok(state) => {
                    let _ = tx.send(AsyncResult::Detail(state));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::DetailError(e.to_string()));
                }
            }
        });
    }

    fn close_detail(&mut self) {
        self.detail_thread_id = None;
        self.detail_state = None;
        self.detail_messages.clear();
        self.detail_loading = false;
        self.detail_error = None;
        self.detail_scroll = 0;
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &mut self.async_rx {
            while let Ok(result) = rx.try_recv() {
                match result {
                    AsyncResult::Rows(rows) => {
                        self.thread_ids = rows
                            .iter()
                            .map(|r| r.get(5).cloned().unwrap_or_default())
                            .collect();
                        let display_rows: Vec<Vec<String>> = rows
                            .into_iter()
                            .map(|mut r| {
                                r.truncate(5);
                                r
                            })
                            .collect();
                        self.table.set_rows(display_rows);
                        self.loaded = true;
                    }
                    AsyncResult::Error(e) => {
                        self.table.set_error(e);
                        self.loaded = true;
                    }
                    AsyncResult::Detail(state) => {
                        self.detail_messages = get_messages(&state.values);
                        self.detail_state = Some(state);
                        self.detail_loading = false;
                    }
                    AsyncResult::DetailError(e) => {
                        self.detail_error = Some(e);
                        self.detail_loading = false;
                    }
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, client: &Client) -> ScreenAction {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return ScreenAction::Quit;
        }
        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return ScreenAction::Quit;
        }

        // Detail pane open — handle detail keys first
        if self.detail_thread_id.is_some() {
            match key.code {
                KeyCode::Enter | KeyCode::Char('o') => {
                    if let Some(tid) = self.detail_thread_id.clone() {
                        self.close_detail();
                        return ScreenAction::NavigateWithContext(
                            Screen::Chat,
                            ScreenContext::Thread(tid),
                        );
                    }
                }
                KeyCode::Esc => {
                    self.close_detail();
                    return ScreenAction::None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                    return ScreenAction::None;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                    return ScreenAction::None;
                }
                KeyCode::PageUp => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(10);
                    return ScreenAction::None;
                }
                KeyCode::PageDown => {
                    self.detail_scroll = self.detail_scroll.saturating_add(10);
                    return ScreenAction::None;
                }
                _ => return ScreenAction::None,
            }
        }

        if self.table.handle_key(key) {
            return ScreenAction::None;
        }

        match key.code {
            KeyCode::Enter => {
                if let Some(sel) = self.table.state.selected() {
                    if let Some(full_id) = self.thread_ids.get(sel).cloned() {
                        self.open_detail(&full_id, client);
                    }
                }
                ScreenAction::None
            }
            KeyCode::Char('r') => {
                self.loaded = false;
                self.on_enter(client);
                ScreenAction::None
            }
            KeyCode::Char('q') | KeyCode::Esc => ScreenAction::Navigate(Screen::Chat),
            _ => ScreenAction::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if self.detail_thread_id.is_some() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(area);

            self.table.render(frame, chunks[0]);
            self.render_detail(frame, chunks[1]);
        } else {
            self.table.render(frame, area);
        }
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect) {
        let thread_id = self.detail_thread_id.as_deref().unwrap_or("?");
        let title = format!(" Thread {thread_id} ");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.detail_loading {
            let loading = Paragraph::new("Loading thread state...");
            frame.render_widget(loading, inner);
            return;
        }

        if let Some(err) = &self.detail_error {
            let err_p =
                Paragraph::new(format!("Error: {err}")).style(Style::default().fg(Color::Red));
            frame.render_widget(err_p, inner);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Status line
        if let Some(state) = &self.detail_state {
            let status = if let Some(next) = &state.next {
                if next.is_empty() {
                    "idle".to_string()
                } else {
                    format!("next: {}", next.join(", "))
                }
            } else {
                "idle".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::raw(status),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Messages: ", Style::default().fg(Color::DarkGray)),
                Span::raw(self.detail_messages.len().to_string()),
            ]));
            lines.push(Line::from(""));
        }

        // Messages
        for msg in &self.detail_messages {
            let (label, color) = match msg.role.as_str() {
                "user" => ("You", Color::Green),
                "assistant" | "ai" => ("Assistant", Color::Cyan),
                "tool" => ("Tool", Color::Yellow),
                other => (other, Color::DarkGray),
            };

            // Tool calls
            for tc in &msg.tool_calls {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{label}: "),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}({})", tc.name, truncate_str(&tc.args, 60)),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }

            // Content
            if !msg.content.is_empty() {
                let content = truncate_str(&msg.content, 200);
                let prefix = if let Some(tool_name) = &msg.tool_name {
                    format!("{label} [{tool_name}]: ")
                } else {
                    format!("{label}: ")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(content),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from("(no messages)"));
        }

        // Footer
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan)),
            Span::raw("Open in Chat  "),
            Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Close"),
        ]));

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.detail_scroll, 0));
        frame.render_widget(para, inner);
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        format!("{}…", &first_line[..max])
    } else if s.lines().count() > 1 {
        format!("{first_line}…")
    } else {
        first_line.to_string()
    }
}

/// Parse an ISO-8601 timestamp and format it in local time.
fn format_local_time(ts: Option<&str>) -> String {
    let Some(ts) = ts else {
        return "-".to_string();
    };
    ts.parse::<DateTime<Utc>>()
        .map(|utc| {
            let local: DateTime<Local> = utc.into();
            local.format("%Y-%m-%d %H:%M").to_string()
        })
        .unwrap_or_else(|_| ts.to_string())
}

/// Extract a short preview of the last message from thread values.
fn extract_last_message(values: &Option<serde_json::Value>) -> String {
    let Some(vals) = values else {
        return "-".to_string();
    };
    // values.messages is typically an array of message objects
    let messages = vals
        .get("messages")
        .and_then(|m| m.as_array())
        .or_else(|| {
            // Some threads have values as an array directly
            vals.as_array()
        });
    let Some(msgs) = messages else {
        return "-".to_string();
    };
    // Find the last message with content
    for msg in msgs.iter().rev() {
        if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                let role = msg
                    .get("type")
                    .or_else(|| msg.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("");
                let prefix = match role {
                    "human" | "user" => "User: ",
                    "ai" | "assistant" => "AI: ",
                    "tool" => "Tool: ",
                    _ => "",
                };
                return truncate_str(&format!("{prefix}{content}"), 80);
            }
        }
        // Handle content as array (e.g. multimodal messages)
        if let Some(parts) = msg.get("content").and_then(|c| c.as_array()) {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        let role = msg
                            .get("type")
                            .or_else(|| msg.get("role"))
                            .and_then(|r| r.as_str())
                            .unwrap_or("");
                        let prefix = match role {
                            "human" | "user" => "User: ",
                            "ai" | "assistant" => "AI: ",
                            _ => "",
                        };
                        return truncate_str(&format!("{prefix}{text}"), 80);
                    }
                }
            }
        }
    }
    "-".to_string()
}
