use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::screen::Screen;

pub struct CommandBar {
    pub active: bool,
    pub input: String,
    matched: Option<Screen>,
}

impl CommandBar {
    pub fn new() -> Self {
        Self {
            active: false,
            input: String::new(),
            matched: None,
        }
    }

    pub fn open(&mut self) {
        self.active = true;
        self.input.clear();
        self.matched = None;
    }

    /// Returns Some(Screen) if user pressed Enter on a valid match, or None if Esc/still typing
    pub fn handle_key(&mut self, key: KeyEvent) -> CommandBarResult {
        match key.code {
            KeyCode::Esc => {
                self.active = false;
                self.input.clear();
                self.matched = None;
                CommandBarResult::Cancelled
            }
            KeyCode::Enter => {
                self.active = false;
                let result = self.matched.take();
                self.input.clear();
                if let Some(screen) = result {
                    CommandBarResult::Navigate(screen)
                } else {
                    CommandBarResult::Cancelled
                }
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                self.matched = Screen::from_input(&self.input);
                CommandBarResult::Typing
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.matched = if self.input.is_empty() {
                    None
                } else {
                    Screen::from_input(&self.input)
                };
                CommandBarResult::Typing
            }
            _ => CommandBarResult::Typing,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.active {
            return;
        }
        let hint = self
            .matched
            .as_ref()
            .map(|s| s.label().to_string())
            .unwrap_or_default();

        let line = Line::from(vec![
            Span::styled(
                ":",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&self.input),
            if !hint.is_empty() && hint.to_lowercase() != self.input.to_lowercase() {
                Span::styled(
                    format!("  -> {hint}"),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )
            } else {
                Span::raw("")
            },
        ]);

        let bar = Paragraph::new(line)
            .style(Style::default().bg(Color::Rgb(30, 30, 30)).fg(Color::White));
        frame.render_widget(bar, area);
    }
}

pub enum CommandBarResult {
    Navigate(Screen),
    Cancelled,
    Typing,
}
