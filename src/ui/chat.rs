use std::collections::VecDeque;
use std::io::{stdout, Write};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use crossterm::execute;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::time;
use tui_textarea::{CursorMove, Input, Key, TextArea};

use crate::api::{Client, StreamEvent};
use crate::ui::styles::{assistant_prefix, print_error, system_text, user_prefix, user_prompt};

const PROMPT: &str = "> ";
const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const ESC_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_INPUT_LINES: usize = 10;
const PLACEHOLDER: &str = "Type a message... (Alt+Enter for newline)";
const SPINNER_FRAMES: [char; 4] = ['|', '/', '-', '\\'];

#[derive(Debug)]
pub enum ChatExit {
    Configure,
    Quit,
}

enum Action {
    None,
    Send(String),
    Cancel,
    Quit,
    Configure,
}

struct ChatState {
    // Input
    textarea: TextArea<'static>,
    last_rendered_lines: u16,
    ctrl_c_at: Option<Instant>,
    last_esc_at: Option<Instant>,
    completions: Vec<usize>,
    completion_idx: usize,
    show_complete: bool,
    // Streaming
    stream_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    active_run_id: Option<String>,
    first_token: bool,
    spinner_idx: usize,
    // Queue
    pending_messages: VecDeque<String>,
}

impl ChatState {
    fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text(PLACEHOLDER);
        Self {
            textarea,
            last_rendered_lines: 0,
            ctrl_c_at: None,
            last_esc_at: None,
            completions: Vec::new(),
            completion_idx: 0,
            show_complete: false,
            stream_rx: None,
            active_run_id: None,
            first_token: false,
            spinner_idx: 0,
            pending_messages: VecDeque::new(),
        }
    }

    fn is_streaming(&self) -> bool {
        self.stream_rx.is_some()
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), Show);
        let _ = terminal::disable_raw_mode();
    }
}

pub async fn run_chat_loop(
    client: &Client,
    assistant_id: &str,
    thread_id: &str,
    history: &[crate::api::Message],
) -> Result<ChatExit> {
    if !history.is_empty() {
        println!("{}", format_history(history));
        println!();
    }

    let _guard = TerminalGuard::enter()?;
    let mut state = ChatState::new();
    let mut term_events = EventStream::new();
    let mut interval = time::interval(Duration::from_millis(80));

    render_input(&mut state)?;

    loop {
        tokio::select! {
            biased;

            // Stream events from background run (highest priority)
            Some(event) = recv_stream(&mut state.stream_rx) => {
                handle_stream_event(
                    &mut state, event, client, thread_id, assistant_id,
                )?;
            }

            // Terminal input events
            Some(Ok(event)) = term_events.next() => {
                let action = handle_terminal_event(&mut state, event)?;
                match action {
                    Action::Send(msg) => {
                        clear_rendered(&mut state.last_rendered_lines)?;
                        println!("{}{}\r", user_prefix(), msg);
                        if state.is_streaming() {
                            // Queue for later with enqueue strategy
                            state.pending_messages.push_back(msg);
                            write!(stdout(), "{}\r\n", system_text("(queued)"))?;
                            stdout().flush()?;
                        } else {
                            start_run(client, thread_id, assistant_id, &msg, None, &mut state);
                        }
                        // Reset textarea
                        state.textarea = TextArea::default();
                        state.textarea.set_placeholder_text(PLACEHOLDER);
                        render_input(&mut state)?;
                    }
                    Action::Cancel => {
                        if let Some(run_id) = state.active_run_id.clone() {
                            let client = client.clone();
                            let tid = thread_id.to_string();
                            tokio::spawn(async move {
                                let _ = client.cancel_run(&tid, &run_id).await;
                            });
                            clear_rendered(&mut state.last_rendered_lines)?;
                            write!(stdout(), "\r\n{}\r\n", system_text("(cancelling...)"))?;
                            stdout().flush()?;
                            render_input(&mut state)?;
                        }
                    }
                    Action::Quit => {
                        clear_rendered(&mut state.last_rendered_lines)?;
                        // Drop guard restores terminal
                        drop(_guard);
                        println!("Goodbye!");
                        return Ok(ChatExit::Quit);
                    }
                    Action::Configure => {
                        clear_rendered(&mut state.last_rendered_lines)?;
                        return Ok(ChatExit::Configure);
                    }
                    Action::None => {
                        render_input(&mut state)?;
                    }
                }
            }

            // Spinner tick
            _ = interval.tick() => {
                if state.is_streaming() && state.first_token {
                    clear_rendered(&mut state.last_rendered_lines)?;
                    let frame = SPINNER_FRAMES[state.spinner_idx % SPINNER_FRAMES.len()];
                    state.spinner_idx += 1;
                    write!(stdout(), "\r{} Thinking...", frame)?;
                    stdout().flush()?;
                    write!(stdout(), "\r\n")?;
                    render_input(&mut state)?;
                }
            }
        }
    }
}

/// Receive from stream_rx, or pend forever if None.
async fn recv_stream(rx: &mut Option<mpsc::UnboundedReceiver<StreamEvent>>) -> Option<StreamEvent> {
    match rx {
        Some(rx) => rx.recv().await,
        None => {
            // Never resolves — makes this select arm inactive
            std::future::pending().await
        }
    }
}

fn handle_stream_event(
    state: &mut ChatState,
    event: StreamEvent,
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
) -> Result<()> {
    match event {
        StreamEvent::RunStarted(id) => {
            state.active_run_id = Some(id);
        }
        StreamEvent::Token(token) => {
            clear_rendered(&mut state.last_rendered_lines)?;
            if state.first_token {
                // Clear spinner line
                write!(stdout(), "\r\x1b[2K")?;
                write!(stdout(), "{}", assistant_prefix())?;
                state.first_token = false;
            }
            write!(stdout(), "{}", token)?;
            stdout().flush()?;
            // Move to next line so input renders below
            write!(stdout(), "\r\n")?;
            render_input(state)?;
        }
        StreamEvent::Done(result) => {
            clear_rendered(&mut state.last_rendered_lines)?;
            if state.first_token {
                // Never got a token — clear spinner
                write!(stdout(), "\r\x1b[2K")?;
            }
            if let Err(err) = result {
                write!(stdout(), "\r\n{}\r\n", print_error(&err.to_string()))?;
            } else {
                // Fetch updated thread state in background
                let client = client.clone();
                let tid = thread_id.to_string();
                tokio::spawn(async move {
                    let _ = client.get_thread(&tid, &["values"]).await;
                });
            }
            write!(stdout(), "\r\n")?;
            stdout().flush()?;

            // Clear streaming state
            state.stream_rx = None;
            state.active_run_id = None;
            state.first_token = false;

            // Drain queue
            if let Some(msg) = state.pending_messages.pop_front() {
                write!(stdout(), "{}{}\r\n", user_prefix(), msg)?;
                stdout().flush()?;
                start_run(client, thread_id, assistant_id, &msg, Some("enqueue"), state);
            }

            render_input(state)?;
        }
    }
    Ok(())
}

fn start_run(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    message: &str,
    multitask_strategy: Option<&str>,
    state: &mut ChatState,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    state.stream_rx = Some(rx);
    state.first_token = true;
    state.spinner_idx = 0;
    state.active_run_id = None;

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

fn handle_terminal_event(state: &mut ChatState, event: Event) -> Result<Action> {
    let Event::Key(key) = event else {
        return Ok(Action::None);
    };

    // Reset ctrl_c if different key
    if key.code != KeyCode::Char('c') || !key.modifiers.contains(KeyModifiers::CONTROL) {
        state.ctrl_c_at = None;
    }

    // Check ctrl_c timeout
    if let Some(start) = state.ctrl_c_at {
        if start.elapsed() > CTRL_C_TIMEOUT {
            state.ctrl_c_at = None;
        }
    }

    // Esc timeout
    if let Some(start) = state.last_esc_at {
        if start.elapsed() > ESC_TIMEOUT {
            state.last_esc_at = None;
        }
    }

    // Completion handling
    if state.show_complete
        && !state.completions.is_empty()
        && handle_completion_keys(
            &key,
            &mut state.textarea,
            &mut state.show_complete,
            &mut state.completion_idx,
            &state.completions,
        )?
    {
        update_completions(&state.textarea, &mut state.completions, &mut state.show_complete);
        return Ok(Action::None);
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if state.ctrl_c_at.is_some() {
                return Ok(Action::Quit);
            }
            state.ctrl_c_at = Some(Instant::now());
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(Action::Quit);
        }
        KeyCode::Esc => {
            if state.last_esc_at.is_some() && state.is_streaming() {
                state.last_esc_at = None;
                return Ok(Action::Cancel);
            }
            state.last_esc_at = Some(Instant::now());
            // Also dismiss completions
            state.show_complete = false;
        }
        // Alt+Enter or Ctrl+J = newline
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
            if state.textarea.lines().len() < MAX_INPUT_LINES {
                state.textarea.insert_newline();
            }
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if state.textarea.lines().len() < MAX_INPUT_LINES {
                state.textarea.insert_newline();
            }
        }
        // Enter = send
        KeyCode::Enter => {
            let value = collect_input(&state.textarea);
            if value.is_empty() {
                return Ok(Action::None);
            }
            if value == "/quit" || value == "/exit" {
                return Ok(Action::Quit);
            }
            if value == "/configure" {
                return Ok(Action::Configure);
            }
            return Ok(Action::Send(value));
        }
        KeyCode::Tab => {
            update_completions(&state.textarea, &mut state.completions, &mut state.show_complete);
            if state.show_complete {
                state.completion_idx = 0;
            }
        }
        _ => {
            if let Some(input) = to_textarea_input(key) {
                state.textarea.input(input);
            }
        }
    }

    update_completions(&state.textarea, &mut state.completions, &mut state.show_complete);
    Ok(Action::None)
}

// --- Rendering ---

fn clear_rendered(last_rendered_lines: &mut u16) -> Result<()> {
    let mut out = stdout();
    if *last_rendered_lines > 0 {
        for _ in 0..*last_rendered_lines {
            write!(out, "\x1b[A\r\x1b[2K")?;
        }
        out.flush()?;
    }
    *last_rendered_lines = 0;
    Ok(())
}

fn render_input(state: &mut ChatState) -> Result<()> {
    let mut out = stdout();

    if state.last_rendered_lines > 0 {
        for _ in 0..state.last_rendered_lines {
            write!(out, "\x1b[A\r\x1b[2K")?;
        }
    }

    let term_width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let prompt_len = PROMPT.len();
    let max_width = term_width.saturating_sub(prompt_len).max(1);

    let mut display_lines: Vec<String> = Vec::new();
    let mut line_map: Vec<(usize, usize)> = Vec::new();

    for (idx, line) in state.textarea.lines().iter().enumerate() {
        let mut remaining = line.as_str();
        if remaining.is_empty() {
            display_lines.push(String::new());
            line_map.push((idx, 0));
            continue;
        }
        let mut col = 0usize;
        while !remaining.is_empty() {
            let take = remaining.len().min(max_width);
            let (chunk, rest) = remaining.split_at(take);
            display_lines.push(chunk.to_string());
            line_map.push((idx, col));
            remaining = rest;
            col += take;
        }
    }

    if display_lines.is_empty() {
        display_lines.push(String::new());
        line_map.push((0, 0));
    }

    let (cursor_display_row, cursor_display_col) =
        raw_cursor_position(&line_map, state.textarea.cursor(), max_width);
    let total_lines = display_lines.len();
    let mut start = 0usize;
    if total_lines > MAX_INPUT_LINES && cursor_display_row + 1 > MAX_INPUT_LINES {
        start = cursor_display_row + 1 - MAX_INPUT_LINES;
    }
    let visible_lines = total_lines.min(MAX_INPUT_LINES);
    let indent = " ".repeat(prompt_len);
    let is_empty = state.textarea.is_empty();

    let mut total_output_lines = 0u16;

    for i in 0..visible_lines {
        let line = &display_lines[start + i];
        let prefix = if i == 0 { user_prompt() } else { indent.clone() };
        if is_empty && i == 0 {
            write!(out, "{}{}\r\n", prefix, system_text(PLACEHOLDER))?;
        } else {
            write!(out, "{}{}\r\n", prefix, line)?;
        }
        total_output_lines += 1;
    }

    if state.show_complete && !state.completions.is_empty() {
        write!(out, "\r\n")?;
        total_output_lines += 1;
        for (i, idx) in state.completions.iter().enumerate() {
            let cmd = &SLASH_COMMANDS[*idx];
            if i == state.completion_idx {
                write!(out, "{} {}\r\n", system_text(&format!("-> {}", cmd.name)), cmd.desc)?;
            } else {
                write!(out, "  {} {}\r\n", cmd.name, cmd.desc)?;
            }
            total_output_lines += 1;
        }
    }

    if state.ctrl_c_at.is_some() {
        write!(out, "\r\n{}\r\n", system_text("Press Ctrl+C again to exit"))?;
        total_output_lines += 2;
    }

    // Show queue count if streaming with pending messages
    if !state.pending_messages.is_empty() {
        let n = state.pending_messages.len();
        write!(out, "{}\r\n", system_text(&format!("({n} message(s) queued)")))?;
        total_output_lines += 1;
    }

    state.last_rendered_lines = total_output_lines;

    // Position cursor
    let display_row = cursor_display_row.saturating_sub(start);
    let lines_from_bottom = total_output_lines.saturating_sub(1) - display_row as u16;
    if lines_from_bottom > 0 {
        write!(out, "\x1b[{}A", lines_from_bottom)?;
    }
    let col = prompt_len + cursor_display_col + 1;
    write!(out, "\x1b[{}G", col)?;

    out.flush()?;
    Ok(())
}

// --- Input helpers ---

fn raw_input(textarea: &TextArea) -> String {
    textarea
        .lines()
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_input(textarea: &TextArea) -> String {
    raw_input(textarea).trim().to_string()
}

fn update_completions(
    textarea: &TextArea,
    completions: &mut Vec<usize>,
    show_complete: &mut bool,
) {
    let value = raw_input(textarea);
    if !value.starts_with('/') || value.contains('\n') {
        completions.clear();
        *show_complete = false;
        return;
    }
    let matches = slash_completions(&value);
    *show_complete = !matches.is_empty();
    *completions = matches;
}

fn handle_completion_keys(
    key: &KeyEvent,
    textarea: &mut TextArea,
    show_complete: &mut bool,
    completion_idx: &mut usize,
    completions: &[usize],
) -> Result<bool> {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            *completion_idx = (*completion_idx + 1) % completions.len();
            Ok(true)
        }
        KeyCode::Up | KeyCode::BackTab => {
            *completion_idx = (*completion_idx + completions.len() - 1) % completions.len();
            Ok(true)
        }
        KeyCode::Enter => {
            let cmd = SLASH_COMMANDS[completions[*completion_idx]].name;
            *textarea = TextArea::from([cmd.to_string()]);
            textarea.move_cursor(CursorMove::End);
            *show_complete = false;
            Ok(true)
        }
        KeyCode::Esc => {
            *show_complete = false;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn raw_cursor_position(
    line_map: &[(usize, usize)],
    cursor: (usize, usize),
    max_width: usize,
) -> (usize, usize) {
    let (cursor_row, cursor_col) = cursor;
    let mut display_row = 0usize;
    for (i, (row, base_col)) in line_map.iter().enumerate() {
        if *row == cursor_row && cursor_col >= *base_col && cursor_col < *base_col + max_width {
            display_row = i;
            let display_col = cursor_col - *base_col;
            return (display_row, display_col);
        }
    }
    (display_row, 0)
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

// --- History formatting ---

fn format_message(msg: &crate::api::Message) -> String {
    match msg.role.as_str() {
        "user" | "human" => format!("{}{}", user_prefix(), msg.content),
        "assistant" | "ai" => format!("{}{}", assistant_prefix(), msg.content),
        _ => format!("[{}] {}", msg.role, msg.content),
    }
}

fn format_history(messages: &[crate::api::Message]) -> String {
    let mut out = String::new();
    for (i, msg) in messages.iter().enumerate() {
        out.push_str(&format_message(msg));
        if i + 1 < messages.len() {
            out.push_str("\n\n");
        }
    }
    out
}

// --- Slash commands ---

struct SlashCommand {
    name: &'static str,
    desc: &'static str,
}

const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/configure",
        desc: "Update connection settings",
    },
    SlashCommand {
        name: "/quit",
        desc: "Exit the chat",
    },
    SlashCommand {
        name: "/exit",
        desc: "Exit the chat",
    },
];

fn slash_completions(text: &str) -> Vec<usize> {
    SLASH_COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, cmd)| cmd.name.starts_with(text).then_some(i))
        .collect()
}
