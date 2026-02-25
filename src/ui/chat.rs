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
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;
use tokio::time;
use tui_textarea::{CursorMove, Input, Key, TextArea};

use crate::api::types::Attachment;
use crate::api::{Client, StreamEvent};
use crate::ui::styles;

const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const ESC_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_INPUT_LINES: usize = 5;
const PLACEHOLDER: &str = "Type a message... (Alt+Enter for newline)";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TOOL_RESULT_MAX_LEN: usize = 200;

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
    Attach(String),
    ListAssistants,
    SwitchAssistant(String),
}

#[derive(Clone)]
enum ChatMessage {
    User(String),
    Assistant(String),
    System(String),
    Error(String),
    ToolUse(String, String),
    ToolResult(String, String),
}

#[derive(Clone)]
struct CompletionItem {
    insert: String,
    label: String,
    desc: String,
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
    completions: Vec<CompletionItem>,
    completion_idx: usize,
    show_complete: bool,

    // Queue & status
    pending_messages: VecDeque<String>,
    update_notice: Option<String>,
    update_rx: Option<mpsc::UnboundedReceiver<String>>,
    context_name: String,
    welcome_lines: Vec<Line<'static>>,
    context_names: Vec<String>,

    // Assistants
    assistant_id: String,
    available_assistants: Vec<(String, String)>, // (id, name/graph_id)

    // Attachments
    pending_attachments: Vec<Attachment>,

    // Human-in-the-loop
    interrupted: bool,

    // Trace info (from /info endpoint)
    tenant_id: Option<String>,
    project_id: Option<String>,

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
            context_names: Vec::new(),
            assistant_id: String::new(),
            available_assistants: Vec::new(),
            pending_attachments: Vec::new(),
            interrupted: false,
            tenant_id: None,
            project_id: None,
            devtools: false,
            metrics: RunMetrics::default(),
        }
    }

    fn is_streaming(&self) -> bool {
        self.stream_rx.is_some()
    }
}

#[allow(dead_code)]
pub struct ChatConfig {
    pub version: String,
    pub endpoint: String,
    pub config_path: String,
    pub context_info: String,
    pub context_names: Vec<String>,
    pub assistant_id: String,
    pub available_assistants: Vec<(String, String)>,
    pub tenant_id: Option<String>,
    pub project_id: Option<String>,
}

pub async fn run_chat_loop(
    client: &Client,
    assistant_id: &str,
    thread_id: &str,
    history: &[crate::api::Message],
    chat_config: &ChatConfig,
) -> Result<ChatExit> {
    let (update_tx, update_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let _ = check_for_updates_loop(update_tx).await;
    });

    let mut app = App::new(&chat_config.context_info, update_rx);
    app.assistant_id = assistant_id.to_string();
    app.available_assistants = chat_config.available_assistants.clone();
    app.tenant_id = chat_config.tenant_id.clone();
    app.project_id = chat_config.project_id.clone();

    // Fetch deployment info
    let deploy_info = match client.get_info().await {
        Ok(info) => {
            let mut parts = Vec::new();
            if let Some(v) = info.get("langgraph_api_version").and_then(|v| v.as_str()) {
                parts.push(format!("langgraph: {v}"));
            } else if let Some(v) = info.get("version").and_then(|v| v.as_str()) {
                parts.push(format!("api: {v}"));
            }

            // Extract tenant/project for trace links
            if let (Some(pid), Some(tid)) = (
                info.get("host")
                    .and_then(|h| h.get("project_id"))
                    .and_then(|v| v.as_str()),
                info.get("host")
                    .and_then(|h| h.get("tenant_id"))
                    .and_then(|v| v.as_str()),
            ) {
                app.tenant_id = Some(tid.to_string());
                app.project_id = Some(pid.to_string());

                if let Ok(project) = client.get_project_details(pid, tid).await {
                    if let Some(name) = project.get("name").and_then(|v| v.as_str()) {
                        parts.push(name.to_string());
                    }
                    if let Some(status) = project.get("status").and_then(|v| v.as_str()) {
                        parts.push(status.to_string());
                    }
                    if let Some(branch) = project.get("repo_branch").and_then(|v| v.as_str()) {
                        parts.push(format!("branch: {branch}"));
                    }
                }
            }

            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        }
        Err(_) => None,
    };

    app.welcome_lines = styles::logo_lines(
        &chat_config.version,
        &chat_config.endpoint,
        &chat_config.config_path,
        &chat_config.context_info,
        deploy_info.as_deref(),
    );
    app.context_names = chat_config.context_names.clone();

    // Load history with tool call support
    for msg in history {
        match msg.role.as_str() {
            "user" | "human" => app.messages.push(ChatMessage::User(msg.content.clone())),
            "assistant" | "ai" => {
                // Show tool calls if present
                for tc in &msg.tool_calls {
                    app.messages
                        .push(ChatMessage::ToolUse(tc.name.clone(), tc.args.clone()));
                }
                if !msg.content.is_empty() {
                    app.messages
                        .push(ChatMessage::Assistant(msg.content.clone()));
                }
            }
            "tool" => {
                let name = msg.tool_name.clone().unwrap_or_else(|| "tool".to_string());
                app.messages
                    .push(ChatMessage::ToolResult(name, msg.content.clone()));
            }
            _ => app.messages.push(ChatMessage::System(format!(
                "[{}] {}",
                msg.role, msg.content
            ))),
        }
    }

    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_event_loop(&mut terminal, &mut app, client, thread_id).await;

    execute!(stdout(), LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    client: &Client,
    thread_id: &str,
) -> Result<ChatExit> {
    let mut term_events = EventStream::new();
    let mut interval = time::interval(Duration::from_millis(80));

    terminal.draw(|f| draw(f, app))?;

    loop {
        tokio::select! {
            biased;

            Some(notice) = recv_update(&mut app.update_rx) => {
                app.update_notice = Some(notice);
                terminal.draw(|f| draw(f, app))?;
            }

            Some(event) = recv_stream(&mut app.stream_rx) => {
                handle_stream_event(app, event, client, thread_id).await;
                terminal.draw(|f| draw(f, app))?;
            }

            Some(Ok(event)) = term_events.next() => {
                let action = handle_terminal_event(app, event);
                match action {
                    Action::Send(msg) => {
                        app.messages.push(ChatMessage::User(msg.clone()));
                        app.auto_scroll = true;
                        if app.is_streaming() {
                            app.pending_messages.push_back(msg);
                            app.messages.push(ChatMessage::System("(queued)".to_string()));
                        } else if app.interrupted {
                            // Resume interrupted graph
                            app.interrupted = false;
                            let input = if msg.trim().is_empty() {
                                None
                            } else {
                                Some(serde_json::json!({"messages": [{"role": "user", "content": msg}]}))
                            };
                            start_resume(client, thread_id, &app.assistant_id.clone(), input, app);
                        } else if !app.pending_attachments.is_empty() {
                            let attachments = std::mem::take(&mut app.pending_attachments);
                            start_run_with_attachments(
                                client,
                                thread_id,
                                &app.assistant_id.clone(),
                                &msg,
                                &attachments,
                                app,
                            );
                        } else {
                            start_run(client, thread_id, &app.assistant_id.clone(), &msg, None, app);
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
                    Action::Quit => return Ok(ChatExit::Quit),
                    Action::Configure => return Ok(ChatExit::Configure),
                    Action::SwitchContext(name) => return Ok(ChatExit::SwitchContext(name)),
                    Action::Help => {
                        show_help(app);
                        reset_textarea(app);
                    }
                    Action::NewThread => return Ok(ChatExit::NewThread),
                    Action::PickThread => return Ok(ChatExit::PickThread),
                    Action::Clear => {
                        app.messages.clear();
                        app.auto_scroll = true;
                        reset_textarea(app);
                    }
                    Action::Attach(path) => {
                        handle_attach(app, &path);
                        reset_textarea(app);
                    }
                    Action::ListAssistants => {
                        list_assistants(app);
                        reset_textarea(app);
                    }
                    Action::SwitchAssistant(id) => {
                        app.assistant_id = id.clone();
                        app.messages.push(ChatMessage::System(
                            format!("Switched to assistant: {id}"),
                        ));
                        reset_textarea(app);
                    }
                    Action::None => {}
                }
                terminal.draw(|f| draw(f, app))?;
            }

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

fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            in_code_block = !in_code_block;
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::new().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  {raw_line}"),
                Style::new().fg(Color::Green),
            )));
            continue;
        }

        // Headers
        if let Some(h) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            )));
        } else if let Some(h) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            )));
        } else if let Some(h) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                h.to_string(),
                Style::new()
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    .fg(Color::Cyan),
            )));
        } else if let Some(item) = raw_line.strip_prefix("- ").or_else(|| raw_line.strip_prefix("* ")) {
            lines.push(Line::from(format!("  • {item}")));
        } else {
            // Inline formatting: **bold** and `code`
            let spans = parse_inline_markdown(raw_line);
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // Look for **bold** or `code`
        if let Some(pos) = remaining.find("**") {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 2..];
            if let Some(end) = after.find("**") {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::new().add_modifier(Modifier::BOLD),
                ));
                remaining = &after[end + 2..];
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                break;
            }
        } else if let Some(pos) = remaining.find('`') {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 1..];
            if let Some(end) = after.find('`') {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::new().fg(Color::Yellow),
                ));
                remaining = &after[end + 1..];
            } else {
                spans.push(Span::raw(remaining[pos..].to_string()));
                break;
            }
        } else {
            spans.push(Span::raw(remaining.to_string()));
            break;
        }
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

fn render_chat(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

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
                let md_lines = render_markdown_lines(text);
                let mut first = true;
                for line in md_lines {
                    if first {
                        let mut spans =
                            vec![Span::styled("Assistant: ", styles::assistant_style())];
                        spans.extend(line.spans);
                        lines.push(Line::from(spans));
                        first = false;
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
                    Span::styled("  🔧 ", Style::new().fg(Color::Yellow)),
                    Span::styled(name.as_str(), Style::new().add_modifier(Modifier::BOLD)),
                    Span::styled(format!("({args_short})"), Style::new().fg(Color::DarkGray)),
                ]));
            }
            ChatMessage::ToolResult(name, content) => {
                let truncated = if content.len() > TOOL_RESULT_MAX_LEN {
                    format!("{}...", &content[..TOOL_RESULT_MAX_LEN - 3])
                } else {
                    content.clone()
                };
                // Show first line only, indented
                let first_line = truncated.lines().next().unwrap_or("").to_string();
                lines.push(Line::from(vec![
                    Span::styled("  ← ", Style::new().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{name}: "),
                        Style::new()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                    Span::styled(first_line, Style::new().fg(Color::DarkGray)),
                ]));
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

    // Streaming content with markdown rendering
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

fn render_devtools(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut parts: Vec<Span> = vec![Span::styled(" devtools ", styles::user_style())];

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
                parts.push(Span::raw(format!("tokens: {} ", app.metrics.token_count)));
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

    if let Some(rid) = app
        .metrics
        .run_id
        .as_deref()
        .or(app.metrics.last_run_id.as_deref())
    {
        let short = if rid.len() > 8 { &rid[..8] } else { rid };
        parts.push(Span::styled(
            format!("run:{short}"),
            styles::system_style_r(),
        ));
    }

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(Style::new().fg(Color::White).bg(Color::Rgb(40, 40, 40)));
    frame.render_widget(bar, area);
}

fn render_status(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut left_parts: Vec<Span> = vec![Span::raw(" "), Span::raw(&app.context_name)];

    // Show current assistant
    left_parts.push(Span::styled(
        format!(" | {}", app.assistant_id),
        Style::new().fg(Color::DarkGray),
    ));

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

async fn handle_stream_event(app: &mut App, event: StreamEvent, client: &Client, thread_id: &str) {
    match event {
        StreamEvent::RunStarted(id) => {
            app.active_run_id = Some(id.clone());
            app.metrics.run_id = Some(id);
        }
        StreamEvent::NewMessage(_id) => {
            if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
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
        StreamEvent::ToolUse(name, args) => {
            // Flush any pending text first
            if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
            app.messages.push(ChatMessage::ToolUse(name, args));
            app.is_waiting = false;
            app.auto_scroll = true;
        }
        StreamEvent::ToolResult(name, content) => {
            app.messages.push(ChatMessage::ToolResult(name, content));
            app.auto_scroll = true;
        }
        StreamEvent::Done(result) => {
            if let Err(ref err) = result {
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

            // Snapshot metrics
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

            // Trace link (devtools only)
            if app.devtools {
                if let (Some(run_id), Some(tid)) = (&app.metrics.run_id, &app.tenant_id) {
                    let url = if let Some(pid) = &app.project_id {
                        format!("https://smith.langchain.com/o/{tid}/projects/p/{pid}/r/{run_id}")
                    } else {
                        format!("https://smith.langchain.com/o/{tid}/r/{run_id}")
                    };
                    app.messages
                        .push(ChatMessage::System(format!("trace: {url}")));
                }
            }

            // Check for human-in-the-loop interrupt
            if result.is_ok() {
                let assistant_id = app.assistant_id.clone();
                if let Ok(state) = client.get_thread_state(thread_id).await {
                    if let Some(next) = &state.next {
                        if !next.is_empty() {
                            let nodes = next.join(", ");
                            app.messages.push(ChatMessage::System(format!(
                                "⏸ Graph paused at: {nodes}. Press Enter to continue or type a response."
                            )));
                            app.interrupted = true;
                            return;
                        }
                    }
                }
                let _ = assistant_id; // suppress unused warning
            }

            // Drain queue
            if let Some(msg) = app.pending_messages.pop_front() {
                if let Some(ChatMessage::System(s)) = app.messages.last() {
                    if s == "(queued)" {
                        app.messages.pop();
                    }
                }
                let aid = app.assistant_id.clone();
                app.messages.push(ChatMessage::User(msg.clone()));
                start_run(client, thread_id, &aid, &msg, Some("enqueue"), app);
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

fn start_run_with_attachments(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    message: &str,
    attachments: &[Attachment],
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
    let attachments = attachments.to_vec();

    tokio::spawn(async move {
        client
            .stream_run_with_attachments(&thread_id, &assistant_id, &message, &attachments, &tx)
            .await;
    });
}

fn start_resume(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    input: Option<serde_json::Value>,
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

    tokio::spawn(async move {
        client
            .resume_run(&thread_id, &assistant_id, input, &tx)
            .await;
    });
}

// --- Input handling ---

fn handle_terminal_event(app: &mut App, event: Event) -> Action {
    let Event::Key(key) = event else {
        return Action::None;
    };

    if key.code != KeyCode::Char('c') || !key.modifiers.contains(KeyModifiers::CONTROL) {
        app.ctrl_c_at = None;
    }

    if let Some(start) = app.ctrl_c_at {
        if start.elapsed() > CTRL_C_TIMEOUT {
            app.ctrl_c_at = None;
        }
    }

    if let Some(start) = app.last_esc_at {
        if start.elapsed() > ESC_TIMEOUT {
            app.last_esc_at = None;
        }
    }

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
        KeyCode::Enter => {
            let value = collect_input(&app.textarea);
            // Allow empty Enter to resume interrupted graph
            if value.is_empty() && app.interrupted {
                return Action::Send(String::new());
            }
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
            if let Some(path) = value.strip_prefix("/attach ") {
                let path = path.trim();
                if !path.is_empty() {
                    return Action::Attach(path.to_string());
                }
            }
            if value == "/assistants" || value == "/assistant" {
                return Action::ListAssistants;
            }
            if let Some(id) = value.strip_prefix("/assistant ") {
                let id = id.trim();
                if !id.is_empty() {
                    return Action::SwitchAssistant(id.to_string());
                }
            }
            return Action::Send(value);
        }
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
            update_completions(app);
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

    update_completions(app);
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
            let insert = app.completions[app.completion_idx].insert.clone();
            app.textarea = TextArea::from([insert]);
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

// --- Helpers ---

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

fn update_completions(app: &mut App) {
    let value: String = app
        .textarea
        .lines()
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if !value.starts_with('/') || value.contains('\n') {
        app.completions.clear();
        app.show_complete = false;
        return;
    }

    // Context autocompletion
    if let Some(prefix) = value.strip_prefix("/context ") {
        let prefix = prefix.to_lowercase();
        let matches: Vec<CompletionItem> = app
            .context_names
            .iter()
            .filter(|name| name.to_lowercase().starts_with(&prefix))
            .map(|name| CompletionItem {
                insert: format!("/context {name}"),
                label: name.clone(),
                desc: "switch context".to_string(),
            })
            .collect();
        app.show_complete = !matches.is_empty();
        app.completions = matches;
        return;
    }

    // Assistant autocompletion
    if let Some(prefix) = value.strip_prefix("/assistant ") {
        let prefix = prefix.to_lowercase();
        let matches: Vec<CompletionItem> = app
            .available_assistants
            .iter()
            .filter(|(_, name)| name.to_lowercase().starts_with(&prefix))
            .map(|(id, name)| CompletionItem {
                insert: format!("/assistant {id}"),
                label: name.clone(),
                desc: id.clone(),
            })
            .collect();
        app.show_complete = !matches.is_empty();
        app.completions = matches;
        return;
    }

    let matches: Vec<CompletionItem> = SLASH_COMMANDS
        .iter()
        .filter(|cmd| cmd.name.starts_with(&value))
        .map(|cmd| CompletionItem {
            insert: cmd.name.to_string(),
            label: cmd.name.to_string(),
            desc: cmd.desc.to_string(),
        })
        .collect();
    app.show_complete = !matches.is_empty();
    app.completions = matches;
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

fn handle_attach(app: &mut App, path: &str) {
    let expanded = if path.starts_with('~') {
        if let Some(home) = dirs_home() {
            path.replacen('~', &home, 1)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    match std::fs::read(&expanded) {
        Ok(data) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            let mime = guess_mime(&expanded);
            let filename = std::path::Path::new(&expanded)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string());
            app.messages.push(ChatMessage::System(format!(
                "📎 Attached: {filename} ({mime}, {} bytes)",
                data.len()
            )));
            app.pending_attachments.push(Attachment {
                filename,
                mime_type: mime,
                base64_data: b64,
            });
        }
        Err(e) => {
            app.messages.push(ChatMessage::Error(format!(
                "Failed to read {expanded}: {e}"
            )));
        }
    }
}

fn guess_mime(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".pdf") {
        "application/pdf".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

fn list_assistants(app: &mut App) {
    if app.available_assistants.is_empty() {
        app.messages
            .push(ChatMessage::System("No assistants found.".to_string()));
    } else {
        app.messages
            .push(ChatMessage::System("Available assistants:".to_string()));
        for (id, name) in &app.available_assistants {
            let current = if *id == app.assistant_id {
                " (current)"
            } else {
                ""
            };
            app.messages
                .push(ChatMessage::System(format!("  {name} [{id}]{current}")));
        }
        app.messages.push(ChatMessage::System(
            "Use /assistant <id> to switch.".to_string(),
        ));
    }
    app.auto_scroll = true;
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
        name: "/assistant",
        desc: "List or switch assistants",
    },
    SlashCommand {
        name: "/attach",
        desc: "Attach a file (/attach <path>)",
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
    app.textarea
        .set_cursor_line_style(ratatui::style::Style::default());
}

fn show_help(app: &mut App) {
    app.messages
        .push(ChatMessage::System("Commands:".to_string()));
    for cmd in SLASH_COMMANDS {
        app.messages.push(ChatMessage::System(format!(
            "  {:<16} {}",
            cmd.name, cmd.desc
        )));
    }
    app.messages.push(ChatMessage::System(String::new()));
    app.messages.push(ChatMessage::System("Keys:".to_string()));
    app.messages.push(ChatMessage::System(
        "  Enter          Send message".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  Alt+Enter      Insert newline".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  Esc Esc        Cancel active run".to_string(),
    ));
    app.messages
        .push(ChatMessage::System("  Ctrl+C Ctrl+C  Quit".to_string()));
    app.messages.push(ChatMessage::System(
        "  PageUp/Down    Scroll chat history".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  F12            Toggle devtools".to_string(),
    ));
    app.auto_scroll = true;
}
