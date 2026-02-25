use crossterm::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

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

fn logo_accent_style() -> Style {
    Style::new().fg(Color::Yellow)
}

fn logo_body_style() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn logo_title_style() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

pub fn logo_lines(
    version: &str,
    endpoint: &str,
    config_path: &str,
    context_info: &str,
) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![Span::styled("   ▄█▀▀█▄", logo_accent_style())]),
        Line::from(vec![
            Span::styled("  ▄██", logo_accent_style()),
            Span::styled("▄░▄", logo_body_style()),
            Span::styled("█", logo_accent_style()),
            Span::raw("    "),
            Span::styled("ailsd", logo_title_style()),
            Span::raw(" "),
            Span::styled(version.to_string(), system_style_r()),
        ]),
        Line::from(vec![
            Span::styled("  ███████", logo_body_style()),
            Span::raw("    "),
            Span::styled(endpoint.to_string(), system_style_r()),
        ]),
        Line::from(vec![
            Span::styled("  ▀█░░░█", logo_body_style()),
            Span::raw("     "),
            Span::styled(context_info.to_string(), system_style_r()),
        ]),
        Line::from(vec![
            Span::styled("   █▀ █▀", logo_body_style()),
            Span::raw("     "),
            Span::styled(config_path.to_string(), system_style_r()),
        ]),
        Line::default(),
    ]
}
