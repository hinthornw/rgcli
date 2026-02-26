//! Internal debug log for troubleshooting (like browser console).
//! Writes to ~/.ailsd/debug.log. Viewable in devtools or via `ailsd logs --debug`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::OnceLock;
use std::time::SystemTime;

use crate::config;

static LOG_ENABLED: OnceLock<bool> = OnceLock::new();

fn is_enabled() -> bool {
    *LOG_ENABLED.get_or_init(|| {
        // Always enabled — it's just a file, only shown when you look for it
        true
    })
}

fn log_path() -> Option<std::path::PathBuf> {
    config::config_dir().ok().map(|d| d.join("debug.log"))
}

/// Write a debug log entry. Silent on failure.
pub fn log(component: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let Some(path) = log_path() else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let millis = d.subsec_millis();
            // Simple ISO-ish timestamp
            let dt = chrono::DateTime::from_timestamp(secs as i64, 0)
                .unwrap_or_default();
            format!("{}.{millis:03}", dt.format("%H:%M:%S"))
        })
        .unwrap_or_default();

    let line = format!("[{timestamp}] [{component}] {message}\n");
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Truncate the log file at the start of a session.
pub fn reset() {
    if let Some(path) = log_path() {
        let _ = fs::write(&path, "");
    }
}

/// Read the last N lines of the debug log.
pub fn tail(n: usize) -> Vec<String> {
    let Some(path) = log_path() else {
        return Vec::new();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .rev()
        .take(n)
        .map(String::from)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
