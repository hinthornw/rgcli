use std::io::{stdout, Write};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};
use tokio::sync::mpsc;
use tokio::time;
use tui_textarea::{CursorMove, Input, Key, TextArea};

use crate::api::{Client, Message};
use crate::ui::styles::{assistant_prefix, print_error, system_text, user_prefix, user_prompt};

const PROMPT: &str = "> ";
const CTRL_C_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_INPUT_LINES: usize = 10;
const PLACEHOLDER: &str = "Type a message... (Shift+Enter for newline)";

#[derive(Debug)]
pub enum ChatExit {
    Configure,
    Quit,
}

enum InputOutcome {
    Message(String),
    Configure,
    Quit,
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
    history: &[Message],
) -> Result<ChatExit> {
    if !history.is_empty() {
        println!("{}", format_history(history));
        println!();
    }

    loop {
        let input = prompt_message()?;
        match input {
            InputOutcome::Quit => {
                println!("Goodbye!");
                return Ok(ChatExit::Quit);
            }
            InputOutcome::Configure => return Ok(ChatExit::Configure),
            InputOutcome::Message(message) => {
                println!("{}{}", user_prefix(), message);
                stream_response(client, assistant_id, thread_id, &message).await?;
            }
        }
    }
}

fn prompt_message() -> Result<InputOutcome> {
    let _guard = TerminalGuard::enter()?;

    let mut textarea = TextArea::default();
    textarea.set_placeholder_text(PLACEHOLDER);

    let mut ctrl_c_at: Option<Instant> = None;
    let mut show_complete = false;
    let mut completion_idx = 0usize;
    let mut completions: Vec<usize> = Vec::new();

    let origin = crossterm::cursor::position().unwrap_or((0, 0));
    render_input(
        origin,
        &textarea,
        ctrl_c_at.is_some(),
        &completions,
        completion_idx,
        show_complete,
    )?;

    loop {
        if let Some(start) = ctrl_c_at {
            if start.elapsed() > CTRL_C_TIMEOUT {
                ctrl_c_at = None;
                render_input(
                    origin,
                    &textarea,
                    ctrl_c_at.is_some(),
                    &completions,
                    completion_idx,
                    show_complete,
                )?;
            }
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if key.code != KeyCode::Char('c') || !key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    ctrl_c_at = None;
                }

                if show_complete && !completions.is_empty() {
                    if handle_completion_keys(
                        &key,
                        &mut textarea,
                        &mut show_complete,
                        &mut completion_idx,
                        &completions,
                    )? {
                        update_completions(&textarea, &mut completions, &mut show_complete);
                        render_input(
                            origin,
                            &textarea,
                            ctrl_c_at.is_some(),
                            &completions,
                            completion_idx,
                            show_complete,
                        )?;
                        continue;
                    }
                }

                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if ctrl_c_at.is_some() {
                            return Ok(InputOutcome::Quit);
                        }
                        ctrl_c_at = Some(Instant::now());
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(InputOutcome::Quit);
                    }
                    KeyCode::Enter => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            if textarea.lines().len() < MAX_INPUT_LINES {
                                textarea.insert_newline();
                            }
                        } else {
                            let value = collect_input(&textarea);
                            if value.is_empty() {
                                continue;
                            }
                            if value == "/quit" || value == "/exit" {
                                return Ok(InputOutcome::Quit);
                            }
                            if value == "/configure" {
                                return Ok(InputOutcome::Configure);
                            }
                            return Ok(InputOutcome::Message(value));
                        }
                    }
                    KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if textarea.lines().len() < MAX_INPUT_LINES {
                            textarea.insert_newline();
                        }
                    }
                    KeyCode::Tab => {
                        update_completions(&textarea, &mut completions, &mut show_complete);
                        if show_complete {
                            completion_idx = 0;
                        }
                    }
                    _ => {
                        if let Some(input) = to_textarea_input(key) {
                            textarea.input(input);
                        }
                    }
                }

                update_completions(&textarea, &mut completions, &mut show_complete);
                render_input(
                    origin,
                    &textarea,
                    ctrl_c_at.is_some(),
                    &completions,
                    completion_idx,
                    show_complete,
                )?;
            }
            Event::Resize(_, _) => {
                render_input(
                    origin,
                    &textarea,
                    ctrl_c_at.is_some(),
                    &completions,
                    completion_idx,
                    show_complete,
                )?;
            }
            _ => {}
        }
    }
}

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

fn render_input(
    origin: (u16, u16),
    textarea: &TextArea,
    ctrl_c_armed: bool,
    completions: &[usize],
    completion_idx: usize,
    show_complete: bool,
) -> Result<()> {
    let mut out = stdout();
    queue!(out, MoveTo(origin.0, origin.1), Clear(ClearType::FromCursorDown))?;

    let term_width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let prompt_len = PROMPT.len();
    let max_width = term_width.saturating_sub(prompt_len).max(1);

    let mut display_lines: Vec<String> = Vec::new();
    let mut line_map: Vec<(usize, usize)> = Vec::new();

    for (idx, line) in textarea.lines().iter().enumerate() {
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
        raw_cursor_position(&line_map, textarea.cursor(), max_width);
    let total_lines = display_lines.len();
    let mut start = 0usize;
    if total_lines > MAX_INPUT_LINES {
        if cursor_display_row + 1 > MAX_INPUT_LINES {
            start = cursor_display_row + 1 - MAX_INPUT_LINES;
        }
    }
    let visible_lines = total_lines.min(MAX_INPUT_LINES);
    let indent = " ".repeat(prompt_len);
    let is_empty = textarea.is_empty();
    for i in 0..visible_lines {
        let line = &display_lines[start + i];
        let prefix = if i == 0 { user_prompt() } else { indent.clone() };
        if is_empty && i == 0 {
            writeln!(out, "{}{}", prefix, system_text(PLACEHOLDER))?;
        } else {
            writeln!(out, "{}{}", prefix, line)?;
        }
    }

    if show_complete && !completions.is_empty() {
        writeln!(out)?;
        for (i, idx) in completions.iter().enumerate() {
            let cmd = &SLASH_COMMANDS[*idx];
            if i == completion_idx {
                writeln!(out, "{} {}", system_text(&format!("-> {}", cmd.name)), cmd.desc)?;
            } else {
                writeln!(out, "  {} {}", cmd.name, cmd.desc)?;
            }
        }
    }

    if ctrl_c_armed {
        writeln!(out, "\n{}", system_text("Press Ctrl+C again to exit"))?;
    }

    let display_row = cursor_display_row.saturating_sub(start);
    let display_col = cursor_display_col;
    queue!(
        out,
        MoveTo(origin.0 + (prompt_len + display_col) as u16, origin.1 + display_row as u16)
    )?;
    out.flush()?;
    Ok(())
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

async fn stream_response(
    client: &Client,
    assistant_id: &str,
    thread_id: &str,
    input: &str,
) -> Result<()> {
    let mut out = stdout();
    let frames = ['|', '/', '-', '\\'];
    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let client_clone = client.clone();
    let assistant_id = assistant_id.to_string();
    let thread_id_owned = thread_id.to_string();
    let input = input.to_string();

    tokio::spawn(async move {
        let res = client_clone
            .stream_run(&thread_id_owned, &assistant_id, &input, |token| {
                let _ = tx.send(StreamEvent::Token(token));
            })
            .await;
        let _ = tx.send(StreamEvent::Done(res));
    });

    let mut first_token = true;
    let mut spinner_idx = 0usize;
    let mut interval = time::interval(Duration::from_millis(80));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if first_token {
                    let frame = frames[spinner_idx % frames.len()];
                    spinner_idx += 1;
                    print!("\r{} Thinking...", frame);
                    out.flush()?;
                }
            }
            Some(event) = rx.recv() => {
                match event {
                    StreamEvent::Token(token) => {
                        if first_token {
                            print!("\r\x1b[K");
                            print!("{}", assistant_prefix());
                            first_token = false;
                        }
                        print!("{}", token);
                        out.flush()?;
                    }
                    StreamEvent::Done(result) => {
                        if first_token {
                            print!("\r\x1b[K");
                        }
                        if let Err(err) = result {
                            println!();
                            println!("{}", print_error(&err.to_string()));
                        } else {
                            let _ = client.get_thread(thread_id, &["values"]).await;
                        }
                        println!();
                        println!();
                        return Ok(());
                    }
                }
            }
        }
    }
}

enum StreamEvent {
    Token(String),
    Done(Result<()>),
}

fn format_message(msg: &Message) -> String {
    match msg.role.as_str() {
        "user" | "human" => format!("{}{}", user_prefix(), msg.content),
        "assistant" | "ai" => format!("{}{}", assistant_prefix(), msg.content),
        _ => format!("[{}] {}", msg.role, msg.content),
    }
}

fn format_history(messages: &[Message]) -> String {
    let mut out = String::new();
    for (i, msg) in messages.iter().enumerate() {
        out.push_str(&format_message(msg));
        if i + 1 < messages.len() {
            out.push_str("\n\n");
        }
    }
    out
}

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
