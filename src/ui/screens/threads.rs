use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::ui::screen::{Screen, ScreenAction, ScreenContext};
use crate::ui::widgets::resource_table::{Column, ResourceTable};

enum AsyncResult {
    Rows(Vec<Vec<String>>),
    Error(String),
    Deleted(String),
}

pub struct ThreadsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
    thread_ids: Vec<String>, // full thread IDs parallel to table rows
}

impl ThreadsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new("Threads", vec![
                Column { name: "ID".to_string(), width_pct: 25 },
                Column { name: "Created".to_string(), width_pct: 35 },
                Column { name: "Updated".to_string(), width_pct: 40 },
            ]),
            loaded: false,
            async_rx: None,
            thread_ids: Vec::new(),
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
                            vec![
                                id_short,
                                t.created_at.clone().unwrap_or_else(|| "-".to_string()),
                                t.updated_at.clone().unwrap_or_else(|| "-".to_string()),
                            ]
                        })
                        .collect();
                    // Send full IDs separately encoded in first row items
                    let ids: Vec<String> = threads.iter().map(|t| t.thread_id.clone()).collect();
                    // Encode full IDs as additional column
                    let mut full_rows: Vec<Vec<String>> = Vec::new();
                    for (i, row) in rows.into_iter().enumerate() {
                        let mut r = row;
                        r.push(ids[i].clone()); // hidden column
                        full_rows.push(r);
                    }
                    let _ = tx.send(AsyncResult::Rows(full_rows));
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
                        self.thread_ids = rows.iter().map(|r| r.get(3).cloned().unwrap_or_default()).collect();
                        let display_rows: Vec<Vec<String>> = rows.into_iter().map(|mut r| { r.truncate(3); r }).collect();
                        self.table.set_rows(display_rows);
                        self.loaded = true;
                    }
                    AsyncResult::Error(e) => {
                        self.table.set_error(e);
                        self.loaded = true;
                    }
                    AsyncResult::Deleted(msg) => {
                        // Reload
                        self.loaded = false;
                        self.table.loading = true;
                        // We'll use the stored msg as confirmation (ignored for now)
                        let _ = msg;
                    }
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, client: &Client) -> ScreenAction {
        // Ctrl+C / Ctrl+D quit
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
            KeyCode::Enter => {
                if let Some(sel) = self.table.state.selected() {
                    if let Some(full_id) = self.thread_ids.get(sel) {
                        return ScreenAction::NavigateWithContext(
                            Screen::Chat,
                            ScreenContext::Thread(full_id.clone()),
                        );
                    }
                }
                ScreenAction::None
            }
            KeyCode::Char('r') => {
                self.loaded = false;
                self.on_enter(client);
                ScreenAction::None
            }
            KeyCode::Char('d') => {
                if let Some(sel) = self.table.state.selected() {
                    if let Some(full_id) = self.thread_ids.get(sel).cloned() {
                        let client = client.clone();
                        let (tx, rx) = mpsc::unbounded_channel();
                        self.async_rx = Some(rx);
                        self.loaded = false;
                        self.table.loading = true;
                        tokio::spawn(async move {
                            let url = format!("{}/threads/{}", client.endpoint(), full_id);
                            match client.delete_url(&url).await {
                                Ok(()) => { let _ = tx.send(AsyncResult::Deleted(full_id)); }
                                Err(e) => { let _ = tx.send(AsyncResult::Error(e.to_string())); }
                            }
                        });
                    }
                }
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
