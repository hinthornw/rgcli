mod helpers;
mod input;
mod markdown;
mod render;
mod streaming;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::api::types::Attachment;
use crate::api::{Client, StreamEvent};
use crate::ui::mascot::{Parrot, ParrotState};
use crate::ui::styles;

pub(crate) use streaming::RunMetrics;

const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const ESC_TIMEOUT: Duration = Duration::from_millis(500);
const PREFIX_TIMEOUT: Duration = Duration::from_secs(1);
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
    pub(crate) insert: String,
    pub(crate) label: String,
    pub(crate) desc: String,
}

pub struct ChatState {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) scroll_offset: u16,
    pub(crate) auto_scroll: bool,

    // Streaming
    pub(crate) stream_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    pub(crate) active_run_id: Option<String>,
    pub(crate) streaming_text: String,
    pub(crate) spinner_idx: usize,
    pub(crate) is_waiting: bool,

    // Input
    pub(crate) textarea: TextArea<'static>,

    // Keys
    pub(crate) ctrl_c_at: Option<Instant>,
    pub(crate) last_esc_at: Option<Instant>,

    // Completions
    pub(crate) completions: Vec<CompletionItem>,
    pub(crate) completion_idx: usize,
    pub(crate) show_complete: bool,

    // Queue & status
    pub(crate) pending_messages: VecDeque<String>,
    pub(crate) update_notice: Option<String>,
    pub(crate) update_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub(crate) context_name: String,
    pub(crate) welcome_lines: Vec<Line<'static>>,
    pub(crate) context_names: Vec<String>,

    // Assistants
    pub(crate) assistant_id: String,
    pub(crate) available_assistants: Vec<(String, String)>,

    // Attachments
    pub(crate) pending_attachments: Vec<Attachment>,

    // Human-in-the-loop
    pub(crate) interrupted: bool,

    // Trace info
    pub(crate) tenant_id: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) tracer_session_id: Option<String>,

    // Dev toolbar
    pub(crate) devtools: bool,
    pub(crate) metrics: RunMetrics,

    // Stream mode
    pub(crate) stream_mode: String,

    // Feedback
    pub(crate) feedback_submitted: Option<bool>,

    // Scroll mode (tmux-style Ctrl+B [)
    pub(crate) scroll_mode: bool,
    pub(crate) prefix_at: Option<Instant>,

    // Search mode
    pub(crate) search_mode: bool,
    pub(crate) search_query: String,
    pub(crate) search_matches: Vec<usize>,
    pub(crate) search_match_idx: usize,

    // Mascot
    pub(crate) parrot: Parrot,

    // Thread history loading
    pub(crate) history_rx: Option<mpsc::UnboundedReceiver<crate::api::types::ThreadState>>,
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
            tracer_session_id: None,
            devtools: false,
            metrics: RunMetrics::default(),
            stream_mode: "messages-tuple".to_string(),
            feedback_submitted: None,
            scroll_mode: false,
            prefix_at: None,
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_match_idx: 0,
            parrot: Parrot::new(),
            history_rx: None,
        }
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.stream_rx.is_some()
    }

    pub fn handle_key_event(
        &mut self,
        event: crossterm::event::Event,
        client: &Client,
        thread_id: &str,
    ) -> super::screen::ScreenAction {
        let action = input::handle_terminal_event(self, event);
        self.execute_action(action, client, thread_id)
    }

    pub fn handle_paste(&mut self, text: &str) -> super::screen::ScreenAction {
        let trimmed = text.trim();

        let mut attached_any = false;
        for line in trimmed.lines() {
            let path = line.trim().trim_matches('\'').trim_matches('"');
            if path.is_empty() {
                continue;
            }
            let p = std::path::Path::new(path);
            if p.exists() && p.is_file() {
                let lower = path.to_lowercase();
                let is_attachable = lower.ends_with(".png")
                    || lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg")
                    || lower.ends_with(".gif")
                    || lower.ends_with(".webp")
                    || lower.ends_with(".pdf");
                if is_attachable {
                    helpers::handle_attach(self, path);
                    attached_any = true;
                    continue;
                }
            }
        }

        if !attached_any {
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
                    streaming::start_resume(
                        client,
                        thread_id,
                        &self.assistant_id.clone(),
                        input,
                        self,
                    );
                } else if !self.pending_attachments.is_empty() {
                    let attachments = std::mem::take(&mut self.pending_attachments);
                    streaming::start_run_with_attachments(
                        client,
                        thread_id,
                        &self.assistant_id.clone(),
                        &msg,
                        &attachments,
                        self,
                    );
                } else {
                    streaming::start_run(
                        client,
                        thread_id,
                        &self.assistant_id.clone(),
                        &msg,
                        None,
                        self,
                    );
                }
                helpers::reset_textarea(self);
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
                helpers::show_help(self);
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::NewThread => ScreenAction::ChatExit(ChatExit::NewThread),
            Action::Clear => {
                self.messages.clear();
                self.auto_scroll = true;
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::Attach(path) => {
                helpers::handle_attach(self, &path);
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::ListAssistants => {
                helpers::list_assistants(self);
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::SwitchAssistant(id) => {
                self.assistant_id = id.clone();
                self.messages
                    .push(ChatMessage::System(format!("Switched to assistant: {id}")));
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::Export => {
                helpers::export_conversation(self);
                helpers::reset_textarea(self);
                ScreenAction::None
            }
            Action::Mode(mode) => {
                self.stream_mode = mode.clone();
                self.messages
                    .push(ChatMessage::System(format!("Stream mode set to: {mode}")));
                helpers::reset_textarea(self);
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
        if self.is_waiting || self.is_streaming() {
            self.parrot.set_state(ParrotState::Thinking);
        } else if !input::collect_input(&self.textarea).is_empty() {
            self.parrot.set_state(ParrotState::Typing);
        } else {
            self.parrot.set_state(ParrotState::Idle);
        }
        self.parrot.tick();
    }

    pub async fn handle_stream_event(&mut self, client: &Client, thread_id: &str) {
        if let Some(event) = streaming::recv_stream(&mut self.stream_rx).await {
            streaming::handle_stream_event(self, event, client, thread_id).await;
        }
    }

    pub fn handle_update_notice(&mut self) {
        if let Some(rx) = &mut self.update_rx {
            if let Ok(notice) = rx.try_recv() {
                self.update_notice = Some(notice);
            }
        }
    }

    pub fn draw_in_area(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let input_height = (self.textarea.lines().len().clamp(1, MAX_INPUT_LINES) as u16) + 2;
        let status_height = if self.search_mode || self.scroll_mode { 2 } else { 1 };

        if self.devtools {
            let devtools_height = 4;
            let chunks = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(input_height),
                Constraint::Length(devtools_height),
                Constraint::Length(status_height),
            ])
            .split(area);

            render::render_chat(frame, self, chunks[0]);
            render::render_input(frame, self, chunks[1]);
            render::render_devtools(frame, self, chunks[2]);
            render::render_status(frame, self, chunks[3]);
        } else {
            let chunks = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(input_height),
                Constraint::Length(status_height),
            ])
            .split(area);

            render::render_chat(frame, self, chunks[0]);
            render::render_input(frame, self, chunks[1]);
            render::render_status(frame, self, chunks[2]);
        }
    }

    pub fn has_pending_stream(&self) -> bool {
        self.stream_rx.is_some()
    }

    pub fn parrot_mut(&mut self) -> &mut Parrot {
        &mut self.parrot
    }

    /// Start an async fetch of thread state to populate chat history.
    pub fn load_thread_history(&mut self, client: &Client, thread_id: &str) {
        self.messages.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.interrupted = false;
        self.streaming_text.clear();
        self.stream_rx = None;
        self.active_run_id = None;
        self.is_waiting = true;
        helpers::reset_textarea(self);

        let client = client.clone();
        let tid = thread_id.to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        self.history_rx = Some(rx);
        tokio::spawn(async move {
            match client.get_thread_state(&tid).await {
                Ok(state) => {
                    let _ = tx.send(state);
                }
                Err(_) => {
                    // Thread may be empty or not exist yet — that's fine
                }
            }
        });
    }

    /// Poll for thread history load completion.
    pub fn poll_history(&mut self) {
        if let Some(rx) = &mut self.history_rx {
            if let Ok(state) = rx.try_recv() {
                let messages = crate::api::types::get_messages(&state.values);
                for msg in &messages {
                    match msg.role.as_str() {
                        "user" | "human" => {
                            self.messages.push(ChatMessage::User(msg.content.clone()));
                        }
                        "assistant" | "ai" => {
                            for tc in &msg.tool_calls {
                                self.messages
                                    .push(ChatMessage::ToolUse(tc.name.clone(), tc.args.clone()));
                            }
                            if !msg.content.is_empty() {
                                self.messages
                                    .push(ChatMessage::Assistant(msg.content.clone()));
                            }
                        }
                        "tool" => {
                            let name =
                                msg.tool_name.clone().unwrap_or_else(|| "tool".to_string());
                            self.messages
                                .push(ChatMessage::ToolResult(name, msg.content.clone()));
                        }
                        _ => {
                            self.messages.push(ChatMessage::System(format!(
                                "[{}] {}",
                                msg.role, msg.content
                            )));
                        }
                    }
                }

                // Check if thread is interrupted (has pending next nodes)
                if let Some(next) = &state.next {
                    if !next.is_empty() {
                        self.interrupted = true;
                        self.messages.push(ChatMessage::System(
                            "Thread is waiting for input. Press Enter to resume.".to_string(),
                        ));
                    }
                }

                self.is_waiting = false;
                self.history_rx = None;
            }
        }
    }
}

pub struct ChatConfig {
    pub version: String,
    pub endpoint: String,
    pub config_path: String,
    pub context_info: String,
    pub context_names: Vec<String>,
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
        let _ = streaming::check_for_updates_loop(update_tx).await;
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
                crate::debug_log::log("chat", &format!("tenant={tid} project={pid}"));

                if let Ok(project) = client.get_project_details(pid, tid).await {
                    if let Some(name) = project.get("name").and_then(|v| v.as_str()) {
                        parts.push(name.to_string());
                        crate::debug_log::log("chat", &format!("deployment name: {name}"));
                        // Look up the LangSmith tracer session ID by project name
                        match client.get_tracer_session_id(name, tid).await {
                            Ok(Some(session_id)) => {
                                crate::debug_log::log(
                                    "chat",
                                    &format!("tracer session: {session_id}"),
                                );
                                app.tracer_session_id = Some(session_id);
                            }
                            Ok(None) => {
                                crate::debug_log::log(
                                    "chat",
                                    "tracer session not found for deployment name",
                                );
                            }
                            Err(e) => {
                                crate::debug_log::log(
                                    "chat",
                                    &format!("tracer session lookup failed: {e}"),
                                );
                            }
                        }
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
    app.welcome_lines.push(Line::default());
    app.welcome_lines.push(Line::from(Span::styled(
        format!("  tip: {}", helpers::random_tip()),
        Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

    app.context_names = chat_config.context_names.clone();

    // Load history
    for msg in history {
        match msg.role.as_str() {
            "user" | "human" => app.messages.push(ChatMessage::User(msg.content.clone())),
            "assistant" | "ai" => {
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
