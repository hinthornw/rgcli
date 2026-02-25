use tui_textarea::TextArea;

use super::{ChatMessage, ChatState, PLACEHOLDER, TIPS};

pub(super) fn handle_attach(app: &mut ChatState, path: &str) {
    let expanded = if path.starts_with('~') {
        if let Some(home) = dirs_home() {
            path.replacen('~', &home, 1)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    match std::fs::read(&expanded) {
        Ok(data) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            let mime = guess_mime(&expanded);
            let filename = std::path::Path::new(&expanded)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string());
            app.messages.push(ChatMessage::System(format!(
                "📎 Attached: {filename} ({mime}, {} bytes)",
                data.len()
            )));
            app.pending_attachments.push(crate::api::types::Attachment {
                filename,
                mime_type: mime,
                base64_data: b64,
            });
        }
        Err(e) => {
            app.messages.push(ChatMessage::Error(format!(
                "Failed to read {expanded}: {e}"
            )));
        }
    }
}

fn guess_mime(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".pdf") {
        "application/pdf".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

pub(super) fn list_assistants(app: &mut ChatState) {
    if app.available_assistants.is_empty() {
        app.messages
            .push(ChatMessage::System("No assistants found.".to_string()));
    } else {
        app.messages
            .push(ChatMessage::System("Available assistants:".to_string()));
        for (id, name) in &app.available_assistants {
            let current = if *id == app.assistant_id {
                " (current)"
            } else {
                ""
            };
            app.messages
                .push(ChatMessage::System(format!("  {name} [{id}]{current}")));
        }
        app.messages.push(ChatMessage::System(
            "Use /assistant <id> to switch.".to_string(),
        ));
    }
    app.auto_scroll = true;
}

pub(super) fn export_conversation(app: &mut ChatState) {
    let mut md = String::new();
    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                md.push_str(&format!("**You:** {text}\n\n"));
            }
            ChatMessage::Assistant(text) => {
                md.push_str(&format!("**Assistant:** {text}\n\n"));
            }
            ChatMessage::ToolUse(name, args) => {
                md.push_str(&format!("> Tool: `{name}({args})`\n\n"));
            }
            ChatMessage::ToolResult(name, content) => {
                md.push_str(&format!("> Result ({name}): {content}\n\n"));
            }
            ChatMessage::System(text) => {
                md.push_str(&format!("*{text}*\n\n"));
            }
            ChatMessage::Error(text) => {
                md.push_str(&format!("**Error:** {text}\n\n"));
            }
        }
    }

    let filename = format!(
        "ailsd-export-{}.md",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    match std::fs::write(&filename, &md) {
        Ok(()) => {
            app.messages.push(ChatMessage::System(format!(
                "Exported conversation to {filename}"
            )));
        }
        Err(e) => {
            app.messages
                .push(ChatMessage::Error(format!("Export failed: {e}")));
        }
    }
    app.auto_scroll = true;
}

pub(super) fn random_tip() -> &'static str {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize;
    TIPS[(secs.wrapping_mul(2654435761)) % TIPS.len()]
}

pub(super) fn tip_for_tick(tick: usize) -> &'static str {
    let idx = tick / 100;
    TIPS[(idx.wrapping_mul(2654435761)) % TIPS.len()]
}

pub(super) fn reset_textarea(app: &mut ChatState) {
    app.textarea = TextArea::default();
    app.textarea.set_placeholder_text(PLACEHOLDER);
    app.textarea
        .set_cursor_line_style(ratatui::style::Style::default());
}

pub(super) fn show_help(app: &mut ChatState) {
    app.messages
        .push(ChatMessage::System("Commands:".to_string()));

    let commands = [
        ("/new", "Start a new thread"),
        ("/context <name>", "Switch context"),
        ("/assistant <id>", "Switch assistant"),
        ("/assistant", "List assistants"),
        ("/mode <mode>", "Switch stream mode"),
        ("/attach <path>", "Attach a file"),
        ("/bench", "Run load test"),
        ("/doctor", "Diagnose connectivity"),
        ("/configure", "Update connection settings"),
        ("/export", "Export conversation to markdown"),
        ("/clear", "Clear chat display"),
        ("/devtools", "Toggle developer toolbar (F12)"),
        ("/help", "Show available commands"),
        ("/exit", "Exit the chat"),
    ];
    for (name, desc) in commands {
        app.messages
            .push(ChatMessage::System(format!("  {:<16} {}", name, desc)));
    }

    app.messages.push(ChatMessage::System(String::new()));
    app.messages.push(ChatMessage::System("Keys:".to_string()));
    app.messages.push(ChatMessage::System(
        "  Enter          Send message".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  Alt+Enter      Insert newline".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  Esc Esc        Cancel active run".to_string(),
    ));
    app.messages
        .push(ChatMessage::System("  Ctrl+C Ctrl+C  Quit".to_string()));
    app.messages.push(ChatMessage::System(
        "  Ctrl+R         Toggle search mode".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  PageUp/Down    Scroll chat history".to_string(),
    ));
    app.messages.push(ChatMessage::System(
        "  F12            Toggle devtools".to_string(),
    ));
    app.auto_scroll = true;
}
