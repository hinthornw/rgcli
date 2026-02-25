use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::ui::screen::{Screen, ScreenAction};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Rows(Vec<Vec<String>>),
    Error(String),
}

pub struct LogsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
}

impl LogsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new("Logs (Recent Runs)", vec![
                Column { name: "Run ID".to_string(), width_pct: 20 },
                Column { name: "Status".to_string(), width_pct: 15 },
                Column { name: "Thread".to_string(), width_pct: 20 },
                Column { name: "Created".to_string(), width_pct: 25 },
            ]),
            loaded: false,
            async_rx: None,
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
            match client.search_threads(5).await {
                Ok(threads) => {
                    let mut all_rows = Vec::new();
                    for thread in &threads {
                        let tid_short: String = thread.thread_id.chars().take(8).collect();
                        let url = format!("{}/threads/{}/runs", client.endpoint(), thread.thread_id);
                        let body = serde_json::json!({ "limit": 3 });
                        if let Ok(resp) = client.post_json(&url, &body).await {
                            let runs = resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                            for r in runs {
                                let id = r.get("run_id").and_then(|v| v.as_str()).unwrap_or("-");
                                let id_short: String = id.chars().take(12).collect();
                                let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("-").to_string();
                                let created = r.get("created_at").and_then(|v| v.as_str()).unwrap_or("-").to_string();
                                all_rows.push(vec![id_short, status, tid_short.clone(), created]);
                            }
                        }
                    }
                    let _ = tx.send(AsyncResult::Rows(all_rows));
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
                    AsyncResult::Rows(rows) => {
                        self.table.set_rows(rows);
                        self.loaded = true;
                    }
                    AsyncResult::Error(e) => {
                        self.table.set_error(e);
                        self.loaded = true;
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

        if self.table.handle_key(key) {
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
        self.table.render(frame, area);
    }
}
