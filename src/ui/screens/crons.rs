use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::ui::screen::{Screen, ScreenAction};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Rows(Vec<Vec<String>>),
    Error(String),
}

pub struct CronsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
}

impl CronsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new(
                "Cron Jobs",
                vec![
                    Column {
                        name: "ID".to_string(),
                        width_pct: 25,
                    },
                    Column {
                        name: "Schedule".to_string(),
                        width_pct: 25,
                    },
                    Column {
                        name: "Assistant".to_string(),
                        width_pct: 25,
                    },
                    Column {
                        name: "Created".to_string(),
                        width_pct: 25,
                    },
                ],
            ),
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
            let url = format!("{}/runs/crons/search", client.endpoint());
            match client.post_json(&url, &serde_json::json!({})).await {
                Ok(resp) => {
                    let crons = resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                    let rows: Vec<Vec<String>> = crons
                        .iter()
                        .map(|c| {
                            let id = c.get("cron_id").and_then(|v| v.as_str()).unwrap_or("-");
                            let id_short: String = id.chars().take(12).collect();
                            let schedule = c
                                .get("schedule")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            let assistant = c
                                .get("assistant_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            let created = c
                                .get("created_at")
                                .and_then(|v| v.as_str())
                                .unwrap_or("-")
                                .to_string();
                            vec![id_short, schedule, assistant, created]
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
