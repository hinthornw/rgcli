use crossterm::style::Stylize;
use ratatui::style::{Color, Modifier, Style};

// --- ratatui styles (for TUI rendering) ---

pub fn user_style() -> Style {
    Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
}

pub fn assistant_style() -> Style {
    Style::new().fg(Color::Green)
}

pub fn system_style_r() -> Style {
    Style::new()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC)
}

pub fn error_style_r() -> Style {
    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn status_bar_style() -> Style {
    Style::new().fg(Color::White).bg(Color::DarkGray)
}

// --- crossterm styles (for non-TUI output: logo, pipe mode, etc.) ---

fn system_style(text: &str) -> String {
    format!("{}", text.dark_grey().italic())
}

fn error_style(text: &str) -> String {
    format!("{}", text.red().bold())
}

pub fn print_error(msg: &str) -> String {
    error_style(&format!("Error: {}", msg))
}

pub fn system_text(msg: &str) -> String {
    system_style(msg)
}

