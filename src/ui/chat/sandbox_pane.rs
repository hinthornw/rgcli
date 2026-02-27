//! Split terminal pane for sandbox shell within the chat screen.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;

use lsandbox::{InputSender, OutputChunk};

/// Maximum lines kept in the terminal output buffer.
const MAX_OUTPUT_LINES: usize = 1000;

/// State for the sandbox terminal pane.
pub(crate) struct SandboxTerminal {
    /// Name of the connected sandbox.
    pub sandbox_name: String,
    /// Output buffer (rendered as terminal content).
    pub output: Vec<String>,
    /// Current input line.
    pub input: String,
    /// Cursor position in input line.
    pub cursor: usize,
    /// Sender for stdin to the running shell.
    input_sender: Option<InputSender>,
    /// Receiver for output chunks from the shell.
    chunk_rx: Option<mpsc::Receiver<OutputChunk>>,
    /// Whether the shell is still running.
    pub alive: bool,
    /// Background task handle.
    _task: Option<tokio::task::JoinHandle<()>>,
}

impl SandboxTerminal {
    /// Create a new terminal pane connected to a sandbox.
    ///
    /// Spawns a bash shell via WebSocket streaming.
    pub async fn connect(sandbox_name: &str, api_key: &str) -> Result<Self, String> {
        let client =
            lsandbox::SandboxClient::new(api_key).map_err(|e| format!("Client error: {e}"))?;
        let sandbox = client
            .get_sandbox(sandbox_name)
            .await
            .map_err(|e| format!("Get sandbox: {e}"))?;
        Self::connect_from_sandbox(sandbox, sandbox_name).await
    }

    /// Create a new terminal pane connected to a server-issued sandbox session.
    pub async fn connect_session(session_id: &str) -> Result<Self, String> {
        let cfg = crate::config::load().map_err(|e| format!("Config error: {e}"))?;
        let api = crate::api::Client::new(&cfg).map_err(|e| format!("API client error: {e}"))?;
        let refreshed = api
            .refresh_sandbox_session(session_id)
            .await
            .map_err(|e| format!("Refresh session: {e}"))?;
        let session = api
            .get_sandbox_session(session_id)
            .await
            .map_err(|e| format!("Get session: {e}"))?;

        let token = if refreshed.token.is_empty() {
            session.token.clone()
        } else {
            refreshed.token
        };
        if token.is_empty() {
            return Err("Session token missing after refresh".to_string());
        }

        let cfg_key = if cfg.api_key.is_empty() {
            std::env::var("LANGSMITH_API_KEY").unwrap_or_default()
        } else {
            cfg.api_key.clone()
        };
        if cfg_key.is_empty() {
            return Err("No API key configured.".to_string());
        }

        let client = lsandbox::SandboxClient::new(&cfg_key)
            .map_err(|e| format!("Sandbox client error: {e}"))?;
        let label = format!("session-{session_id}");
        let sandbox = client.sandbox_from_dataplane(&label, &session.sandbox.http_base_url, &token);
        Self::connect_from_sandbox(sandbox, &label).await
    }

    async fn connect_from_sandbox(sandbox: lsandbox::Sandbox, label: &str) -> Result<Self, String> {
        let mut handle = sandbox
            .run_streaming("/bin/bash")
            .await
            .map_err(|e| format!("Connect: {e}"))?;

        let input_sender = handle.input_sender();

        // Move chunk receiving to a channel we can poll synchronously
        let (tx, rx) = mpsc::channel::<OutputChunk>(256);
        let task = tokio::spawn(async move {
            while let Some(chunk) = handle.recv().await {
                if tx.send(chunk).await.is_err() {
                    break;
                }
            }
            // handle.wait() consumed implicitly when handle drops
        });

        Ok(Self {
            sandbox_name: label.to_string(),
            output: vec![format!("Connected to sandbox '{label}'")],
            input: String::new(),
            cursor: 0,
            input_sender: Some(input_sender),
            chunk_rx: Some(rx),
            alive: true,
            _task: Some(task),
        })
    }

    /// Poll for new output chunks (non-blocking).
    pub fn poll(&mut self) {
        if let Some(rx) = &mut self.chunk_rx {
            // Drain all available chunks
            while let Ok(chunk) = rx.try_recv() {
                // Split data by newlines and append to output buffer
                let data = &chunk.data;
                if data.is_empty() {
                    continue;
                }

                // Handle the output: split on newlines
                let lines: Vec<&str> = data.split('\n').collect();
                for (i, line) in lines.iter().enumerate() {
                    if i == 0 {
                        // Append to last line if exists
                        if let Some(last) = self.output.last_mut() {
                            last.push_str(line);
                        } else {
                            self.output.push(line.to_string());
                        }
                    } else {
                        self.output.push(line.to_string());
                    }
                }

                // Trim buffer
                while self.output.len() > MAX_OUTPUT_LINES {
                    self.output.remove(0);
                }
            }
        }
    }

    /// Send the current input line to the shell.
    pub fn submit_input(&mut self) {
        if let Some(sender) = &self.input_sender {
            let line = format!("{}\n", self.input);
            let sender = sender.clone();
            tokio::spawn(async move {
                let _ = sender.send(&line).await;
            });
        }
        // Echo input to output
        self.output.push(format!("$ {}", self.input));
        self.input.clear();
        self.cursor = 0;
    }

    /// Handle a character input.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Handle backspace.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.input.remove(self.cursor);
        }
    }

    /// Handle delete.
    pub fn delete_char(&mut self) {
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
        }
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor += 1;
        }
    }

    /// Kill the shell.
    pub fn kill(&mut self) {
        if let Some(sender) = &self.input_sender {
            let sender = sender.clone();
            tokio::spawn(async move {
                let _ = sender.kill().await;
            });
        }
        self.alive = false;
        self.input_sender = None;
        self.chunk_rx = None;
    }

    /// Render the terminal pane.
    pub fn render(&self, frame: &mut ratatui::Frame, area: Rect, focused: bool) {
        let border_color = if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let title = format!(
            " Terminal: {} {} ",
            self.sandbox_name,
            if self.alive { "●" } else { "○" }
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 2 {
            return;
        }

        // Reserve bottom line for input
        let output_area = Rect {
            height: inner.height.saturating_sub(1),
            ..inner
        };
        let input_area = Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        };

        // Render output (show last N lines that fit)
        let visible_lines = output_area.height as usize;
        let start = self.output.len().saturating_sub(visible_lines);
        let lines: Vec<Line> = self.output[start..]
            .iter()
            .map(|s| Line::from(Span::styled(s.as_str(), Style::default().fg(Color::White))))
            .collect();

        let output_widget = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(output_widget, output_area);

        // Render input line
        let prompt = "$ ";
        let input_line = Line::from(vec![
            Span::styled(prompt, Style::default().fg(Color::Green)),
            Span::styled(&self.input, Style::default().fg(Color::White)),
        ]);
        let input_widget = Paragraph::new(input_line);
        frame.render_widget(input_widget, input_area);

        // Show cursor if focused
        if focused {
            frame.set_cursor_position((
                input_area.x + prompt.len() as u16 + self.cursor as u16,
                input_area.y,
            ));
        }
    }
}
