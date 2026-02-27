use std::fmt;

/// Errors from sandbox operations.
#[derive(Debug)]
pub enum SandboxError {
    /// Authentication failure (invalid/missing API key).
    Auth(String),
    /// Resource not found.
    NotFound { resource_type: String, name: String },
    /// Connection failure.
    Connection(String),
    /// Operation timed out waiting for resource readiness.
    Timeout { resource_type: String },
    /// Org quota exceeded.
    Quota { quota_type: String, message: String },
    /// Invalid input.
    Validation {
        message: String,
        details: Vec<String>,
    },
    /// Sandbox creation failed (image pull, crash loop, etc.).
    Creation { error_type: String, message: String },
    /// Runtime operation failed (command exec, file I/O).
    Operation { operation: String, message: String },
    /// Sandbox has no dataplane URL configured.
    DataplaneNotConfigured,
    /// Command execution timed out.
    CommandTimeout(String),
    /// Server is reloading (hot-reload), reconnect to resume.
    ServerReload(String),
    /// Generic HTTP error.
    Http { status: u16, body: String },
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(msg) => write!(f, "authentication error: {msg}"),
            Self::NotFound {
                resource_type,
                name,
            } => write!(f, "{resource_type} not found: {name}"),
            Self::Connection(msg) => write!(f, "connection error: {msg}"),
            Self::Timeout { resource_type } => {
                write!(f, "timeout waiting for {resource_type}")
            }
            Self::Quota {
                quota_type,
                message,
            } => write!(f, "quota exceeded ({quota_type}): {message}"),
            Self::Validation { message, .. } => write!(f, "validation error: {message}"),
            Self::Creation {
                error_type,
                message,
            } => write!(f, "sandbox creation failed ({error_type}): {message}"),
            Self::Operation { operation, message } => {
                write!(f, "{operation} failed: {message}")
            }
            Self::CommandTimeout(msg) => write!(f, "command timed out: {msg}"),
            Self::ServerReload(msg) => write!(f, "server reloading: {msg}"),
            Self::DataplaneNotConfigured => write!(f, "sandbox dataplane URL not configured"),
            Self::Http { status, body } => write!(f, "HTTP {status}: {body}"),
        }
    }
}

impl std::error::Error for SandboxError {}

impl From<reqwest::Error> for SandboxError {
    fn from(e: reqwest::Error) -> Self {
        Self::Connection(e.to_string())
    }
}

/// Parse an HTTP error response into a typed SandboxError.
pub fn parse_http_error(status: u16, body: &str) -> SandboxError {
    // Try to parse as JSON for structured errors
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let message = json
            .get("detail")
            .or_else(|| json.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or(body)
            .to_string();
        let error_type = json
            .get("error_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        return match status {
            401 | 403 => SandboxError::Auth(message),
            404 => SandboxError::NotFound {
                resource_type: "resource".to_string(),
                name: message,
            },
            409 if message.contains("already exists") => SandboxError::Validation {
                message,
                details: vec![],
            },
            422 => SandboxError::Validation {
                message,
                details: json
                    .get("detail")
                    .and_then(|d| d.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.get("msg").and_then(|m| m.as_str()))
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            429 => SandboxError::Quota {
                quota_type: extract_quota_type(&message).to_string(),
                message,
            },
            500 if !error_type.is_empty() => SandboxError::Creation {
                error_type,
                message,
            },
            _ => SandboxError::Http {
                status,
                body: message,
            },
        };
    }

    SandboxError::Http {
        status,
        body: body.to_string(),
    }
}

fn extract_quota_type(message: &str) -> &str {
    let lower = message.to_lowercase();
    if lower.contains("sandbox") && lower.contains("count") {
        "sandbox_count"
    } else if lower.contains("cpu") {
        "cpu"
    } else if lower.contains("memory") {
        "memory"
    } else if lower.contains("volume") && lower.contains("count") {
        "volume_count"
    } else if lower.contains("storage") {
        "storage"
    } else {
        "unknown"
    }
}
