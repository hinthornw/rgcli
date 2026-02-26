use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::{CursorMove, Input, Key, TextArea};

use super::{
    Action, CTRL_C_TIMEOUT, ChatExit, ChatMessage, ChatState, CompletionItem, ESC_TIMEOUT,
    MAX_INPUT_LINES, PLACEHOLDER,
};

struct SlashCommand {
    name: &'static str,
    desc: &'static str,
}

const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/new",
        desc: "Start a new thread",
    },
    SlashCommand {
        name: "/context",
        desc: "Switch context (/context <name>)",
    },
    SlashCommand {
        name: "/assistant",
        desc: "Switch assistant (/assistant <id>)",
    },
    SlashCommand {
        name: "/bench",
        desc: "Run load test",
    },
    SlashCommand {
        name: "/doctor",
        desc: "Diagnose connectivity",
    },
    SlashCommand {
        name: "/mode",
        desc: "Switch stream mode (/mode <mode>)",
    },
    SlashCommand {
        name: "/attach",
        desc: "Attach a file (/attach <path>)",
    },
    SlashCommand {
        name: "/configure",
        desc: "Update connection settings",
    },
    SlashCommand {
        name: "/export",
        desc: "Export conversation to markdown",
    },
    SlashCommand {
        name: "/clear",
        desc: "Clear chat display",
    },
    SlashCommand {
        name: "/devtools",
        desc: "Toggle developer toolbar (F12)",
    },
    SlashCommand {
        name: "/console",
        desc: "Show debug log",
    },
    SlashCommand {
        name: "/help",
        desc: "Show available commands",
    },
    SlashCommand {
        name: "/exit",
        desc: "Exit the chat",
    },
];

pub(super) fn handle_terminal_event(app: &mut ChatState, event: Event) -> Action {
    let Event::Key(key) = event else {
        return Action::None;
    };

    if key.code != KeyCode::Char('c') || !key.modifiers.contains(KeyModifiers::CONTROL) {
        app.ctrl_c_at = None;
    }

    if let Some(start) = app.ctrl_c_at {
        if start.elapsed() > CTRL_C_TIMEOUT {
            app.ctrl_c_at = None;
        }
    }

    if let Some(start) = app.last_esc_at {
        if start.elapsed() > ESC_TIMEOUT {
            app.last_esc_at = None;
        }
    }

    // Handle search mode
    if app.search_mode {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.search_mode = false;
                app.search_query.clear();
                app.search_matches.clear();
                app.search_match_idx = 0;
                return Action::None;
            }
            KeyCode::Backspace => {
                app.search_query.pop();
                update_search_matches(app);
                return Action::None;
            }
            KeyCode::Up | KeyCode::Char('p')
                if key.code == KeyCode::Up
                    || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Navigate to previous match
                if !app.search_matches.is_empty() {
                    if app.search_match_idx > 0 {
                        app.search_match_idx -= 1;
                    } else {
                        app.search_match_idx = app.search_matches.len() - 1;
                    }
                    scroll_to_match(app);
                }
                return Action::None;
            }
            KeyCode::Down | KeyCode::Char('n')
                if key.code == KeyCode::Down
                    || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Navigate to next match
                if !app.search_matches.is_empty() {
                    app.search_match_idx = (app.search_match_idx + 1) % app.search_matches.len();
                    scroll_to_match(app);
                }
                return Action::None;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_query.push(c);
                update_search_matches(app);
                return Action::None;
            }
            _ => return Action::None,
        }
    }

    if app.show_complete && !app.completions.is_empty() {
        if let Some(action) = handle_completion_key(&key, app) {
            return action;
        }
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.ctrl_c_at.is_some() {
                return Action::Quit;
            }
            app.ctrl_c_at = Some(Instant::now());
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.search_mode = !app.search_mode;
            if !app.search_mode {
                app.search_query.clear();
                app.search_matches.clear();
            } else {
                update_search_matches(app);
            }
        }
        KeyCode::Esc => {
            if app.last_esc_at.is_some() && app.is_streaming() {
                app.last_esc_at = None;
                return Action::Cancel;
            }
            app.last_esc_at = Some(Instant::now());
            app.show_complete = false;
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
            if app.textarea.lines().len() < MAX_INPUT_LINES {
                app.textarea.insert_newline();
            }
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.textarea.lines().len() < MAX_INPUT_LINES {
                app.textarea.insert_newline();
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.textarea = TextArea::default();
            app.textarea.set_placeholder_text(PLACEHOLDER);
            app.textarea
                .set_cursor_line_style(ratatui::style::Style::default());
        }
        KeyCode::Enter => {
            let value = collect_input(&app.textarea);
            if value.is_empty() && app.interrupted {
                return Action::Send(String::new());
            }
            if value.is_empty() {
                return Action::None;
            }
            if value == "/quit" || value == "/exit" {
                return Action::Quit;
            }
            if value == "/configure" {
                return Action::Configure;
            }
            if value == "/help" {
                return Action::Help;
            }
            if let Some(name) = value.strip_prefix("/context ") {
                let name = name.trim();
                if !name.is_empty() {
                    return Action::SwitchContext(name.to_string());
                }
            }
            if value == "/context" {
                return Action::Help;
            }
            if value == "/new" {
                return Action::NewThread;
            }
            if value == "/doctor" {
                return Action::ExitFor(ChatExit::RunDoctor);
            }
            if value == "/bench" || value.starts_with("/bench ") {
                return Action::ExitFor(ChatExit::RunBench);
            }
            if value == "/clear" {
                return Action::Clear;
            }
            if value == "/devtools" {
                app.devtools = !app.devtools;
                super::helpers::reset_textarea(app);
                return Action::None;
            }
            if value == "/console" {
                let lines = crate::debug_log::tail(50);
                if lines.is_empty() {
                    app.messages.push(ChatMessage::System("No debug log entries.".to_string()));
                } else {
                    app.messages.push(ChatMessage::System("─── Debug Console ───".to_string()));
                    for line in lines {
                        app.messages.push(ChatMessage::System(line));
                    }
                    app.messages.push(ChatMessage::System("─────────────────────".to_string()));
                }
                app.auto_scroll = true;
                super::helpers::reset_textarea(app);
                return Action::None;
            }
            if let Some(path) = value.strip_prefix("/attach ") {
                let path = path.trim();
                if !path.is_empty() {
                    return Action::Attach(path.to_string());
                }
            }
            if value == "/export" {
                return Action::Export;
            }
            if value == "/assistant" {
                return Action::ListAssistants;
            }
            if let Some(id) = value.strip_prefix("/assistant ") {
                let id = id.trim();
                if !id.is_empty() {
                    return Action::SwitchAssistant(id.to_string());
                }
            }
            if value == "/mode" {
                return Action::Help;
            }
            if let Some(mode) = value.strip_prefix("/mode ") {
                let mode = mode.trim();
                if !mode.is_empty() {
                    let valid_modes = ["messages-tuple", "values", "updates", "events", "debug"];
                    if valid_modes.contains(&mode) {
                        return Action::Mode(mode.to_string());
                    } else {
                        app.messages.push(ChatMessage::Error(format!(
                            "Invalid mode: {mode}. Valid modes: {}",
                            valid_modes.join(", ")
                        )));
                        super::helpers::reset_textarea(app);
                        return Action::None;
                    }
                }
            }
            return Action::Send(value);
        }
        KeyCode::PageUp => {
            app.auto_scroll = false;
            app.scroll_offset = app.scroll_offset.saturating_add(10);
        }
        KeyCode::PageDown => {
            if app.scroll_offset > 0 {
                app.scroll_offset = app.scroll_offset.saturating_sub(10);
            } else {
                app.auto_scroll = true;
            }
        }
        KeyCode::F(12) => {
            app.devtools = !app.devtools;
        }
        KeyCode::Tab => {
            update_completions(app);
            if app.show_complete {
                app.completion_idx = 0;
            }
        }
        _ => {
            if let Some(input) = to_textarea_input(key) {
                app.textarea.input(input);
            }
        }
    }

    update_completions(app);
    Action::None
}

fn handle_completion_key(key: &KeyEvent, app: &mut ChatState) -> Option<Action> {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            app.completion_idx = (app.completion_idx + 1) % app.completions.len();
            Some(Action::None)
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.completion_idx =
                (app.completion_idx + app.completions.len() - 1) % app.completions.len();
            Some(Action::None)
        }
        KeyCode::Enter => {
            let insert = app.completions[app.completion_idx].insert.clone();
            app.textarea = TextArea::from([insert]);
            app.textarea.move_cursor(CursorMove::End);
            app.show_complete = false;
            Some(Action::None)
        }
        KeyCode::Esc => {
            app.show_complete = false;
            Some(Action::None)
        }
        _ => None,
    }
}

pub(super) fn collect_input(textarea: &TextArea) -> String {
    textarea
        .lines()
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Scroll the chat view so the current search match is visible.
fn scroll_to_match(app: &mut ChatState) {
    if app.search_matches.is_empty() {
        return;
    }
    let match_msg_idx = app.search_matches[app.search_match_idx];
    // Estimate: each message is roughly 2 lines from the top.
    // We use message count as a rough proxy — the render will clamp.
    let total = app.messages.len();
    let from_bottom = total.saturating_sub(match_msg_idx);
    app.scroll_offset = (from_bottom as u16).saturating_mul(2);
    app.auto_scroll = false;
}

fn update_search_matches(app: &mut ChatState) {
    app.search_matches.clear();
    app.search_match_idx = 0;
    if app.search_query.is_empty() {
        return;
    }

    let query_lower = app.search_query.to_lowercase();
    for (idx, msg) in app.messages.iter().enumerate() {
        let text = match msg {
            ChatMessage::User(text)
            | ChatMessage::Assistant(text)
            | ChatMessage::System(text)
            | ChatMessage::Error(text) => text,
            ChatMessage::ToolUse(name, args) => {
                let combined = format!("{} {}", name, args);
                if combined.to_lowercase().contains(&query_lower) {
                    app.search_matches.push(idx);
                }
                continue;
            }
            ChatMessage::ToolResult(name, content) => {
                let combined = format!("{} {}", name, content);
                if combined.to_lowercase().contains(&query_lower) {
                    app.search_matches.push(idx);
                }
                continue;
            }
        };

        if text.to_lowercase().contains(&query_lower) {
            app.search_matches.push(idx);
        }
    }
}

fn update_completions(app: &mut ChatState) {
    let value: String = app
        .textarea
        .lines()
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if !value.starts_with('/') || value.contains('\n') {
        app.completions.clear();
        app.show_complete = false;
        return;
    }

    // Context autocompletion
    if let Some(prefix) = value.strip_prefix("/context ") {
        let prefix = prefix.to_lowercase();
        let matches: Vec<CompletionItem> = app
            .context_names
            .iter()
            .filter(|name| name.to_lowercase().starts_with(&prefix))
            .map(|name| CompletionItem {
                insert: format!("/context {name}"),
                label: name.clone(),
                desc: "switch context".to_string(),
            })
            .collect();
        app.show_complete = !matches.is_empty();
        app.completions = matches;
        return;
    }

    // Assistant autocompletion
    if let Some(prefix) = value.strip_prefix("/assistant ") {
        let prefix = prefix.to_lowercase();
        let matches: Vec<CompletionItem> = app
            .available_assistants
            .iter()
            .filter(|(_, name)| name.to_lowercase().starts_with(&prefix))
            .map(|(id, name)| CompletionItem {
                insert: format!("/assistant {id}"),
                label: name.clone(),
                desc: id.clone(),
            })
            .collect();
        app.show_complete = !matches.is_empty();
        app.completions = matches;
        return;
    }

    let matches: Vec<CompletionItem> = SLASH_COMMANDS
        .iter()
        .filter(|cmd| cmd.name.starts_with(&value))
        .map(|cmd| CompletionItem {
            insert: cmd.name.to_string(),
            label: cmd.name.to_string(),
            desc: cmd.desc.to_string(),
        })
        .collect();
    app.show_complete = !matches.is_empty();
    app.completions = matches;
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
