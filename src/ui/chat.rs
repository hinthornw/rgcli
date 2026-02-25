use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use tokio::sync::mpsc;
use tui_textarea::{CursorMove, Input, Key, TextArea};

use crate::api::types::Attachment;
use crate::api::{Client, StreamEvent};
use crate::ui::mascot::{Parrot, ParrotState};
use crate::ui::styles;

const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const ESC_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_INPUT_LINES: usize = 5;
const PLACEHOLDER: &str = "Type a message... (Alt+Enter for newline)";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const THINKING_VERBS: &[&str] = &[
    "thinking",
    "pondering",
    "contemplating",
    "musing",
    "cogitating",
    "ruminating",
    "deliberating",
    "mulling",
    "noodling",
    "brainstorming",
];
const TOOL_RESULT_MAX_LEN: usize = 200;
const TIPS: &[&str] = &[
    "Press F12 to toggle devtools with TTFT and token metrics",
    "Drag and drop images into the terminal to attach them",
    "Use /assistant to switch between different graph assistants",
    "Use /context <name> to switch deployment contexts on the fly",
    "Pipe mode: echo \"question\" | ailsd for non-interactive use",
    "Use ailsd bench to load test your deployment",
    "Use ailsd logs to browse recent run traces",
    "Use ailsd doctor to diagnose connectivity issues",
    "Double-tap Esc to cancel a streaming response",
    "Use /export to save the conversation as markdown",
    "Press Ctrl+B to navigate between screens (threads, runs, store...)",
];

#[derive(Debug)]
pub enum ChatExit {
    Configure,
    SwitchContext(String),
    NewThread,
    Quit,
    RunDoctor,
    RunBench,
}

pub(crate) enum Action {
    None,
    Send(String),
    Cancel,
    Quit,
    Configure,
    SwitchContext(String),
    Help,
    NewThread,
    Clear,
    Attach(String),
    ListAssistants,
    SwitchAssistant(String),
    Export,
    Mode(String),
    ExitFor(ChatExit),
}

#[derive(Clone)]
pub(crate) enum ChatMessage {
    User(String),
    Assistant(String),
    System(String),
    Error(String),
    ToolUse(String, String),
    ToolResult(String, String),
}

#[derive(Clone)]
pub(crate) struct CompletionItem {
    insert: String,
    label: String,
    desc: String,
}

pub struct ChatState {
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

    // Stream mode
    stream_mode: String,

    // Search mode
    search_mode: bool,
    search_query: String,
    search_matches: Vec<usize>,

    // Mascot
    parrot: Parrot,
}

#[derive(Clone)]
pub(crate) struct ToolExec {
    name: String,
    started_at: Instant,
    completed_at: Option<Instant>,
    duration_ms: Option<u128>,
}

#[derive(Default, Clone)]
pub(crate) struct RunMetrics {
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
    // Tool timeline
    tool_timeline: Vec<ToolExec>,
    last_tool_timeline: Vec<ToolExec>,
    // Graph node tracking
    current_node: Option<String>,
    node_history: Vec<String>,
    last_node_history: Vec<String>,
}

impl ChatState {
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
            stream_mode: "messages-tuple".to_string(),
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            parrot: Parrot::new(),
        }
    }

    fn is_streaming(&self) -> bool {
        self.stream_rx.is_some()
    }

    /// Handle a terminal key event, executing any resulting actions internally.
    /// Returns a ScreenAction for the app orchestrator.
    pub fn handle_key_event(
        &mut self,
        event: Event,
        client: &Client,
        thread_id: &str,
    ) -> super::screen::ScreenAction {
        let action = handle_terminal_event(self, event);
        self.execute_action(action, client, thread_id)
    }

    /// Handle pasted text — detect file paths and auto-attach, otherwise insert as text.
    pub fn handle_paste(&mut self, text: &str) -> super::screen::ScreenAction {
        let trimmed = text.trim();

        // Check if it looks like one or more file paths (drag-and-drop)
        let mut attached_any = false;
        for line in trimmed.lines() {
            let path = line.trim().trim_matches('\'').trim_matches('"');
            if path.is_empty() {
                continue;
            }
            let p = std::path::Path::new(path);
            if p.exists() && p.is_file() {
                // Check if it's an image or supported file type
                let lower = path.to_lowercase();
                let is_attachable = lower.ends_with(".png")
                    || lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg")
                    || lower.ends_with(".gif")
                    || lower.ends_with(".webp")
                    || lower.ends_with(".pdf");
                if is_attachable {
                    handle_attach(self, path);
                    attached_any = true;
                    continue;
                }
            }
        }

        if !attached_any {
            // Not a file path — insert as regular text into textarea
            self.textarea.insert_str(trimmed);
        }

        super::screen::ScreenAction::None
    }

    fn execute_action(
        &mut self,
        action: Action,
        client: &Client,
        thread_id: &str,
    ) -> super::screen::ScreenAction {
        use super::screen::ScreenAction;
        match action {
            Action::Send(msg) => {
                self.messages.push(ChatMessage::User(msg.clone()));
                self.auto_scroll = true;
                if self.is_streaming() {
                    self.pending_messages.push_back(msg);
                    self.messages
                        .push(ChatMessage::System("(queued)".to_string()));
                } else if self.interrupted {
                    self.interrupted = false;
                    let input = if msg.trim().is_empty() {
                        None
                    } else {
                        Some(serde_json::json!({"messages": [{"role": "user", "content": msg}]}))
                    };
                    start_resume(client, thread_id, &self.assistant_id.clone(), input, self);
                } else if !self.pending_attachments.is_empty() {
                    let attachments = std::mem::take(&mut self.pending_attachments);
                    start_run_with_attachments(
                        client,
                        thread_id,
                        &self.assistant_id.clone(),
                        &msg,
                        &attachments,
                        self,
                    );
                } else {
                    start_run(
                        client,
                        thread_id,
                        &self.assistant_id.clone(),
                        &msg,
                        None,
                        self,
                    );
                }
                reset_textarea(self);
                ScreenAction::None
            }
            Action::Cancel => {
                if let Some(run_id) = self.active_run_id.clone() {
                    let client = client.clone();
                    let tid = thread_id.to_string();
                    tokio::spawn(async move {
                        let _ = client.cancel_run(&tid, &run_id).await;
                    });
                    self.messages
                        .push(ChatMessage::System("(cancelling...)".to_string()));
                }
                ScreenAction::None
            }
            Action::Quit => ScreenAction::ChatExit(ChatExit::Quit),
            Action::Configure => ScreenAction::ChatExit(ChatExit::Configure),
            Action::SwitchContext(name) => ScreenAction::ChatExit(ChatExit::SwitchContext(name)),
            Action::Help => {
                show_help(self);
                reset_textarea(self);
                ScreenAction::None
            }
            Action::NewThread => ScreenAction::ChatExit(ChatExit::NewThread),
            Action::Clear => {
                self.messages.clear();
                self.auto_scroll = true;
                reset_textarea(self);
                ScreenAction::None
            }
            Action::Attach(path) => {
                handle_attach(self, &path);
                reset_textarea(self);
                ScreenAction::None
            }
            Action::ListAssistants => {
                list_assistants(self);
                reset_textarea(self);
                ScreenAction::None
            }
            Action::SwitchAssistant(id) => {
                self.assistant_id = id.clone();
                self.messages
                    .push(ChatMessage::System(format!("Switched to assistant: {id}")));
                reset_textarea(self);
                ScreenAction::None
            }
            Action::Export => {
                export_conversation(self);
                reset_textarea(self);
                ScreenAction::None
            }
            Action::Mode(mode) => {
                self.stream_mode = mode.clone();
                self.messages
                    .push(ChatMessage::System(format!("Stream mode set to: {mode}")));
                reset_textarea(self);
                ScreenAction::None
            }
            Action::ExitFor(exit) => ScreenAction::ChatExit(exit),
            Action::None => ScreenAction::None,
        }
    }

    pub fn handle_tick(&mut self) {
        if self.is_streaming() {
            self.spinner_idx += 1;
        }
        // Update parrot state based on chat state
        if self.is_waiting || self.is_streaming() {
            self.parrot.set_state(ParrotState::Thinking);
        } else if !collect_input(&self.textarea).is_empty() {
            self.parrot.set_state(ParrotState::Typing);
        } else {
            self.parrot.set_state(ParrotState::Idle);
        }
        self.parrot.tick();
    }

    pub async fn handle_stream_event(&mut self, client: &Client, thread_id: &str) {
        if let Some(event) = recv_stream(&mut self.stream_rx).await {
            handle_stream_event(self, event, client, thread_id).await;
        }
    }

    pub fn handle_update_notice(&mut self) {
        // Try to receive an update notice without blocking
        if let Some(rx) = &mut self.update_rx {
            if let Ok(notice) = rx.try_recv() {
                self.update_notice = Some(notice);
            }
        }
    }

    pub fn draw_in_area(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let input_height = (self.textarea.lines().len().clamp(1, MAX_INPUT_LINES) as u16) + 2;
        let status_height = if self.search_mode { 2 } else { 1 };

        if self.devtools {
            let devtools_height = 3;
            let chunks = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(input_height),
                Constraint::Length(devtools_height),
                Constraint::Length(status_height),
            ])
            .split(area);

            render_chat(frame, self, chunks[0]);
            render_input(frame, self, chunks[1]);
            render_devtools(frame, self, chunks[2]);
            render_status(frame, self, chunks[3]);
        } else {
            let chunks = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(input_height),
                Constraint::Length(status_height),
            ])
            .split(area);

            render_chat(frame, self, chunks[0]);
            render_input(frame, self, chunks[1]);
            render_status(frame, self, chunks[2]);
        }
    }

    pub fn has_pending_stream(&self) -> bool {
        self.stream_rx.is_some()
    }

    pub fn parrot_mut(&mut self) -> &mut Parrot {
        &mut self.parrot
    }

    #[allow(dead_code)]
    pub fn has_pending_update(&self) -> bool {
        self.update_rx.is_some()
    }
}

pub struct ChatConfig {
    pub version: String,
    pub endpoint: String,
    pub config_path: String,
    pub context_info: String,
    pub context_names: Vec<String>,
    #[allow(dead_code)]
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

    let mut app = ChatState::new(&chat_config.context_info, update_rx);
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
    // Add a random tip
    app.welcome_lines.push(Line::default());
    app.welcome_lines.push(Line::from(Span::styled(
        format!("  tip: {}", random_tip()),
        Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

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

    let tui = super::app::TuiApp::new(app, client.clone(), thread_id.to_string());
    tui.run().await
}

// --- Drawing ---

fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_language: Option<String> = None;
    let mut code_lines: Vec<String> = Vec::new();

    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .unwrap_or_else(|| ts.themes.values().next().unwrap());

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            if in_code_block {
                // End of code block - render accumulated code with syntax highlighting
                if let Some(lang) = &code_language {
                    let syntax = ps
                        .find_syntax_by_token(lang)
                        .unwrap_or_else(|| ps.find_syntax_plain_text());
                    let mut highlighter = HighlightLines::new(syntax, theme);

                    for code_line in &code_lines {
                        let highlighted = highlighter
                            .highlight_line(code_line, &ps)
                            .unwrap_or_default();
                        let spans: Vec<Span<'static>> = highlighted
                            .into_iter()
                            .map(|(style, text)| {
                                Span::styled(format!("  {}", text), syntect_to_ratatui_style(style))
                            })
                            .collect();
                        lines.push(Line::from(spans));
                    }
                } else {
                    // No language specified, use plain green
                    for code_line in &code_lines {
                        lines.push(Line::from(Span::styled(
                            format!("  {code_line}"),
                            Style::new().fg(Color::Green),
                        )));
                    }
                }
                code_lines.clear();
                code_language = None;
                in_code_block = false;
            } else {
                // Start of code block
                in_code_block = true;
                code_language = raw_line.strip_prefix("```").map(|s| s.trim().to_string());
            }
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::new().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code_block {
            code_lines.push(raw_line.to_string());
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
        } else if let Some(item) = raw_line
            .strip_prefix("- ")
            .or_else(|| raw_line.strip_prefix("* "))
        {
            lines.push(Line::from(format!("  • {item}")));
        } else {
            // Inline formatting: **bold** and `code`
            let spans = parse_inline_markdown(raw_line);
            lines.push(Line::from(spans));
        }
    }

    // Handle unclosed code block at end of text
    if in_code_block && !code_lines.is_empty() {
        if let Some(lang) = &code_language {
            let syntax = ps
                .find_syntax_by_token(lang)
                .unwrap_or_else(|| ps.find_syntax_plain_text());
            let mut highlighter = HighlightLines::new(syntax, theme);

            for code_line in &code_lines {
                let highlighted = highlighter
                    .highlight_line(code_line, &ps)
                    .unwrap_or_default();
                let spans: Vec<Span<'static>> = highlighted
                    .into_iter()
                    .map(|(style, text)| {
                        Span::styled(format!("  {}", text), syntect_to_ratatui_style(style))
                    })
                    .collect();
                lines.push(Line::from(spans));
            }
        } else {
            for code_line in &code_lines {
                lines.push(Line::from(Span::styled(
                    format!("  {code_line}"),
                    Style::new().fg(Color::Green),
                )));
            }
        }
    }

    lines
}

fn syntect_to_ratatui_style(style: SyntectStyle) -> Style {
    let fg = style.foreground;
    Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
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

fn render_chat(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    lines.extend(app.welcome_lines.clone());

    for (idx, msg) in app.messages.iter().enumerate() {
        let is_match = app.search_mode && app.search_matches.contains(&idx);
        let highlight_style = if is_match {
            Style::new().bg(Color::Rgb(60, 60, 0))
        } else {
            Style::default()
        };

        match msg {
            ChatMessage::User(text) => {
                lines.push(Line::default());
                for line in text.lines() {
                    let spans = vec![
                        Span::styled("You: ", styles::user_style()),
                        Span::styled(line, highlight_style),
                    ];
                    lines.push(Line::from(spans));
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
                        if is_match {
                            let highlighted_spans: Vec<Span> = line
                                .spans
                                .into_iter()
                                .map(|s| Span::styled(s.content, s.style.patch(highlight_style)))
                                .collect();
                            spans.extend(highlighted_spans);
                        } else {
                            spans.extend(line.spans);
                        }
                        lines.push(Line::from(spans));
                        first = false;
                    } else if is_match {
                        let highlighted_spans: Vec<Span> = line
                            .spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.patch(highlight_style)))
                            .collect();
                        lines.push(Line::from(highlighted_spans));
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
                    Span::styled(
                        "  🔧 ",
                        Style::new().fg(Color::Yellow).patch(highlight_style),
                    ),
                    Span::styled(
                        name.as_str(),
                        Style::new()
                            .add_modifier(Modifier::BOLD)
                            .patch(highlight_style),
                    ),
                    Span::styled(
                        format!("({args_short})"),
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
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
                    Span::styled(
                        "  ← ",
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
                    Span::styled(
                        format!("{name}: "),
                        Style::new()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC)
                            .patch(highlight_style),
                    ),
                    Span::styled(
                        first_line,
                        Style::new().fg(Color::DarkGray).patch(highlight_style),
                    ),
                ]));
            }
            ChatMessage::System(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::system_style_r().patch(highlight_style),
                )));
            }
            ChatMessage::Error(text) => {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    styles::error_style_r().patch(highlight_style),
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
        let verb_idx = (app.spinner_idx / 8) % THINKING_VERBS.len();
        let verb = THINKING_VERBS[verb_idx];
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("{spinner} {verb}..."),
            styles::system_style_r(),
        )));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  tip: {}", tip_for_tick(app.spinner_idx)),
            Style::new()
                .fg(Color::Rgb(80, 80, 80))
                .add_modifier(Modifier::ITALIC),
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

fn render_input(frame: &mut ratatui::Frame, app: &mut ChatState, area: Rect) {
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

fn render_devtools(frame: &mut ratatui::Frame, app: &ChatState, area: Rect) {
    let bg = Style::new().fg(Color::White).bg(Color::Rgb(40, 40, 40));
    let dim = Style::new().fg(Color::DarkGray).bg(Color::Rgb(40, 40, 40));

    // Line 1: Metrics
    let mut line1: Vec<Span> = vec![Span::styled(" devtools ", styles::user_style())];

    if app.is_streaming() || app.is_waiting {
        if let Some(started) = app.metrics.run_started_at {
            let elapsed = started.elapsed().as_millis();
            if let Some(first) = app.metrics.first_token_at {
                let ttft = first.duration_since(started).as_millis();
                line1.push(Span::raw(format!("TTFT: {}ms ", ttft)));
                let stream_dur = first.elapsed().as_secs_f64();
                if stream_dur > 0.0 && app.metrics.token_count > 1 {
                    let tps = (app.metrics.token_count - 1) as f64 / stream_dur;
                    line1.push(Span::raw(format!("{:.0} tok/s ", tps)));
                }
                line1.push(Span::raw(format!("tokens: {} ", app.metrics.token_count)));
            } else {
                line1.push(Span::raw(format!("waiting: {}ms ", elapsed)));
            }
        }
    } else if app.metrics.last_total_ms.is_some() {
        if let Some(ttft) = app.metrics.last_ttft_ms {
            line1.push(Span::raw(format!("TTFT: {}ms ", ttft)));
        }
        if let Some(tps) = app.metrics.last_tokens_per_sec {
            line1.push(Span::raw(format!("{:.0} tok/s ", tps)));
        }
        if let Some(total) = app.metrics.last_total_ms {
            line1.push(Span::raw(format!("total: {}ms ", total)));
        }
        if let Some(count) = app.metrics.last_token_count {
            line1.push(Span::raw(format!("tokens: {} ", count)));
        }
    }

    if let Some(rid) = app
        .metrics
        .run_id
        .as_deref()
        .or(app.metrics.last_run_id.as_deref())
    {
        let short = if rid.len() > 8 { &rid[..8] } else { rid };
        line1.push(Span::styled(
            format!("run:{short}"),
            styles::system_style_r(),
        ));
    }

    // Line 2: Tool timeline
    let timeline = if app.is_streaming() || app.is_waiting {
        &app.metrics.tool_timeline
    } else {
        &app.metrics.last_tool_timeline
    };
    let mut line2: Vec<Span> = vec![Span::styled(" tools ", dim)];
    if timeline.is_empty() {
        line2.push(Span::styled("none", dim));
    } else {
        for (i, tool) in timeline.iter().enumerate() {
            if i > 0 {
                line2.push(Span::styled(" > ", dim));
            }
            let name_style = Style::new().fg(Color::Cyan).bg(Color::Rgb(40, 40, 40));
            if let Some(ms) = tool.duration_ms {
                line2.push(Span::styled(tool.name.to_string(), name_style));
                line2.push(Span::styled(
                    format!(" {}ms", ms),
                    Style::new().fg(Color::Green).bg(Color::Rgb(40, 40, 40)),
                ));
            } else {
                line2.push(Span::styled(format!("{}...", tool.name), name_style));
            }
        }
    }

    // Line 3: Node + trace link
    let node_history = if app.is_streaming() || app.is_waiting {
        &app.metrics.node_history
    } else {
        &app.metrics.last_node_history
    };
    let mut line3: Vec<Span> = Vec::new();
    if !node_history.is_empty() {
        line3.push(Span::styled(" nodes ", dim));
        line3.push(Span::styled(
            node_history.join(" > "),
            Style::new().fg(Color::Magenta).bg(Color::Rgb(40, 40, 40)),
        ));
        line3.push(Span::raw("  "));
    } else {
        line3.push(Span::styled(" ", dim));
    }
    // Trace link
    if let Some(rid) = app
        .metrics
        .run_id
        .as_deref()
        .or(app.metrics.last_run_id.as_deref())
    {
        if let Some(tid) = &app.tenant_id {
            let url = if let Some(pid) = &app.project_id {
                format!("https://smith.langchain.com/o/{tid}/projects/p/{pid}/r/{rid}")
            } else {
                format!("https://smith.langchain.com/o/{tid}/r/{rid}")
            };
            line3.push(Span::styled(
                format!("trace: {url}"),
                Style::new().fg(Color::Blue).bg(Color::Rgb(40, 40, 40)),
            ));
        }
    }

    let text = ratatui::text::Text::from(vec![
        Line::from(line1),
        Line::from(line2),
        Line::from(line3),
    ]);
    let bar = Paragraph::new(text).style(bg);
    frame.render_widget(bar, area);
}

fn render_status(frame: &mut ratatui::Frame, app: &ChatState, area: Rect) {
    if app.search_mode {
        // Split area into search bar and status bar
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

        // Render search bar
        let search_text = format!(
            " Search: {} ({} matches)",
            app.search_query,
            app.search_matches.len()
        );
        let search_line = Line::from(Span::styled(
            search_text,
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        let search_bar = Paragraph::new(search_line).style(styles::status_bar_style());
        frame.render_widget(search_bar, chunks[0]);

        // Render normal status bar
        render_status_bar(frame, app, chunks[1]);
    } else {
        render_status_bar(frame, app, area);
    }
}

fn render_status_bar(frame: &mut ratatui::Frame, app: &ChatState, area: Rect) {
    let mut left_parts: Vec<Span> = vec![Span::raw(" "), Span::raw(&app.context_name)];

    // Show current assistant
    left_parts.push(Span::styled(
        format!(" | {}", app.assistant_id),
        Style::new().fg(Color::DarkGray),
    ));

    // Show stream mode if not default
    if app.stream_mode != "messages-tuple" {
        left_parts.push(Span::styled(
            format!(" | mode:{}", app.stream_mode),
            Style::new().fg(Color::Yellow),
        ));
    }

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

    // Show trace link (short run ID) when not in devtools mode
    if !app.devtools {
        if let Some(rid) = app
            .metrics
            .last_run_id
            .as_deref()
            .or(app.metrics.run_id.as_deref())
        {
            let short = if rid.len() > 8 { &rid[..8] } else { rid };
            left_parts.push(Span::styled(
                format!(" | trace:{short}"),
                Style::new().fg(Color::Blue),
            ));
        }
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

async fn handle_stream_event(
    app: &mut ChatState,
    event: StreamEvent,
    client: &Client,
    thread_id: &str,
) {
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
            // Track tool execution start
            app.metrics.tool_timeline.push(ToolExec {
                name: name.clone(),
                started_at: Instant::now(),
                completed_at: None,
                duration_ms: None,
            });
            app.messages.push(ChatMessage::ToolUse(name, args));
            app.is_waiting = false;
            app.auto_scroll = true;
        }
        StreamEvent::ToolResult(name, content) => {
            // Complete tool execution timing
            let now = Instant::now();
            if let Some(tool) = app
                .metrics
                .tool_timeline
                .iter_mut()
                .rev()
                .find(|t| t.name == name && t.completed_at.is_none())
            {
                tool.completed_at = Some(now);
                tool.duration_ms = Some(now.duration_since(tool.started_at).as_millis());
            }
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
            // Snapshot tool timeline and node history
            app.metrics.last_tool_timeline = std::mem::take(&mut app.metrics.tool_timeline);
            app.metrics.last_node_history = std::mem::take(&mut app.metrics.node_history);
            app.metrics.current_node = None;

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
    app: &mut ChatState,
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
    app.metrics.tool_timeline.clear();
    app.metrics.node_history.clear();
    app.metrics.current_node = None;

    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let message = message.to_string();
    let strategy = multitask_strategy.map(String::from);
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .stream_run(
                &thread_id,
                &assistant_id,
                &message,
                strategy.as_deref(),
                Some(&stream_mode),
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
    app: &mut ChatState,
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
    app.metrics.tool_timeline.clear();
    app.metrics.node_history.clear();
    app.metrics.current_node = None;

    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let message = message.to_string();
    let attachments = attachments.to_vec();
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .stream_run_with_attachments(
                &thread_id,
                &assistant_id,
                &message,
                &attachments,
                Some(&stream_mode),
                &tx,
            )
            .await;
    });
}

fn start_resume(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    input: Option<serde_json::Value>,
    app: &mut ChatState,
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
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .resume_run(&thread_id, &assistant_id, input, Some(&stream_mode), &tx)
            .await;
    });
}

// --- Input handling ---

fn handle_terminal_event(app: &mut ChatState, event: Event) -> Action {
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

    // Handle search mode
    if app.search_mode {
        match key.code {
            KeyCode::Esc => {
                app.search_mode = false;
                app.search_query.clear();
                app.search_matches.clear();
                return Action::None;
            }
            KeyCode::Backspace => {
                app.search_query.pop();
                update_search_matches(app);
                return Action::None;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_query.push(c);
                update_search_matches(app);
                return Action::None;
            }
            _ => return Action::None,
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
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.search_mode = !app.search_mode;
            if !app.search_mode {
                app.search_query.clear();
                app.search_matches.clear();
            } else {
                update_search_matches(app);
            }
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
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.textarea = TextArea::default();
            app.textarea.set_placeholder_text(PLACEHOLDER);
            app.textarea
                .set_cursor_line_style(ratatui::style::Style::default());
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
            if value == "/doctor" {
                return Action::ExitFor(ChatExit::RunDoctor);
            }
            if value == "/bench" || value.starts_with("/bench ") {
                return Action::ExitFor(ChatExit::RunBench);
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
            if value == "/export" {
                return Action::Export;
            }
            if value == "/assistant" {
                return Action::ListAssistants;
            }
            if let Some(id) = value.strip_prefix("/assistant ") {
                let id = id.trim();
                if !id.is_empty() {
                    return Action::SwitchAssistant(id.to_string());
                }
            }
            if value == "/mode" {
                return Action::Help;
            }
            if let Some(mode) = value.strip_prefix("/mode ") {
                let mode = mode.trim();
                if !mode.is_empty() {
                    // Validate mode
                    let valid_modes = ["messages-tuple", "values", "updates", "events", "debug"];
                    if valid_modes.contains(&mode) {
                        return Action::Mode(mode.to_string());
                    } else {
                        app.messages.push(ChatMessage::Error(format!(
                            "Invalid mode: {mode}. Valid modes: {}",
                            valid_modes.join(", ")
                        )));
                        reset_textarea(app);
                        return Action::None;
                    }
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

fn handle_completion_key(key: &KeyEvent, app: &mut ChatState) -> Option<Action> {
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

fn update_search_matches(app: &mut ChatState) {
    app.search_matches.clear();
    if app.search_query.is_empty() {
        return;
    }

    let query_lower = app.search_query.to_lowercase();
    for (idx, msg) in app.messages.iter().enumerate() {
        let text = match msg {
            ChatMessage::User(text)
            | ChatMessage::Assistant(text)
            | ChatMessage::System(text)
            | ChatMessage::Error(text) => text,
            ChatMessage::ToolUse(name, args) => {
                let combined = format!("{} {}", name, args);
                if combined.to_lowercase().contains(&query_lower) {
                    app.search_matches.push(idx);
                }
                continue;
            }
            ChatMessage::ToolResult(name, content) => {
                let combined = format!("{} {}", name, content);
                if combined.to_lowercase().contains(&query_lower) {
                    app.search_matches.push(idx);
                }
                continue;
            }
        };

        if text.to_lowercase().contains(&query_lower) {
            app.search_matches.push(idx);
        }
    }
}

fn update_completions(app: &mut ChatState) {
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

fn handle_attach(app: &mut ChatState, path: &str) {
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

fn list_assistants(app: &mut ChatState) {
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

fn export_conversation(app: &mut ChatState) {
    let mut md = String::new();
    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                md.push_str(&format!("**You:** {text}\n\n"));
            }
            ChatMessage::Assistant(text) => {
                md.push_str(&format!("**Assistant:** {text}\n\n"));
            }
            ChatMessage::ToolUse(name, args) => {
                md.push_str(&format!("> Tool: `{name}({args})`\n\n"));
            }
            ChatMessage::ToolResult(name, content) => {
                md.push_str(&format!("> Result ({name}): {content}\n\n"));
            }
            ChatMessage::System(text) => {
                md.push_str(&format!("*{text}*\n\n"));
            }
            ChatMessage::Error(text) => {
                md.push_str(&format!("**Error:** {text}\n\n"));
            }
        }
    }

    let filename = format!(
        "ailsd-export-{}.md",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    match std::fs::write(&filename, &md) {
        Ok(()) => {
            app.messages.push(ChatMessage::System(format!(
                "Exported conversation to {filename}"
            )));
        }
        Err(e) => {
            app.messages
                .push(ChatMessage::Error(format!("Export failed: {e}")));
        }
    }
    app.auto_scroll = true;
}

fn random_tip() -> &'static str {
    // Mix seconds with a prime multiplier for better distribution
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize;
    TIPS[(secs.wrapping_mul(2654435761)) % TIPS.len()]
}

fn tip_for_tick(tick: usize) -> &'static str {
    // Rotate tips every ~8 seconds (100 ticks at 80ms interval)
    let idx = tick / 100;
    TIPS[(idx.wrapping_mul(2654435761)) % TIPS.len()]
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
        name: "/context",
        desc: "Switch context (/context <name>)",
    },
    SlashCommand {
        name: "/assistant",
        desc: "Switch assistant (/assistant <id>)",
    },
    SlashCommand {
        name: "/bench",
        desc: "Run load test",
    },
    SlashCommand {
        name: "/doctor",
        desc: "Diagnose connectivity",
    },
    SlashCommand {
        name: "/mode",
        desc: "Switch stream mode (/mode <mode>)",
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
        name: "/export",
        desc: "Export conversation to markdown",
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

fn reset_textarea(app: &mut ChatState) {
    app.textarea = TextArea::default();
    app.textarea.set_placeholder_text(PLACEHOLDER);
    app.textarea
        .set_cursor_line_style(ratatui::style::Style::default());
}

fn show_help(app: &mut ChatState) {
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
        "  Ctrl+R         Toggle search mode".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  PageUp/Down    Scroll chat history".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  F12            Toggle devtools".to_string(),
    ));
    app.auto_scroll = true;
}
