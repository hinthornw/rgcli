//! WebSocket-based command execution for streaming stdout/stderr.

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

use crate::error::SandboxError;

/// Convert an HTTP(S) dataplane URL to a WebSocket URL for /execute/ws.
pub(crate) fn build_ws_url(dataplane_url: &str) -> String {
    let ws_url = dataplane_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    format!("{}/execute/ws", ws_url.trim_end_matches('/'))
}

/// A raw WebSocket message from the server.
#[derive(Debug, Clone)]
pub(crate) enum WsEvent {
    Started {
        command_id: String,
        pid: Option<u32>,
    },
    Stdout {
        data: String,
        offset: usize,
    },
    Stderr {
        data: String,
        offset: usize,
    },
    Exit {
        exit_code: i32,
    },
    Error {
        error_type: String,
        error: String,
    },
}

fn parse_ws_message(text: &str) -> Option<WsEvent> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let msg_type = v.get("type")?.as_str()?;
    match msg_type {
        "started" => Some(WsEvent::Started {
            command_id: v
                .get("command_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            pid: v.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32),
        }),
        "stdout" => Some(WsEvent::Stdout {
            data: v.get("data")?.as_str()?.to_string(),
            offset: v.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        }),
        "stderr" => Some(WsEvent::Stderr {
            data: v.get("data")?.as_str()?.to_string(),
            offset: v.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        }),
        "exit" => Some(WsEvent::Exit {
            exit_code: v.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
        }),
        "error" => Some(WsEvent::Error {
            error_type: v
                .get("error_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string(),
            error: v
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string(),
        }),
        _ => None,
    }
}

/// Convert a WsEvent::Error into the appropriate SandboxError.
pub(crate) fn error_from_ws_msg(
    error_type: &str,
    error: &str,
    command_id: Option<&str>,
) -> SandboxError {
    match error_type {
        "CommandTimeout" => SandboxError::CommandTimeout(error.to_string()),
        "CommandNotFound" => SandboxError::Operation {
            operation: if command_id.is_some() {
                "reconnect".to_string()
            } else {
                "command".to_string()
            },
            message: if let Some(id) = command_id {
                format!("Command not found: {id}")
            } else {
                error.to_string()
            },
        },
        "SessionExpired" => SandboxError::Operation {
            operation: if command_id.is_some() {
                "reconnect".to_string()
            } else {
                "command".to_string()
            },
            message: if let Some(id) = command_id {
                format!("Session expired: {id}")
            } else {
                error.to_string()
            },
        },
        _ => SandboxError::Operation {
            operation: if command_id.is_some() {
                "reconnect".to_string()
            } else {
                "command".to_string()
            },
            message: error.to_string(),
        },
    }
}

/// A live WebSocket connection to a sandbox.
///
/// Wraps the split sink/stream for sending control messages and receiving events.
pub(crate) struct WsConnection {
    sink: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    stream: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
}

impl WsConnection {
    /// Connect to the sandbox WebSocket endpoint.
    pub async fn connect(dataplane_url: &str, api_key: &str) -> Result<Self, SandboxError> {
        let ws_url = build_ws_url(dataplane_url);
        let mut request = ws_url
            .into_client_request()
            .map_err(|e| SandboxError::Connection(e.to_string()))?;

        if let Ok(val) = HeaderValue::from_str(api_key) {
            request.headers_mut().insert("X-Api-Key", val);
        }

        let (ws_stream, _response) =
            tokio_tungstenite::connect_async(request)
                .await
                .map_err(|e| {
                    let msg = e.to_string();
                    if msg.contains("404") {
                        SandboxError::Connection(
                        "Server does not support WebSocket execution (/execute/ws returned 404). \
                         Use run() for HTTP-based execution."
                            .to_string(),
                    )
                    } else {
                        SandboxError::Connection(msg)
                    }
                })?;

        let (sink, stream) = ws_stream.split();
        Ok(Self { sink, stream })
    }

    /// Send a JSON message to the server.
    pub async fn send_json(&mut self, value: &serde_json::Value) -> Result<(), SandboxError> {
        let text = serde_json::to_string(value).map_err(|e| SandboxError::Operation {
            operation: "ws_send".to_string(),
            message: e.to_string(),
        })?;
        self.sink
            .send(Message::Text(text))
            .await
            .map_err(|e| SandboxError::Connection(e.to_string()))
    }

    /// Receive the next event from the server.
    pub async fn recv(&mut self) -> Result<Option<WsEvent>, SandboxError> {
        loop {
            match self.stream.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Some(event) = parse_ws_message(&text) {
                        return Ok(Some(event));
                    }
                    // Skip unrecognized messages
                }
                Some(Ok(Message::Close(frame))) => {
                    if let Some(ref f) = frame {
                        if f.code == tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Away {
                            return Err(SandboxError::ServerReload(
                                "Server is reloading, reconnect to resume".to_string(),
                            ));
                        }
                    }
                    return Ok(None);
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    return Err(SandboxError::Connection(format!("WebSocket error: {e}")));
                }
                None => return Ok(None),
            }
        }
    }

    /// Send a kill signal for the running command.
    pub async fn send_kill(&mut self) -> Result<(), SandboxError> {
        self.send_json(&serde_json::json!({"type": "kill"})).await
    }

    /// Send stdin data to the running command.
    pub async fn send_input(&mut self, data: &str) -> Result<(), SandboxError> {
        self.send_json(&serde_json::json!({"type": "input", "data": data}))
            .await
    }

    /// Send an execute request.
    pub async fn send_execute(
        &mut self,
        command: &str,
        timeout: u64,
        shell: &str,
        env: &std::collections::HashMap<String, String>,
        cwd: Option<&str>,
    ) -> Result<(), SandboxError> {
        let mut payload = serde_json::json!({
            "type": "execute",
            "command": command,
            "timeout": timeout,
            "shell": shell,
        });
        if !env.is_empty() {
            payload["env"] = serde_json::to_value(env).unwrap_or_default();
        }
        if let Some(c) = cwd {
            payload["cwd"] = serde_json::Value::String(c.to_string());
        }
        self.send_json(&payload).await
    }

    /// Send a reconnect request.
    pub async fn send_reconnect(
        &mut self,
        command_id: &str,
        stdout_offset: usize,
        stderr_offset: usize,
    ) -> Result<(), SandboxError> {
        self.send_json(&serde_json::json!({
            "type": "reconnect",
            "command_id": command_id,
            "stdout_offset": stdout_offset,
            "stderr_offset": stderr_offset,
        }))
        .await
    }
}
