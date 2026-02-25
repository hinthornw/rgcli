use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::ui::screen::{Screen, ScreenAction};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Namespaces(Vec<Vec<String>>),
    Items(Vec<Vec<String>>),
    Error(String),
}

#[derive(PartialEq)]
enum Pane {
    Left,
    Right,
}

pub struct StoreScreen {
    namespace_table: ResourceTable,
    items_table: ResourceTable,
    active_pane: Pane,
    namespace_paths: Vec<String>, // full namespace paths
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
}

impl StoreScreen {
    pub fn new() -> Self {
        Self {
            namespace_table: ResourceTable::new("Namespaces", vec![
                Column { name: "Namespace".to_string(), width_pct: 100 },
            ]),
            items_table: ResourceTable::new("Items", vec![
                Column { name: "Key".to_string(), width_pct: 40 },
                Column { name: "Value (preview)".to_string(), width_pct: 60 },
            ]),
            active_pane: Pane::Left,
            namespace_paths: Vec::new(),
            loaded: false,
            async_rx: None,
        }
    }

    pub fn on_enter(&mut self, client: &Client) {
        if self.loaded {
            return;
        }
        self.namespace_table.loading = true;
        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        let client = client.clone();
        tokio::spawn(async move {
            let url = format!("{}/store/namespaces", client.endpoint());
            match client.post_json(&url, &serde_json::json!({})).await {
                Ok(resp) => {
                    let namespaces = resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                    let rows: Vec<Vec<String>> = namespaces
                        .iter()
                        .filter_map(|ns| {
                            // Each namespace is an array of path components
                            let parts = ns.as_array()?;
                            let path: Vec<&str> = parts.iter().filter_map(|p| p.as_str()).collect();
                            Some(vec![path.join(".")])
                        })
                        .collect();
                    let _ = tx.send(AsyncResult::Namespaces(rows));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    fn load_items(&mut self, namespace: &str, client: &Client) {
        self.items_table.loading = true;
        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        let client = client.clone();
        let ns_parts: Vec<String> = namespace.split('.').map(|s| s.to_string()).collect();
        tokio::spawn(async move {
            let url = format!("{}/store/items/search", client.endpoint());
            let body = serde_json::json!({
                "namespace_prefix": ns_parts,
                "limit": 100,
            });
            match client.post_json(&url, &body).await {
                Ok(resp) => {
                    let items = resp.get("items").and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[]);
                    let rows: Vec<Vec<String>> = items
                        .iter()
                        .map(|item| {
                            let key = item.get("key").and_then(|v| v.as_str()).unwrap_or("-").to_string();
                            let value = item.get("value").map(|v| {
                                let s = v.to_string();
                                if s.len() > 60 { format!("{}...", &s[..57]) } else { s }
                            }).unwrap_or_else(|| "-".to_string());
                            vec![key, value]
                        })
                        .collect();
                    let _ = tx.send(AsyncResult::Items(rows));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &mut self.async_rx {
            while let Ok(result) = rx.try_recv() {
                match result {
                    AsyncResult::Namespaces(rows) => {
                        self.namespace_paths = rows.iter().map(|r| r[0].clone()).collect();
                        self.namespace_table.set_rows(rows);
                        self.loaded = true;
                    }
                    AsyncResult::Items(rows) => {
                        self.items_table.set_rows(rows);
                    }
                    AsyncResult::Error(e) => {
                        if !self.loaded {
                            self.namespace_table.set_error(e);
                            self.loaded = true;
                        } else {
                            self.items_table.set_error(e);
                        }
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

        // Tab switches panes
        if key.code == KeyCode::Tab {
            self.active_pane = if self.active_pane == Pane::Left { Pane::Right } else { Pane::Left };
            return ScreenAction::None;
        }

        if self.active_pane == Pane::Left {
            if self.namespace_table.handle_key(key) {
                return ScreenAction::None;
            }
            if key.code == KeyCode::Enter {
                if let Some(sel) = self.namespace_table.state.selected() {
                    if let Some(ns) = self.namespace_paths.get(sel).cloned() {
                        self.load_items(&ns, client);
                        self.active_pane = Pane::Right;
                    }
                }
                return ScreenAction::None;
            }
        } else if self.items_table.handle_key(key) {
            return ScreenAction::None;
        }

        match key.code {
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
        let chunks = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

        // Highlight active pane with different border style
        self.namespace_table.render(frame, chunks[0]);
        self.items_table.render(frame, chunks[1]);
    }
}
