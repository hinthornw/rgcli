use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::langsmith::Deployment;
use crate::ui::screen::{Screen, ScreenAction, ScreenContext};
use crate::ui::widgets::resource_table::{Column, ResourceTable};
use crate::{config, langsmith};

enum AsyncResult {
    List(Vec<Deployment>),
    Error(String),
    Info(String),
    InfoError(String),
    ContextAdded(String),
}

pub struct DeploymentsScreen {
    pub table: ResourceTable,
    loaded: bool,
    async_rx: Option<mpsc::UnboundedReceiver<AsyncResult>>,
    deployments: Vec<Deployment>,
    context_map: HashMap<String, String>, // deployment_url → context_name
    active_context: String,
    api_key: String,

    // Detail pane
    detail_idx: Option<usize>,
    detail_info: Option<String>,
    detail_loading: bool,
    detail_error: Option<String>,
    detail_scroll: u16,

    // API key input mode
    api_key_input: bool,
    api_key_buf: String,
}

impl DeploymentsScreen {
    pub fn new() -> Self {
        Self {
            table: ResourceTable::new(
                "Deployments",
                vec![
                    Column {
                        name: "Name".to_string(),
                        width_pct: 30,
                    },
                    Column {
                        name: "Status".to_string(),
                        width_pct: 15,
                    },
                    Column {
                        name: "Context".to_string(),
                        width_pct: 20,
                    },
                    Column {
                        name: "URL".to_string(),
                        width_pct: 35,
                    },
                ],
            ),
            loaded: false,
            async_rx: None,
            deployments: Vec::new(),
            context_map: HashMap::new(),
            active_context: String::new(),
            api_key: String::new(),
            detail_idx: None,
            detail_info: None,
            detail_loading: false,
            detail_error: None,
            detail_scroll: 0,
            api_key_input: false,
            api_key_buf: String::new(),
        }
    }

    pub fn on_enter(&mut self) {
        if self.loaded {
            return;
        }
        self.table.loading = true;

        // Load context config to build URL → context name map
        let ctx_cfg = config::load_context_config().unwrap_or_default();
        self.active_context = ctx_cfg.current_context.clone();
        self.context_map.clear();
        for (name, cfg) in &ctx_cfg.contexts {
            let url = cfg.endpoint.trim_end_matches('/').to_string();
            self.context_map.insert(url, name.clone());
        }

        // Get API key (check config, then env var)
        let mut api_key = config::load()
            .map(|c| c.api_key.clone())
            .unwrap_or_default();
        if api_key.is_empty() {
            api_key = std::env::var("LANGSMITH_API_KEY").unwrap_or_default();
        }
        if api_key.is_empty() {
            self.table.loading = false;
            self.api_key_input = true;
            self.api_key_buf.clear();
            return;
        }
        self.api_key = api_key.clone();

        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        tokio::spawn(async move {
            match langsmith::search_deployments(&api_key, "").await {
                Ok(deployments) => {
                    let _ = tx.send(AsyncResult::List(deployments));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    fn build_rows(&self) -> Vec<Vec<String>> {
        self.deployments
            .iter()
            .map(|d| {
                let url = d.url().unwrap_or("(no url)");
                let url_trimmed = url.trim_end_matches('/');
                let context = self
                    .context_map
                    .get(url_trimmed)
                    .map(|name| {
                        if *name == self.active_context {
                            format!("{name} *")
                        } else {
                            name.clone()
                        }
                    })
                    .unwrap_or_else(|| "-".to_string());
                vec![
                    d.name.clone(),
                    if d.status.is_empty() {
                        "-".to_string()
                    } else {
                        d.status.clone()
                    },
                    context,
                    url.to_string(),
                ]
            })
            .collect()
    }

    fn open_detail(&mut self, idx: usize) {
        let Some(deployment) = self.deployments.get(idx) else {
            return;
        };
        self.detail_idx = Some(idx);
        self.detail_info = None;
        self.detail_loading = false;
        self.detail_error = None;
        self.detail_scroll = 0;

        // Fetch /info if deployment has a URL
        if let Some(url) = deployment.url() {
            self.detail_loading = true;
            let info_url = format!("{}/info", url.trim_end_matches('/'));
            let api_key = self.api_key.clone();
            let (tx, rx) = mpsc::unbounded_channel();
            self.async_rx = Some(rx);
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let mut req = client.get(&info_url);
                if !api_key.is_empty() {
                    req = req.header("x-api-key", &api_key);
                }
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            let version =
                                json.get("version").and_then(|v| v.as_str()).unwrap_or("-");
                            let _ = tx.send(AsyncResult::Info(version.to_string()));
                        } else {
                            let _ = tx.send(AsyncResult::InfoError("Invalid JSON".to_string()));
                        }
                    }
                    Ok(resp) => {
                        let _ = tx.send(AsyncResult::InfoError(format!("HTTP {}", resp.status())));
                    }
                    Err(e) => {
                        let _ = tx.send(AsyncResult::InfoError(e.to_string()));
                    }
                }
            });
        }
    }

    fn add_as_context(&mut self) {
        let Some(idx) = self.detail_idx else { return };
        let Some(deployment) = self.deployments.get(idx) else {
            return;
        };
        let Some(url) = deployment.url() else {
            self.detail_error = Some("Deployment has no URL".to_string());
            return;
        };

        let context_name = deployment
            .name
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "-");

        let cfg = config::Config {
            endpoint: url.to_string(),
            api_key: self.api_key.clone(),
            assistant_id: String::new(),
            custom_headers: HashMap::new(),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        self.async_rx = Some(rx);
        let name = context_name.clone();
        let cfg_clone = cfg.clone();
        tokio::spawn(async move {
            match config::save_context(&name, &cfg_clone) {
                Ok(()) => {
                    let _ = tx.send(AsyncResult::ContextAdded(name));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::InfoError(format!("Save failed: {e}")));
                }
            }
        });
    }

    fn close_detail(&mut self) {
        self.detail_idx = None;
        self.detail_info = None;
        self.detail_loading = false;
        self.detail_error = None;
        self.detail_scroll = 0;
    }

    pub fn poll(&mut self) {
        let mut results = Vec::new();
        if let Some(rx) = &mut self.async_rx {
            while let Ok(result) = rx.try_recv() {
                results.push(result);
            }
        }
        for result in results {
            match result {
                AsyncResult::List(deployments) => {
                    self.deployments = deployments;
                    let rows = self.build_rows();
                    self.table.set_rows(rows);
                    self.loaded = true;
                }
                AsyncResult::Error(e) => {
                    self.table.set_error(e);
                    self.loaded = true;
                }
                AsyncResult::Info(version) => {
                    self.detail_info = Some(version);
                    self.detail_loading = false;
                }
                AsyncResult::InfoError(e) => {
                    self.detail_error = Some(e);
                    self.detail_loading = false;
                }
                AsyncResult::ContextAdded(name) => {
                    if let Some(idx) = self.detail_idx {
                        if let Some(d) = self.deployments.get(idx) {
                            if let Some(url) = d.url() {
                                self.context_map
                                    .insert(url.trim_end_matches('/').to_string(), name.clone());
                            }
                        }
                    }
                    let rows = self.build_rows();
                    self.table.set_rows(rows);
                    self.detail_error = None;
                    self.detail_info = Some(format!("Added as context: {name}"));
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ScreenAction {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return ScreenAction::Quit;
        }
        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return ScreenAction::Quit;
        }

        // API key input mode
        if self.api_key_input {
            match key.code {
                KeyCode::Esc => {
                    self.api_key_input = false;
                    return ScreenAction::Navigate(Screen::Chat);
                }
                KeyCode::Enter => {
                    let key_val = self.api_key_buf.trim().to_string();
                    if !key_val.is_empty() {
                        // Save the API key to the current context's config
                        if let Ok(mut cfg) = config::load() {
                            cfg.api_key = key_val.clone();
                            let ctx_name = config::current_context_name();
                            let _ = config::save_context(&ctx_name, &cfg);
                        }
                        self.api_key_input = false;
                        self.api_key_buf.clear();
                        // Retry loading
                        self.loaded = false;
                        self.on_enter();
                    }
                    return ScreenAction::None;
                }
                KeyCode::Backspace => {
                    self.api_key_buf.pop();
                    return ScreenAction::None;
                }
                KeyCode::Char(c) => {
                    self.api_key_buf.push(c);
                    return ScreenAction::None;
                }
                _ => return ScreenAction::None,
            }
        }

        // Detail pane keys
        if self.detail_idx.is_some() {
            match key.code {
                KeyCode::Enter => {
                    // Switch to this deployment's context if it exists
                    if let Some(idx) = self.detail_idx {
                        if let Some(d) = self.deployments.get(idx) {
                            if let Some(url) = d.url() {
                                let url_trimmed = url.trim_end_matches('/');
                                if let Some(name) = self.context_map.get(url_trimmed) {
                                    let name = name.clone();
                                    self.close_detail();
                                    return ScreenAction::NavigateWithContext(
                                        Screen::Chat,
                                        ScreenContext::SwitchContext(name),
                                    );
                                }
                            }
                        }
                    }
                    return ScreenAction::None;
                }
                KeyCode::Char('a') => {
                    self.add_as_context();
                    return ScreenAction::None;
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
                    self.open_detail(sel);
                }
                ScreenAction::None
            }
            KeyCode::Char('r') => {
                self.loaded = false;
                self.on_enter();
                ScreenAction::None
            }
            KeyCode::Char('q') | KeyCode::Esc => ScreenAction::Navigate(Screen::Chat),
            _ => ScreenAction::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if self.api_key_input {
            self.render_api_key_prompt(frame, area);
            return;
        }
        if self.detail_idx.is_some() {
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

    fn render_api_key_prompt(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Deployments ")
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let masked: String = if self.api_key_buf.is_empty() {
            String::new()
        } else {
            let len = self.api_key_buf.len();
            if len <= 4 {
                "*".repeat(len)
            } else {
                format!("{}{}", "*".repeat(len - 4), &self.api_key_buf[len - 4..])
            }
        };

        let lines = vec![
            Line::default(),
            Line::from(Span::styled(
                "  Enter your LangSmith API key to view deployments.",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "  Get one at https://smith.langchain.com/settings",
                Style::default().fg(Color::DarkGray),
            )),
            Line::default(),
            Line::from(vec![
                Span::styled("  API Key: ", Style::default().fg(Color::Cyan)),
                Span::raw(&masked),
                Span::styled("_", Style::default().fg(Color::White)),
            ]),
            Line::default(),
            Line::from(vec![
                Span::styled("  [Enter] ", Style::default().fg(Color::Yellow)),
                Span::raw("Save & connect  "),
                Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)),
                Span::raw("Cancel"),
            ]),
            Line::default(),
            Line::from(Span::styled(
                "  The key will be saved to your current context config.",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let para = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(para, inner);
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect) {
        let Some(idx) = self.detail_idx else { return };
        let Some(deployment) = self.deployments.get(idx) else {
            return;
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", deployment.name))
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.detail_loading {
            let p = Paragraph::new("Loading...");
            frame.render_widget(p, inner);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Name
        lines.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&deployment.name),
        ]));

        // Status
        let status = if deployment.status.is_empty() {
            "-"
        } else {
            &deployment.status
        };
        let status_color = match status {
            "active" | "running" => Color::Green,
            "error" | "failed" => Color::Red,
            _ => Color::Yellow,
        };
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(status, Style::default().fg(status_color)),
        ]));

        // URL
        let url = deployment.url().unwrap_or("(no url)");
        lines.push(Line::from(vec![
            Span::styled("URL: ", Style::default().fg(Color::DarkGray)),
            Span::raw(url),
        ]));

        // API Version (from /info)
        if let Some(version) = &self.detail_info {
            lines.push(Line::from(vec![
                Span::styled("API Version: ", Style::default().fg(Color::DarkGray)),
                Span::raw(version),
            ]));
        }

        if let Some(err) = &self.detail_error {
            lines.push(Line::from(Span::styled(
                format!("Error: {err}"),
                Style::default().fg(Color::Red),
            )));
        }

        // Context info
        let url_trimmed = url.trim_end_matches('/');
        lines.push(Line::from(""));
        if let Some(ctx_name) = self.context_map.get(url_trimmed) {
            let active = if *ctx_name == self.active_context {
                " (active)"
            } else {
                ""
            };
            lines.push(Line::from(vec![
                Span::styled("Context: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{ctx_name}{active}"),
                    Style::default().fg(Color::Green),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Context: ", Style::default().fg(Color::DarkGray)),
                Span::raw("not configured"),
            ]));
        }

        // Footer
        lines.push(Line::from(""));
        let has_context = self.context_map.contains_key(url_trimmed);
        let mut footer = vec![
            Span::styled("[a] ", Style::default().fg(Color::Yellow)),
            Span::raw("Add as context  "),
        ];
        if has_context {
            footer.push(Span::styled("[Enter] ", Style::default().fg(Color::Cyan)));
            footer.push(Span::raw("Switch  "));
        }
        footer.push(Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)));
        footer.push(Span::raw("Close"));
        lines.push(Line::from(footer));

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.detail_scroll, 0));
        frame.render_widget(para, inner);
    }
}
