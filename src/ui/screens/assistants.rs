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

pub struct AssistantsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
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
