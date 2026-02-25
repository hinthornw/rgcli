use std::io::{stdout, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};

use crate::api::{get_messages, Thread};
use crate::ui::styles::system_text;

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

pub fn pick_thread(threads: &[Thread]) -> Result<Option<Thread>> {
    if threads.is_empty() {
        return Ok(None);
    }

    let _guard = TerminalGuard::enter()?;
    let mut cursor = 0usize;
    let origin = crossterm::cursor::position().unwrap_or((0, 0));

    render_picker(origin, threads, cursor)?;

    loop {
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                }
                KeyCode::Down => {
                    if cursor + 1 < threads.len() {
                        cursor += 1;
                    }
                }
                KeyCode::Tab => {
                    if cursor + 1 < threads.len() {
                        cursor += 1;
                    }
                }
                KeyCode::BackTab => {
                    cursor = cursor.saturating_sub(1);
                }
                KeyCode::Enter => return Ok(Some(threads[cursor].clone())),
                _ => {}
            }
            render_picker(origin, threads, cursor)?;
        }
    }
}

fn render_picker(origin: (u16, u16), threads: &[Thread], cursor: usize) -> Result<()> {
    let mut out = stdout();
    queue!(out, MoveTo(origin.0, origin.1), Clear(ClearType::FromCursorDown))?;
    writeln!(out, "Select a thread to resume:\n")?;

    for (i, thread) in threads.iter().enumerate() {
        let preview = thread_preview(thread);
        let thread_id = thread.thread_id.chars().take(8).collect::<String>();
        let line = format!("{} - {}", thread_id, preview);
        if i == cursor {
            writeln!(out, "{}", system_text(&format!("> {}", line)))?;
        } else {
            writeln!(out, "  {}", line)?;
        }
    }

    writeln!(
        out,
        "\n{}",
        system_text("(↑/↓ to move, enter to select, esc to cancel)")
    )?;
    out.flush()?;
    Ok(())
}

fn thread_preview(thread: &Thread) -> String {
    let Some(values) = &thread.values else {
        return "(empty)".to_string();
    };
    let messages = get_messages(values);
    if messages.is_empty() {
        return "(empty)".to_string();
    }
    for msg in &messages {
        if msg.role == "user" || msg.role == "human" {
            return format!("\"{}\"", truncate_preview(&msg.content));
        }
    }
    format!("\"{}\"", truncate_preview(&messages[0].content))
}

fn truncate_preview(text: &str) -> String {
    if text.len() > 50 {
        format!("{}...", &text[..47])
    } else {
        text.to_string()
    }
}
