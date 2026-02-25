use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::Frame;

pub struct Column {
    pub name: String,
    pub width_pct: u16,
}

pub struct ResourceTable {
    pub title: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    pub state: TableState,
    pub filter_mode: bool,
    pub filter_query: String,
    pub filtered_indices: Vec<usize>,
    pub loading: bool,
    pub error: Option<String>,
    pub empty_message: String,
}

impl ResourceTable {
    pub fn new(title: &str, columns: Vec<Column>) -> Self {
        Self {
            title: title.to_string(),
            columns,
            rows: Vec::new(),
            state: TableState::default(),
            filter_mode: false,
            filter_query: String::new(),
            filtered_indices: Vec::new(),
            loading: true,
            error: None,
            empty_message: "No items found.".to_string(),
        }
    }

    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        self.loading = false;
        self.error = None;
        self.apply_filter();
        if !self.filtered_indices.is_empty() {
            self.state.select(Some(0));
        } else {
            self.state.select(None);
        }
    }

    pub fn set_error(&mut self, err: String) {
        self.loading = false;
        self.error = Some(err);
    }

    #[allow(dead_code)]
    pub fn selected_row(&self) -> Option<&Vec<String>> {
        let idx = self.state.selected()?;
        let real_idx = self.filtered_indices.get(idx)?;
        self.rows.get(*real_idx)
    }

    /// Returns true if the key was consumed
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.filter_mode {
            match key.code {
                KeyCode::Esc => {
                    self.filter_mode = false;
                    return true;
                }
                KeyCode::Enter => {
                    self.filter_mode = false;
                    return true;
                }
                KeyCode::Char(c) => {
                    self.filter_query.push(c);
                    self.apply_filter();
                    return true;
                }
                KeyCode::Backspace => {
                    self.filter_query.pop();
                    self.apply_filter();
                    return true;
                }
                _ => return false,
            }
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.next();
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.prev();
                true
            }
            KeyCode::Char('/') => {
                self.filter_mode = true;
                self.filter_query.clear();
                true
            }
            KeyCode::Char('G') => {
                self.select_last();
                true
            }
            KeyCode::Char('g') => {
                self.select_first();
                true
            }
            _ => false,
        }
    }

    fn next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.filtered_indices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn prev(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.filtered_indices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn select_first(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.state.select(Some(0));
        }
    }

    fn select_last(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.state.select(Some(self.filtered_indices.len() - 1));
        }
    }

    fn apply_filter(&mut self) {
        if self.filter_query.is_empty() {
            self.filtered_indices = (0..self.rows.len()).collect();
        } else {
            let q = self.filter_query.to_lowercase();
            self.filtered_indices = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.iter().any(|cell| cell.to_lowercase().contains(&q)))
                .map(|(i, _)| i)
                .collect();
        }
        // Keep selection in bounds
        if let Some(sel) = self.state.selected() {
            if sel >= self.filtered_indices.len() {
                self.state.select(if self.filtered_indices.is_empty() {
                    None
                } else {
                    Some(self.filtered_indices.len() - 1)
                });
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if self.loading {
            let msg = Paragraph::new("Loading...")
                .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
                .block(Block::default().borders(Borders::ALL).title(self.title.as_str()));
            frame.render_widget(msg, area);
            return;
        }

        if let Some(err) = &self.error {
            let msg = Paragraph::new(format!("Error: {err}"))
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title(self.title.as_str()));
            frame.render_widget(msg, area);
            return;
        }

        // Reserve space for filter bar if active
        let (table_area, filter_area) = if self.filter_mode {
            let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        if self.filtered_indices.is_empty() {
            let msg = Paragraph::new(self.empty_message.as_str())
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(self.title.as_str()));
            frame.render_widget(msg, table_area);
        } else {
            let header_cells: Vec<ratatui::text::Span> = self
                .columns
                .iter()
                .map(|c| Span::styled(c.name.as_str(), Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)))
                .collect();
            let header = Row::new(header_cells).height(1);

            let widths: Vec<Constraint> = self
                .columns
                .iter()
                .map(|c| Constraint::Percentage(c.width_pct))
                .collect();

            let rows: Vec<Row> = self
                .filtered_indices
                .iter()
                .map(|&idx| {
                    let cells: Vec<String> = self.rows[idx].clone();
                    let styled_cells: Vec<ratatui::text::Span> = cells
                        .into_iter()
                        .enumerate()
                        .map(|(col_idx, text)| {
                            // Color status columns
                            if col_idx < self.columns.len()
                                && self.columns[col_idx].name.to_lowercase() == "status"
                            {
                                let color = match text.as_str() {
                                    "success" => Color::Green,
                                    "error" => Color::Red,
                                    "running" | "pending" => Color::Yellow,
                                    "interrupted" => Color::Magenta,
                                    _ => Color::Reset,
                                };
                                Span::styled(text, Style::default().fg(color))
                            } else {
                                Span::raw(text)
                            }
                        })
                        .collect();
                    Row::new(styled_cells)
                })
                .collect();

            let table = Table::new(rows, &widths)
                .header(header)
                .block(Block::default().borders(Borders::ALL).title(self.title.as_str()))
                .row_highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .fg(Color::Yellow),
                );

            frame.render_stateful_widget(table, table_area, &mut self.state);
        }

        // Render filter bar
        if let Some(f_area) = filter_area {
            let filter_text = format!("/{}", self.filter_query);
            let filter_line = Line::from(vec![
                Span::styled("Filter: ", Style::default().fg(Color::Yellow)),
                Span::raw(filter_text),
            ]);
            let bar = Paragraph::new(filter_line)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White));
            frame.render_widget(bar, f_area);
        }
    }
}
