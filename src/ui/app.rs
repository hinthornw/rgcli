use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::time;

use super::chat::{ChatExit, ChatState};
use super::mascot::ParrotState;
use super::screen::{Screen, ScreenAction, ScreenContext};
use super::screens;
use super::widgets::command_bar::{CommandBar, CommandBarResult};
use crate::api::Client;

pub struct TuiApp {
    screen: Screen,
    screen_stack: Vec<Screen>,
    command_bar: CommandBar,

    // Screen states
    chat: ChatState,
    threads: screens::ThreadsScreen,
    assistants: screens::AssistantsScreen,
    runs: screens::RunsScreen,
    store: screens::StoreScreen,
    crons: screens::CronsScreen,
    logs: screens::LogsScreen,

    // Shared
    client: Client,
    thread_id: String,
}

impl TuiApp {
    pub fn new(chat: ChatState, client: Client, thread_id: String) -> Self {
        Self {
            screen: Screen::Chat,
            screen_stack: Vec::new(),
            command_bar: CommandBar::new(),
            chat,
            threads: screens::ThreadsScreen::new(),
            assistants: screens::AssistantsScreen::new(),
            runs: screens::RunsScreen::new(),
            store: screens::StoreScreen::new(),
            crons: screens::CronsScreen::new(),
            logs: screens::LogsScreen::new(),
            client,
            thread_id,
        }
    }

    pub async fn run(mut self) -> Result<ChatExit> {
        terminal::enable_raw_mode()?;
        execute!(
            stdout(),
            EnterAlternateScreen,
            crossterm::event::EnableBracketedPaste,
        )?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let result = self.event_loop(&mut terminal).await;

        execute!(
            stdout(),
            crossterm::event::DisableBracketedPaste,
            LeaveAlternateScreen,
        )?;
        terminal::disable_raw_mode()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<ChatExit> {
        let mut term_events = EventStream::new();
        let mut interval = time::interval(Duration::from_millis(80));

        terminal.draw(|f| self.draw(f))?;

        loop {
            // Poll async results from screens
            self.poll_screens();

            tokio::select! {
                biased;

                // Chat stream events (only when on chat screen and streaming)
                event = async {
                    if self.screen == Screen::Chat && self.chat.has_pending_stream() {
                        self.chat.handle_stream_event(&self.client, &self.thread_id).await;
                        true
                    } else {
                        std::future::pending::<bool>().await
                    }
                } => {
                    if event {
                        terminal.draw(|f| self.draw(f))?;
                    }
                }

                Some(Ok(event)) = term_events.next() => {
                    if let Some(exit) = self.handle_event(event) {
                        return Ok(exit);
                    }
                    terminal.draw(|f| self.draw(f))?;
                }

                _ = interval.tick() => {
                    self.chat.handle_tick();
                    self.chat.handle_update_notice();
                    terminal.draw(|f| self.draw(f))?;
                }
            }
        }
    }

    fn poll_screens(&mut self) {
        self.chat.poll_history();
        self.threads.poll();
        self.assistants.poll();
        self.runs.poll();
        self.store.poll();
        self.crons.poll();
        self.logs.poll();
    }

    fn handle_event(&mut self, event: Event) -> Option<ChatExit> {
        // Paste events go directly to chat screen
        if let Event::Paste(ref text) = event {
            if self.screen == Screen::Chat {
                let action = self.chat.handle_paste(text);
                return self.handle_action(action);
            }
            return None;
        }

        let Event::Key(key) = event else {
            return None;
        };

        // Command bar gets priority
        if self.command_bar.active {
            match self.command_bar.handle_key(key) {
                CommandBarResult::Navigate(screen) => {
                    self.navigate_to(screen);
                }
                CommandBarResult::Cancelled | CommandBarResult::Typing => {}
            }
            return None;
        }

        // Ctrl+B opens command bar from ANY screen (including chat)
        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.command_bar.open();
            return None;
        }

        // `:` opens command bar from non-chat screens
        if self.screen != Screen::Chat && key.code == KeyCode::Char(':') {
            self.command_bar.open();
            return None;
        }

        // Route to active screen
        let action = match &self.screen {
            Screen::Chat => self
                .chat
                .handle_key_event(event, &self.client, &self.thread_id),
            Screen::Threads => self.threads.handle_key(key, &self.client),
            Screen::Assistants => self.assistants.handle_key(key, &self.client),
            Screen::Runs => self.runs.handle_key(key, &self.client),
            Screen::Store => self.store.handle_key(key, &self.client),
            Screen::Crons => self.crons.handle_key(key, &self.client),
            Screen::Logs => self.logs.handle_key(key, &self.client),
        };

        self.handle_action(action)
    }

    fn handle_action(&mut self, action: ScreenAction) -> Option<ChatExit> {
        match action {
            ScreenAction::None => None,
            ScreenAction::Navigate(screen) => {
                self.navigate_to(screen);
                None
            }
            ScreenAction::NavigateWithContext(screen, ctx) => {
                self.apply_context(&ctx);
                self.navigate_to(screen);
                None
            }
            ScreenAction::Back => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                    self.enter_screen();
                }
                None
            }
            ScreenAction::Quit => Some(ChatExit::Quit),
            ScreenAction::Refresh => {
                self.enter_screen();
                None
            }
            ScreenAction::ChatExit(exit) => Some(exit),
        }
    }

    fn apply_context(&mut self, ctx: &ScreenContext) {
        match ctx {
            ScreenContext::Thread(tid) => {
                self.thread_id = tid.clone();
                self.chat.load_thread_history(&self.client, tid);
            }
            ScreenContext::Assistant(id) => {
                self.chat.assistant_id = id.clone();
                self.chat
                    .messages
                    .push(super::chat::ChatMessage::System(format!(
                        "Switched to assistant: {id}"
                    )));
                self.chat.auto_scroll = true;
            }
        }
    }

    fn navigate_to(&mut self, screen: Screen) {
        if screen != self.screen {
            self.screen_stack.push(self.screen.clone());
            self.screen = screen;
            self.enter_screen();
        }
    }

    fn enter_screen(&mut self) {
        // Update parrot state for the new screen
        let parrot_state = match &self.screen {
            Screen::Chat => ParrotState::Idle,
            Screen::Threads => ParrotState::Threads,
            Screen::Assistants => ParrotState::Assistants,
            Screen::Runs => ParrotState::Runs,
            Screen::Store => ParrotState::Store,
            Screen::Crons => ParrotState::Crons,
            Screen::Logs => ParrotState::Logs,
        };
        self.chat.parrot_mut().set_state(parrot_state);

        match &self.screen {
            Screen::Chat => {} // Chat is always alive
            Screen::Threads => self.threads.on_enter(&self.client),
            Screen::Assistants => self.assistants.on_enter(&self.client),
            Screen::Runs => self.runs.on_enter(&self.client),
            Screen::Store => self.store.on_enter(&self.client),
            Screen::Crons => self.crons.on_enter(&self.client),
            Screen::Logs => self.logs.on_enter(&self.client),
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        // Tab bar (1 line) + content area + optional command bar (1 line)
        let cmd_height = if self.command_bar.active { 1 } else { 0 };
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(cmd_height),
        ])
        .split(area);

        self.render_tab_bar(frame, chunks[0]);

        match &self.screen {
            Screen::Chat => self.chat.draw_in_area(frame, chunks[1]),
            Screen::Threads => self.threads.render(frame, chunks[1]),
            Screen::Assistants => self.assistants.render(frame, chunks[1]),
            Screen::Runs => self.runs.render(frame, chunks[1]),
            Screen::Store => self.store.render(frame, chunks[1]),
            Screen::Crons => self.crons.render(frame, chunks[1]),
            Screen::Logs => self.logs.render(frame, chunks[1]),
        }

        if self.command_bar.active {
            self.command_bar.render(frame, chunks[2]);
        }
    }

    fn render_tab_bar(&self, frame: &mut ratatui::Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::raw(" "));
        for screen in Screen::all() {
            let label = screen.label();
            if *screen == self.screen {
                spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
            }
            spans.push(Span::raw("  "));
        }
        // Right-align a Ctrl+B hint
        let hint = Span::styled("^B navigate ", Style::default().fg(Color::Rgb(80, 80, 80)));
        let tabs_width: usize = spans.iter().map(|s| s.content.len()).sum();
        let hint_width = hint.content.len();
        let padding = area.width as usize - tabs_width.min(area.width as usize) - hint_width;
        if padding > 0 {
            spans.push(Span::raw(" ".repeat(padding)));
            spans.push(hint);
        }
        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().bg(Color::Rgb(25, 25, 25)));
        frame.render_widget(bar, area);
    }
}
