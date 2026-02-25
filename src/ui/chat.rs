use std::collections::VecDeque;
use std::io::{Stdout, stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;
use tokio::time;
use tui_textarea::{CursorMove, Input, Key, TextArea};

use crate::api::{Client, StreamEvent};
use crate::ui::styles;

const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const ESC_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_INPUT_LINES: usize = 5;
const PLACEHOLDER: &str = "Type a message... (Alt+Enter for newline)";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug)]
pub enum ChatExit {
    Configure,
    SwitchContext(String),
    NewThread,
    PickThread,
    Quit,
}

enum Action {
    None,
    Send(String),
    Cancel,
    Quit,
    Configure,
    SwitchContext(String),
    Help,
    NewThread,
    PickThread,
    Clear,
}

#[derive(Clone)]
enum ChatMessage {
    User(String),
    Assistant(String),
    System(String),
    Error(String),
}

struct App {
    messages: Vec<ChatMessage>,
    scroll_offset: u16,
    auto_scroll: bool,

    // Streaming
    stream_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    active_run_id: Option<String>,
    streaming_text: String,
    spinner_idx: usize,
    is_waiting: bool,

    // Input
    textarea: TextArea<'static>,

    // Keys
    ctrl_c_at: Option<Instant>,
    last_esc_at: Option<Instant>,

    // Completions
    completions: Vec<usize>,
    completion_idx: usize,
    show_complete: bool,

    // Queue & status
    pending_messages: VecDeque<String>,
    update_notice: Option<String>,
    update_rx: Option<mpsc::UnboundedReceiver<String>>,
    context_name: String,
    welcome_lines: Vec<Line<'static>>,
    thread_id: String,

    // Dev toolbar
    devtools: bool,
    metrics: RunMetrics,
}

#[derive(Default, Clone)]
struct RunMetrics {
    run_started_at: Option<Instant>,
    first_token_at: Option<Instant>,
    last_token_at: Option<Instant>,
    token_count: usize,
    total_chars: usize,
    run_id: Option<String>,
    // Last completed run stats (persisted after Done)
    last_ttft_ms: Option<u128>,
    last_tokens_per_sec: Option<f64>,
    last_total_ms: Option<u128>,
    last_token_count: Option<usize>,
    last_run_id: Option<String>,
}

impl App {
    fn new(context_name: &str, update_rx: mpsc::UnboundedReceiver<String>) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text(PLACEHOLDER);
        textarea.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            stream_rx: None,
            active_run_id: None,
            streaming_text: String::new(),
            spinner_idx: 0,
            is_waiting: false,
            textarea,
            ctrl_c_at: None,
            last_esc_at: None,
            completions: Vec::new(),
            completion_idx: 0,
            show_complete: false,
            pending_messages: VecDeque::new(),
            update_notice: None,
            update_rx: Some(update_rx),
            context_name: context_name.to_string(),
            welcome_lines: Vec::new(),
            thread_id: String::new(),
            devtools: false,
            metrics: RunMetrics::default(),
        }
    }

    fn is_streaming(&self) -> bool {
        self.stream_rx.is_some()
    }
}

pub struct ChatConfig {
    pub version: String,
    pub endpoint: String,
    pub config_path: String,
    pub context_info: String,
}

pub async fn run_chat_loop(
    client: &Client,
    assistant_id: &str,
    thread_id: &str,
    history: &[crate::api::Message],
    chat_config: &ChatConfig,
) -> Result<ChatExit> {
    // Spawn background update checker
    let (update_tx, update_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let _ = check_for_updates_loop(update_tx).await;
    });

    let mut app = App::new(&chat_config.context_info, update_rx);

    // Fetch deployment info
    let deploy_info = match client.get_info().await {
        Ok(info) => {
            let mut parts = Vec::new();
            if let Some(v) = info.get("langgraph_api_version").and_then(|v| v.as_str()) {
                parts.push(format!("langgraph: {v}"));
            } else if let Some(v) = info.get("version").and_then(|v| v.as_str()) {
                parts.push(format!("api: {v}"));
            }
            if parts.is_empty() { None } else { Some(parts.join(" | ")) }
        }
        Err(_) => None,
    };

    // Add logo as initial chat content
    app.welcome_lines = styles::logo_lines(
        &chat_config.version,
        &chat_config.endpoint,
        &chat_config.config_path,
        &chat_config.context_info,
        deploy_info.as_deref(),
    );

    // Load history
    for msg in history {
        match msg.role.as_str() {
            "user" | "human" => app.messages.push(ChatMessage::User(msg.content.clone())),
            "assistant" | "ai" => app
                .messages
                .push(ChatMessage::Assistant(msg.content.clone())),
            _ => app.messages.push(ChatMessage::System(format!(
                "[{}] {}",
                msg.role, msg.content
            ))),
        }
    }

    // Enter alternate screen
    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_event_loop(&mut terminal, &mut app, client, assistant_id, thread_id).await;

    // Restore terminal
    execute!(stdout(), LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    client: &Client,
    assistant_id: &str,
    thread_id: &str,
) -> Result<ChatExit> {
    let mut term_events = EventStream::new();
    let mut interval = time::interval(Duration::from_millis(80));

    // Initial draw
    terminal.draw(|f| draw(f, app))?;

    loop {
        tokio::select! {
            biased;

            // Update notifications
            Some(notice) = recv_update(&mut app.update_rx) => {
                app.update_notice = Some(notice);
                terminal.draw(|f| draw(f, app))?;
            }

            // Stream events
            Some(event) = recv_stream(&mut app.stream_rx) => {
                handle_stream_event(app, event, client, thread_id, assistant_id);
                terminal.draw(|f| draw(f, app))?;
            }

            // Terminal input
            Some(Ok(event)) = term_events.next() => {
                let action = handle_terminal_event(app, event);
                match action {
                    Action::Send(msg) => {
                        app.messages.push(ChatMessage::User(msg.clone()));
                        app.auto_scroll = true;
                        if app.is_streaming() {
                            app.pending_messages.push_back(msg);
                            app.messages.push(ChatMessage::System("(queued)".to_string()));
                        } else {
                            start_run(client, thread_id, assistant_id, &msg, None, app);
                        }
                        reset_textarea(app);
                    }
                    Action::Cancel => {
                        if let Some(run_id) = app.active_run_id.clone() {
                            let client = client.clone();
                            let tid = thread_id.to_string();
                            tokio::spawn(async move {
                                let _ = client.cancel_run(&tid, &run_id).await;
                            });
                            app.messages.push(ChatMessage::System("(cancelling...)".to_string()));
                        }
                    }
                    Action::Quit => {
                        return Ok(ChatExit::Quit);
                    }
                    Action::Configure => {
                        return Ok(ChatExit::Configure);
                    }
                    Action::SwitchContext(name) => {
                        return Ok(ChatExit::SwitchContext(name));
                    }
                    Action::Help => {
                        show_help(app);
                        reset_textarea(app);
                    }
                    Action::NewThread => {
                        return Ok(ChatExit::NewThread);
                    }
                    Action::PickThread => {
                        return Ok(ChatExit::PickThread);
                    }
                    Action::Clear => {
                        app.messages.clear();
                        app.auto_scroll = true;
                        reset_textarea(app);
                    }
                    Action::None => {}
                }
                terminal.draw(|f| draw(f, app))?;
            }

            // Spinner tick
            _ = interval.tick() => {
                if app.is_streaming() {
                    app.spinner_idx += 1;
                    terminal.draw(|f| draw(f, app))?;
                }
            }
        }
    }
}

// --- Drawing ---

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let input_height = (app.textarea.lines().len().clamp(1, MAX_INPUT_LINES) as u16) + 2;
    let area = frame.area();

    if app.devtools {
        let chunks = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(input_height),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

        render_chat(frame, app, chunks[0]);
        render_input(frame, app, chunks[1]);
        render_devtools(frame, app, chunks[2]);
        render_status(frame, app, chunks[3]);
    } else {
        let chunks = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(area);

        render_chat(frame, app, chunks[0]);
        render_input(frame, app, chunks[1]);
        render_status(frame, app, chunks[2]);
    }
}

fn render_chat(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Welcome logo
    lines.extend(app.welcome_lines.clone());

    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                lines.push(Line::default());
                for line in text.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("You: ", styles::user_style()),
                        Span::raw(line),
                    ]));
                }
            }
            ChatMessage::Assistant(text) => {
                lines.push(Line::default());
                let mut first = true;
                for line in text.lines() {
                    if first {
                        lines.push(Line::from(vec![
                            Span::styled("Assistant: ", styles::assistant_style()),
                            Span::raw(line),
                        ]));
                        first = false;
                    } else {
                        lines.push(Line::raw(line));
                    }
                }
            }
            ChatMessage::System(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::system_style_r(),
                )));
            }
            ChatMessage::Error(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::error_style_r(),
                )));
            }
        }
    }

    // Streaming content
    if !app.streaming_text.is_empty() {
        lines.push(Line::default());
        let mut first = true;
        for line in app.streaming_text.lines() {
            if first {
                lines.push(Line::from(vec![
                    Span::styled("Assistant: ", styles::assistant_style()),
                    Span::raw(line),
                ]));
                first = false;
            } else {
                lines.push(Line::raw(line));
            }
        }
        // Handle trailing newline
        if app.streaming_text.ends_with('\n') {
            lines.push(Line::raw(""));
        }
    } else if app.is_waiting {
        let frame_idx = app.spinner_idx % SPINNER_FRAMES.len();
        let spinner = SPINNER_FRAMES[frame_idx];
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("{} Thinking...", spinner),
            styles::system_style_r(),
        )));
    }

    let scroll = if app.auto_scroll {
        compute_auto_scroll(&lines, area)
    } else {
        app.scroll_offset
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE))
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

fn compute_auto_scroll(lines: &[Line], area: Rect) -> u16 {
    // Estimate total wrapped lines manually
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

fn render_input(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(ratatui::style::Style::new().dark_gray());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(&app.textarea, inner);

    // Render completion popup above input area
    if app.show_complete && !app.completions.is_empty() {
        let items: Vec<Line> = app
            .completions
            .iter()
            .enumerate()
            .map(|(i, &idx)| {
                let cmd = &SLASH_COMMANDS[idx];
                if i == app.completion_idx {
                    Line::from(vec![
                        Span::styled(format!(" > {} ", cmd.name), styles::user_style()),
                        Span::styled(cmd.desc, styles::system_style_r()),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(format!("   {} ", cmd.name)),
                        Span::styled(cmd.desc, styles::system_style_r()),
                    ])
                }
            })
            .collect();

        let popup_height = items.len() as u16 + 2; // +2 for borders
        let popup_width = 40.min(area.width);

        // Position popup just above the input area
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

fn render_devtools(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut parts: Vec<Span> = vec![Span::styled(" devtools ", styles::user_style())];

    // Show live metrics during streaming, otherwise last completed run
    if app.is_streaming() || app.is_waiting {
        if let Some(started) = app.metrics.run_started_at {
            let elapsed = started.elapsed().as_millis();
            if let Some(first) = app.metrics.first_token_at {
                let ttft = first.duration_since(started).as_millis();
                parts.push(Span::raw(format!("TTFT: {}ms ", ttft)));
                let stream_dur = first.elapsed().as_secs_f64();
                if stream_dur > 0.0 && app.metrics.token_count > 1 {
                    let tps = (app.metrics.token_count - 1) as f64 / stream_dur;
                    parts.push(Span::raw(format!("{:.0} tok/s ", tps)));
                }
                parts.push(Span::raw(format!(
                    "tokens: {} ",
                    app.metrics.token_count
                )));
            } else {
                parts.push(Span::raw(format!("waiting: {}ms ", elapsed)));
            }
        }
    } else if app.metrics.last_total_ms.is_some() {
        if let Some(ttft) = app.metrics.last_ttft_ms {
            parts.push(Span::raw(format!("TTFT: {}ms ", ttft)));
        }
        if let Some(tps) = app.metrics.last_tokens_per_sec {
            parts.push(Span::raw(format!("{:.0} tok/s ", tps)));
        }
        if let Some(total) = app.metrics.last_total_ms {
            parts.push(Span::raw(format!("total: {}ms ", total)));
        }
        if let Some(count) = app.metrics.last_token_count {
            parts.push(Span::raw(format!("tokens: {} ", count)));
        }
    }

    if let Some(rid) = app.metrics.run_id.as_deref().or(app.metrics.last_run_id.as_deref()) {
        let short = if rid.len() > 8 { &rid[..8] } else { rid };
        parts.push(Span::styled(format!("run:{short}"), styles::system_style_r()));
    }

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(
        ratatui::style::Style::new()
            .fg(ratatui::style::Color::White)
            .bg(ratatui::style::Color::Rgb(40, 40, 40)),
    );
    frame.render_widget(bar, area);
}

fn render_status(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut left_parts: Vec<Span> = vec![Span::raw(" "), Span::raw(&app.context_name)];

    if !app.pending_messages.is_empty() {
        let n = app.pending_messages.len();
        left_parts.push(Span::raw(format!(" | {} queued", n)));
    }

    let right = if let Some(notice) = &app.update_notice {
        notice.clone()
    } else {
        String::new()
    };

    // Build the status line: left-aligned context, right-aligned notice
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

// --- Streaming ---

async fn recv_update(rx: &mut Option<mpsc::UnboundedReceiver<String>>) -> Option<String> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn check_for_updates_loop(tx: mpsc::UnboundedSender<String>) -> Result<()> {
    use crate::update;
    tokio::time::sleep(Duration::from_secs(5)).await;
    loop {
        let _ = update::force_check().await;
        if let Some(notice) = update::pending_update_notice() {
            let _ = tx.send(notice);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

async fn recv_stream(rx: &mut Option<mpsc::UnboundedReceiver<StreamEvent>>) -> Option<StreamEvent> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

fn handle_stream_event(
    app: &mut App,
    event: StreamEvent,
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
) {
    match event {
        StreamEvent::RunStarted(id) => {
            app.active_run_id = Some(id.clone());
            app.metrics.run_id = Some(id);
        }
        StreamEvent::Token(token) => {
            if app.metrics.first_token_at.is_none() {
                app.metrics.first_token_at = Some(Instant::now());
            }
            app.metrics.last_token_at = Some(Instant::now());
            app.metrics.token_count += 1;
            app.metrics.total_chars += token.len();
            app.is_waiting = false;
            app.streaming_text.push_str(&token);
            app.auto_scroll = true;
        }
        StreamEvent::Done(result) => {
            if let Err(err) = result {
                app.messages
                    .push(ChatMessage::Error(format!("Error: {}", err)));
            } else if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
            app.streaming_text.clear();
            app.stream_rx = None;
            app.active_run_id = None;
            app.is_waiting = false;

            // Snapshot metrics from completed run
            if let Some(started) = app.metrics.run_started_at {
                app.metrics.last_total_ms = Some(started.elapsed().as_millis());
                app.metrics.last_ttft_ms = app
                    .metrics
                    .first_token_at
                    .map(|t| t.duration_since(started).as_millis());
                app.metrics.last_token_count = Some(app.metrics.token_count);
                app.metrics.last_run_id = app.metrics.run_id.clone();
                if let (Some(first), Some(last)) =
                    (app.metrics.first_token_at, app.metrics.last_token_at)
                {
                    let dur = last.duration_since(first).as_secs_f64();
                    if dur > 0.0 && app.metrics.token_count > 1 {
                        app.metrics.last_tokens_per_sec =
                            Some((app.metrics.token_count - 1) as f64 / dur);
                    }
                }
            }

            // Drain queue
            if let Some(msg) = app.pending_messages.pop_front() {
                // Remove the "(queued)" system message
                if let Some(ChatMessage::System(s)) = app.messages.last() {
                    if s == "(queued)" {
                        app.messages.pop();
                    }
                }
                app.messages.push(ChatMessage::User(msg.clone()));
                start_run(client, thread_id, assistant_id, &msg, Some("enqueue"), app);
            }
        }
    }
}

fn start_run(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    message: &str,
    multitask_strategy: Option<&str>,
    app: &mut App,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    app.stream_rx = Some(rx);
    app.is_waiting = true;
    app.spinner_idx = 0;
    app.active_run_id = None;
    app.streaming_text.clear();
    app.metrics.run_started_at = Some(Instant::now());
    app.metrics.first_token_at = None;
    app.metrics.last_token_at = None;
    app.metrics.token_count = 0;
    app.metrics.total_chars = 0;
    app.metrics.run_id = None;

    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let message = message.to_string();
    let strategy = multitask_strategy.map(String::from);

    tokio::spawn(async move {
        client
            .stream_run(
                &thread_id,
                &assistant_id,
                &message,
                strategy.as_deref(),
                &tx,
            )
            .await;
    });
}

// --- Input handling ---

fn handle_terminal_event(app: &mut App, event: Event) -> Action {
    let Event::Key(key) = event else {
        return Action::None;
    };

    // Reset ctrl_c if different key
    if key.code != KeyCode::Char('c') || !key.modifiers.contains(KeyModifiers::CONTROL) {
        app.ctrl_c_at = None;
    }

    // Check ctrl_c timeout
    if let Some(start) = app.ctrl_c_at {
        if start.elapsed() > CTRL_C_TIMEOUT {
            app.ctrl_c_at = None;
        }
    }

    // Esc timeout
    if let Some(start) = app.last_esc_at {
        if start.elapsed() > ESC_TIMEOUT {
            app.last_esc_at = None;
        }
    }

    // Completion handling
    if app.show_complete && !app.completions.is_empty() {
        if let Some(action) = handle_completion_key(&key, app) {
            return action;
        }
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.ctrl_c_at.is_some() {
                return Action::Quit;
            }
            app.ctrl_c_at = Some(Instant::now());
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        KeyCode::Esc => {
            if app.last_esc_at.is_some() && app.is_streaming() {
                app.last_esc_at = None;
                return Action::Cancel;
            }
            app.last_esc_at = Some(Instant::now());
            app.show_complete = false;
        }
        // Alt+Enter or Ctrl+J = newline
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
            if app.textarea.lines().len() < MAX_INPUT_LINES {
                app.textarea.insert_newline();
            }
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.textarea.lines().len() < MAX_INPUT_LINES {
                app.textarea.insert_newline();
            }
        }
        // Enter = send
        KeyCode::Enter => {
            let value = collect_input(&app.textarea);
            if value.is_empty() {
                return Action::None;
            }
            if value == "/quit" || value == "/exit" {
                return Action::Quit;
            }
            if value == "/configure" {
                return Action::Configure;
            }
            if value == "/help" {
                return Action::Help;
            }
            if let Some(name) = value.strip_prefix("/context ") {
                let name = name.trim();
                if !name.is_empty() {
                    return Action::SwitchContext(name.to_string());
                }
            }
            if value == "/context" {
                return Action::Help;
            }
            if value == "/new" {
                return Action::NewThread;
            }
            if value == "/threads" {
                return Action::PickThread;
            }
            if value == "/clear" {
                return Action::Clear;
            }
            if value == "/devtools" {
                app.devtools = !app.devtools;
                reset_textarea(app);
                return Action::None;
            }
            return Action::Send(value);
        }
        // Scroll
        KeyCode::PageUp => {
            app.auto_scroll = false;
            app.scroll_offset = app.scroll_offset.saturating_add(10);
        }
        KeyCode::PageDown => {
            if app.scroll_offset > 0 {
                app.scroll_offset = app.scroll_offset.saturating_sub(10);
            } else {
                app.auto_scroll = true;
            }
        }
        KeyCode::F(12) => {
            app.devtools = !app.devtools;
        }
        KeyCode::Tab => {
            update_completions(&app.textarea, &mut app.completions, &mut app.show_complete);
            if app.show_complete {
                app.completion_idx = 0;
            }
        }
        _ => {
            if let Some(input) = to_textarea_input(key) {
                app.textarea.input(input);
            }
        }
    }

    update_completions(&app.textarea, &mut app.completions, &mut app.show_complete);
    Action::None
}

fn handle_completion_key(key: &KeyEvent, app: &mut App) -> Option<Action> {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            app.completion_idx = (app.completion_idx + 1) % app.completions.len();
            Some(Action::None)
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.completion_idx =
                (app.completion_idx + app.completions.len() - 1) % app.completions.len();
            Some(Action::None)
        }
        KeyCode::Enter => {
            let cmd = SLASH_COMMANDS[app.completions[app.completion_idx]].name;
            app.textarea = TextArea::from([cmd.to_string()]);
            app.textarea.move_cursor(CursorMove::End);
            app.show_complete = false;
            Some(Action::None)
        }
        KeyCode::Esc => {
            app.show_complete = false;
            Some(Action::None)
        }
        _ => None,
    }
}

// --- Input helpers ---

fn collect_input(textarea: &TextArea) -> String {
    textarea
        .lines()
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn update_completions(textarea: &TextArea, completions: &mut Vec<usize>, show_complete: &mut bool) {
    let value: String = textarea
        .lines()
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if !value.starts_with('/') || value.contains('\n') {
        completions.clear();
        *show_complete = false;
        return;
    }
    let matches = slash_completions(&value);
    *show_complete = !matches.is_empty();
    *completions = matches;
}

fn to_textarea_input(key: KeyEvent) -> Option<Input> {
    let modifiers = key.modifiers;
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let shift = modifiers.contains(KeyModifiers::SHIFT);

    let key = match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter => Key::Enter,
        _ => return None,
    };

    Some(Input {
        key,
        ctrl,
        alt,
        shift,
    })
}

// --- Slash commands ---

#[allow(dead_code)]
struct SlashCommand {
    name: &'static str,
    desc: &'static str,
}

const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/new",
        desc: "Start a new thread",
    },
    SlashCommand {
        name: "/threads",
        desc: "Browse and switch threads",
    },
    SlashCommand {
        name: "/context",
        desc: "Switch context (/context <name>)",
    },
    SlashCommand {
        name: "/configure",
        desc: "Update connection settings",
    },
    SlashCommand {
        name: "/clear",
        desc: "Clear chat display",
    },
    SlashCommand {
        name: "/devtools",
        desc: "Toggle developer toolbar (F12)",
    },
    SlashCommand {
        name: "/help",
        desc: "Show available commands",
    },
    SlashCommand {
        name: "/exit",
        desc: "Exit the chat",
    },
];

fn reset_textarea(app: &mut App) {
    app.textarea = TextArea::default();
    app.textarea.set_placeholder_text(PLACEHOLDER);
    app.textarea.set_cursor_line_style(ratatui::style::Style::default());
}

fn show_help(app: &mut App) {
    app.messages.push(ChatMessage::System("Commands:".to_string()));
    for cmd in SLASH_COMMANDS {
        app.messages.push(ChatMessage::System(format!("  {:<16} {}", cmd.name, cmd.desc)));
    }
    app.messages.push(ChatMessage::System(String::new()));
    app.messages.push(ChatMessage::System("Keys:".to_string()));
    app.messages.push(ChatMessage::System("  Enter          Send message".to_string()));
    app.messages.push(ChatMessage::System("  Alt+Enter      Insert newline".to_string()));
    app.messages.push(ChatMessage::System("  Esc Esc        Cancel active run".to_string()));
    app.messages.push(ChatMessage::System("  Ctrl+C Ctrl+C  Quit".to_string()));
    app.messages.push(ChatMessage::System("  PageUp/Down    Scroll chat history".to_string()));
    app.messages.push(ChatMessage::System("  F12            Toggle devtools".to_string()));
    app.auto_scroll = true;
}

fn slash_completions(text: &str) -> Vec<usize> {
    SLASH_COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, cmd)| cmd.name.starts_with(text).then_some(i))
        .collect()
}
