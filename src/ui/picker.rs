use std::io::{stdout, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;

use crate::api::{get_messages, Thread};
use crate::ui::styles::system_text;

pub fn pick_thread(threads: &[Thread]) -> Result<Option<Thread>> {
    if threads.is_empty() {
        return Ok(None);
    }

    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, Hide)?;

    let result = pick_thread_inner(threads);

    execute!(stdout(), LeaveAlternateScreen, Show)?;
    terminal::disable_raw_mode()?;

    result
}

fn pick_thread_inner(threads: &[Thread]) -> Result<Option<Thread>> {
    let mut cursor = 0usize;
    render_picker(threads, cursor)?;

    loop {
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c')
                    if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    return Ok(None);
                }
                KeyCode::Up | KeyCode::BackTab => {
                    cursor = cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Tab => {
                    if cursor + 1 < threads.len() {
                        cursor += 1;
                    }
                }
                KeyCode::Enter => return Ok(Some(threads[cursor].clone())),
                _ => {}
            }
            render_picker(threads, cursor)?;
        }
    }
}

fn render_picker(threads: &[Thread], cursor: usize) -> Result<()> {
    let mut out = stdout();
    execute!(out, MoveTo(0, 0), Clear(ClearType::All))?;

    writeln!(out, "Select a thread to resume:\r\n\r")?;

    for (i, thread) in threads.iter().enumerate() {
        let preview = thread_preview(thread);
        let thread_id: String = thread.thread_id.chars().take(8).collect();
        let line = format!("{} - {}", thread_id, preview);
        if i == cursor {
            writeln!(out, "  {}\r", system_text(&format!("> {}", line)))?;
        } else {
            writeln!(out, "    {}\r", line)?;
        }
    }

    writeln!(
        out,
        "\r\n  {}\r",
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
    let text = text.lines().next().unwrap_or(text);
    if text.chars().count() > 50 {
        let truncated: String = text.chars().take(47).collect();
        format!("{truncated}...")
    } else {
        text.to_string()
    }
}
