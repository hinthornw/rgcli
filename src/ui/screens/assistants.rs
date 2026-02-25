use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::ui::screen::{Screen, ScreenAction, ScreenContext};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Rows(Vec<Vec<String>>, Vec<Value>),
    Error(String),
    Versions(Vec<Value>),
    VersionsError(String),
    Graph(String),
    GraphError(String),
}

pub struct AssistantsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
    assistant_data: Vec<Value>,

    // Detail pane
    detail_id: Option<String>,
    detail_data: Option<Value>,
    detail_versions: Vec<Value>,
    detail_graph: Option<String>,
    detail_loading: bool,
    detail_error: Option<String>,
    detail_scroll: u16,
}

impl AssistantsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new(
                "Assistants",
                vec![
                    Column {
                        name: "ID".to_string(),
                        width_pct: 30,
                    },
                    Column {
                        name: "Name".to_string(),
                        width_pct: 25,
                    },
                    Column {
                        name: "Graph".to_string(),
                        width_pct: 20,
                    },
                    Column {
                        name: "Updated".to_string(),
                        width_pct: 25,
                    },
                ],
            ),
            loaded: false,
            async_rx: None,
            assistant_data: Vec::new(),
            detail_id: None,
            detail_data: None,
            detail_versions: Vec::new(),
            detail_graph: None,
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
            match client.list_assistants().await {
                Ok(assistants) => {
                    let rows: Vec<Vec<String>> = assistants
                        .iter()
                        .map(|a| {
                            let id = a
                                .get("assistant_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            let name = a
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            let graph = a
                                .get("graph_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            let updated = a
                                .get("updated_at")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            vec![id, name, graph, updated]
                        })
                        .collect();
                    let _ = tx.send(AsyncResult::Rows(rows, assistants));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    fn open_detail(&mut self, idx: usize, client: &Client) {
        let Some(data) = self.assistant_data.get(idx).cloned() else {
            return;
        };
        let id = data
            .get("assistant_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            return;
        }

        self.detail_id = Some(id.clone());
        self.detail_data = Some(data);
        self.detail_versions.clear();
        self.detail_graph = None;
        self.detail_loading = true;
        self.detail_error = None;
        self.detail_scroll = 0;

        let client = client.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        tokio::spawn(async move {
            match client.get_assistant_versions(&id).await {
                Ok(versions) => {
                    let _ = tx.send(AsyncResult::Versions(versions));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::VersionsError(e.to_string()));
                }
            }
        });
    }

    fn fetch_graph(&mut self, client: &Client) {
        let Some(id) = self.detail_id.clone() else {
            return;
        };
        let client = client.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        tokio::spawn(async move {
            match client.get_assistant_graph(&id).await {
                Ok(graph) => {
                    let ascii = render_ascii_graph(&graph);
                    let _ = tx.send(AsyncResult::Graph(ascii));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::GraphError(e.to_string()));
                }
            }
        });
    }

    fn close_detail(&mut self) {
        self.detail_id = None;
        self.detail_data = None;
        self.detail_versions.clear();
        self.detail_graph = None;
        self.detail_loading = false;
        self.detail_error = None;
        self.detail_scroll = 0;
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &mut self.async_rx {
            while let Ok(result) = rx.try_recv() {
                match result {
                    AsyncResult::Rows(rows, data) => {
                        self.assistant_data = data;
                        self.table.set_rows(rows);
                        self.loaded = true;
                    }
                    AsyncResult::Error(e) => {
                        self.table.set_error(e);
                        self.loaded = true;
                    }
                    AsyncResult::Versions(versions) => {
                        self.detail_versions = versions;
                        self.detail_loading = false;
                    }
                    AsyncResult::VersionsError(e) => {
                        self.detail_error = Some(e);
                        self.detail_loading = false;
                    }
                    AsyncResult::Graph(ascii) => {
                        self.detail_graph = Some(ascii);
                    }
                    AsyncResult::GraphError(e) => {
                        self.detail_graph = Some(format!("Error: {e}"));
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

        // Detail pane keys
        if self.detail_id.is_some() {
            match key.code {
                KeyCode::Enter | KeyCode::Char('o') => {
                    if let Some(id) = self.detail_id.clone() {
                        self.close_detail();
                        return ScreenAction::NavigateWithContext(
                            Screen::Chat,
                            ScreenContext::Assistant(id),
                        );
                    }
                }
                KeyCode::Esc => {
                    self.close_detail();
                    return ScreenAction::None;
                }
                KeyCode::Char('g') => {
                    if self.detail_graph.is_none() {
                        self.fetch_graph(client);
                    }
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
                    self.open_detail(sel, client);
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
        if self.detail_id.is_some() {
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
        let id = self.detail_id.as_deref().unwrap_or("?");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Assistant {id} "))
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.detail_loading {
            let p = Paragraph::new("Loading...");
            frame.render_widget(p, inner);
            return;
        }

        if let Some(err) = &self.detail_error {
            let p =
                Paragraph::new(format!("Error: {err}")).style(Style::default().fg(Color::Red));
            frame.render_widget(p, inner);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        if let Some(data) = &self.detail_data {
            let name = data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let graph = data
                .get("graph_id")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let created = data
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let updated = data
                .get("updated_at")
                .and_then(|v| v.as_str())
                .unwrap_or("-");

            lines.push(Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
                Span::raw(name),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Graph: ", Style::default().fg(Color::DarkGray)),
                Span::raw(graph),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Created: ", Style::default().fg(Color::DarkGray)),
                Span::raw(created),
                Span::raw("  "),
                Span::styled("Updated: ", Style::default().fg(Color::DarkGray)),
                Span::raw(updated),
            ]));

            // Config
            if let Some(config) = data.get("config") {
                if !config.is_null() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Config:",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )));
                    let pretty =
                        serde_json::to_string_pretty(config).unwrap_or_else(|_| "{}".to_string());
                    for line in pretty.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            }

            // Metadata
            if let Some(meta) = data.get("metadata") {
                if meta.is_object() && !meta.as_object().unwrap().is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Metadata:",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )));
                    let pretty = serde_json::to_string_pretty(meta)
                        .unwrap_or_else(|_| "{}".to_string());
                    for line in pretty.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            }
        }

        // Versions
        if !self.detail_versions.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Versions: {}", self.detail_versions.len()),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            for (i, v) in self.detail_versions.iter().enumerate() {
                let num = v
                    .get("version")
                    .and_then(|v| v.as_i64())
                    .map(|n| format!("v{n}"))
                    .unwrap_or_else(|| "?".to_string());
                let created = v
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let active = if i == 0 { " (active)" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {num:<6}"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(created),
                    Span::styled(active, Style::default().fg(Color::Green)),
                ]));
            }
        }

        // Graph
        if let Some(graph) = &self.detail_graph {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Graph:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in graph.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // Footer
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan)),
            Span::raw("Use in Chat  "),
            Span::styled("[g] ", Style::default().fg(Color::Yellow)),
            Span::raw("Graph  "),
            Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Close"),
        ]));

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.detail_scroll, 0));
        frame.render_widget(para, inner);
    }
}

/// Render a graph JSON object as ASCII art. Returns a string.
fn render_ascii_graph(graph: &Value) -> String {
    let mut output = String::new();

    let nodes = match graph.get("nodes").and_then(|v| v.as_array()) {
        Some(n) => n,
        None => return "No nodes found".to_string(),
    };
    let edges = match graph.get("edges").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return "No edges found".to_string(),
    };

    let mut outgoing: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    for edge in edges {
        let source = edge
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let target = edge
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let conditional = edge
            .get("conditional")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        outgoing.entry(source).or_default().push((target, conditional));
    }

    let mut node_ids: Vec<String> = nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    node_ids.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("__start__", _) => std::cmp::Ordering::Less,
        (_, "__start__") => std::cmp::Ordering::Greater,
        ("__end__", _) => std::cmp::Ordering::Greater,
        (_, "__end__") => std::cmp::Ordering::Less,
        _ => a.cmp(b),
    });

    for (i, node_id) in node_ids.iter().enumerate() {
        output.push_str(&format!("[{node_id}]\n"));
        if let Some(targets) = outgoing.get(node_id) {
            match targets.len().cmp(&1) {
                std::cmp::Ordering::Equal => {
                    let (_, conditional) = &targets[0];
                    output.push_str("  │\n");
                    if *conditional {
                        output.push_str("  ▼ (conditional)\n");
                    } else {
                        output.push_str("  ▼\n");
                    }
                }
                std::cmp::Ordering::Greater => {
                    output.push_str("  │\n");
                    let mut names = Vec::new();
                    for (target, conditional) in targets {
                        if *conditional {
                            names.push(format!("[{target}](c)"));
                        } else {
                            names.push(format!("[{target}]"));
                        }
                    }
                    output.push_str(&format!("  ├─ {}\n", names.join("  ")));
                }
                std::cmp::Ordering::Less => {}
            }
        }
        if i < node_ids.len() - 1 {
            output.push('\n');
        }
    }

    output
}
