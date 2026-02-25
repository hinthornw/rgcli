use crossterm::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

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

// --- ratatui logo (for in-TUI display) ---

pub fn logo_lines(
    version: &str,
    endpoint: &str,
    config_path: &str,
    context_info: &str,
    deploy_info: Option<&str>,
) -> Vec<Line<'static>> {
    super::mascot::logo_with_parrot(version, endpoint, config_path, context_info, deploy_info)
}
