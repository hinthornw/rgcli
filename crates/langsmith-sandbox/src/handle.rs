//! CommandHandle — async streaming handle for WebSocket command execution.

use std::collections::HashMap;

use tokio::sync::mpsc;

use crate::error::SandboxError;
use crate::models::{ExecutionResult, OutputChunk};
use crate::ws::{WsConnection, WsEvent, error_from_ws_msg};

const MAX_AUTO_RECONNECTS: u32 = 5;
const BACKOFF_BASE_MS: u64 = 500;
const BACKOFF_MAX_MS: u64 = 8000;

/// Handle to a running command with streaming output.
///
/// Receives [`OutputChunk`]s via an async channel. Call [`recv()`](Self::recv)
/// to get chunks, or [`wait()`](Self::wait) to drain all output and get the
/// final [`ExecutionResult`].
///
/// Supports sending stdin and kill signals to the running command.
///
/// # Example
/// ```no_run
/// # async fn example(sandbox: &lsandbox::Sandbox) -> Result<(), lsandbox::SandboxError> {
/// let mut handle = sandbox.run_streaming("make build").await?;
/// while let Some(chunk) = handle.recv().await {
///     print!("{}", chunk.data);
/// }
/// let result = handle.result().expect("command completed");
/// println!("Exit code: {}", result.exit_code);
/// # Ok(())
/// # }
/// ```
pub struct CommandHandle {
    /// Receiver for output chunks.
    rx: mpsc::Receiver<OutputChunk>,
    /// Sender for control messages to the background task.
    control_tx: mpsc::Sender<ControlMsg>,
    /// The server-assigned command ID.
    command_id: Option<String>,
    /// The process ID on the sandbox.
    pid: Option<u32>,
    /// The final result, populated after the stream ends.
    final_result: Option<ExecutionResult>,
    /// Handle to the background task.
    task: Option<tokio::task::JoinHandle<Result<ExecutionResult, SandboxError>>>,
}

enum ControlMsg {
    Kill,
    Input(String),
}

impl CommandHandle {
    /// Receive the next output chunk, or `None` when the command exits.
    pub async fn recv(&mut self) -> Option<OutputChunk> {
        self.rx.recv().await
    }

    /// Drain all remaining output and return the final result.
    pub async fn wait(mut self) -> Result<ExecutionResult, SandboxError> {
        // Drain remaining chunks
        while self.rx.recv().await.is_some() {}
        // Await the background task
        if let Some(task) = self.task.take() {
            task.await.map_err(|e| SandboxError::Operation {
                operation: "command".to_string(),
                message: format!("task panicked: {e}"),
            })?
        } else {
            self.final_result.take().ok_or(SandboxError::Operation {
                operation: "command".to_string(),
                message: "stream ended without exit message".to_string(),
            })
        }
    }

    /// The server-assigned command ID (available after construction).
    pub fn command_id(&self) -> Option<&str> {
        self.command_id.as_deref()
    }

    /// The process ID on the sandbox (available after construction).
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Get the result if already available (non-blocking).
    pub fn result(&self) -> Option<&ExecutionResult> {
        self.final_result.as_ref()
    }

    /// Send a kill signal to the running command (SIGKILL).
    pub async fn kill(&self) -> Result<(), SandboxError> {
        self.control_tx
            .send(ControlMsg::Kill)
            .await
            .map_err(|_| SandboxError::Operation {
                operation: "kill".to_string(),
                message: "command already exited".to_string(),
            })
    }

    /// Write data to the command's stdin.
    pub async fn send_input(&self, data: &str) -> Result<(), SandboxError> {
        self.control_tx
            .send(ControlMsg::Input(data.to_string()))
            .await
            .map_err(|_| SandboxError::Operation {
                operation: "send_input".to_string(),
                message: "command already exited".to_string(),
            })
    }
}

/// Parameters needed to establish or re-establish a WS execution.
#[derive(Clone)]
pub(crate) struct WsExecParams {
    pub dataplane_url: String,
    pub api_key: String,
    pub command: String,
    pub timeout: u64,
    pub shell: String,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
}

/// Start a new streaming command execution over WebSocket.
///
/// Connects, sends the execute request, reads the "started" message,
/// then spawns a background task that forwards output chunks to the handle.
pub(crate) async fn start_streaming(params: WsExecParams) -> Result<CommandHandle, SandboxError> {
    let mut conn = WsConnection::connect(&params.dataplane_url, &params.api_key).await?;

    // Send execute request
    conn.send_execute(
        &params.command,
        params.timeout,
        &params.shell,
        &params.env,
        params.cwd.as_deref(),
    )
    .await?;

    // Read the "started" message eagerly
    let (command_id, pid) = match conn.recv().await? {
        Some(WsEvent::Started { command_id, pid }) => (command_id, pid),
        Some(WsEvent::Error { error_type, error }) => {
            return Err(error_from_ws_msg(&error_type, &error, None));
        }
        other => {
            return Err(SandboxError::Operation {
                operation: "command".to_string(),
                message: format!("expected 'started' message, got: {other:?}"),
            });
        }
    };

    let (chunk_tx, chunk_rx) = mpsc::channel::<OutputChunk>(256);
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);

    let cmd_id = command_id.clone();
    let task = tokio::spawn(run_stream_loop(conn, chunk_tx, control_rx, params, cmd_id));

    Ok(CommandHandle {
        rx: chunk_rx,
        control_tx,
        command_id: Some(command_id),
        pid,
        final_result: None,
        task: Some(task),
    })
}

/// Background task: read WS events, send chunks, handle reconnect.
async fn run_stream_loop(
    mut conn: WsConnection,
    chunk_tx: mpsc::Sender<OutputChunk>,
    mut control_rx: mpsc::Receiver<ControlMsg>,
    params: WsExecParams,
    command_id: String,
) -> Result<ExecutionResult, SandboxError> {
    let mut stdout_parts = Vec::new();
    let mut stderr_parts = Vec::new();
    let mut last_stdout_offset: usize = 0;
    let mut last_stderr_offset: usize = 0;
    let mut reconnect_attempts: u32 = 0;
    let mut killed = false;

    loop {
        tokio::select! {
            biased;

            // Control messages from the handle
            ctrl = control_rx.recv() => {
                match ctrl {
                    Some(ControlMsg::Kill) => {
                        killed = true;
                        let _ = conn.send_kill().await;
                    }
                    Some(ControlMsg::Input(data)) => {
                        let _ = conn.send_input(&data).await;
                    }
                    None => {} // Handle dropped, keep reading
                }
            }

            // WebSocket events
            event = conn.recv() => {
                match event {
                    Ok(Some(WsEvent::Stdout { data, offset })) => {
                        stdout_parts.push(data.clone());
                        last_stdout_offset = offset + data.len();
                        reconnect_attempts = 0;
                        let _ = chunk_tx.send(OutputChunk {
                            stream: "stdout".to_string(),
                            data,
                            offset,
                        }).await;
                    }
                    Ok(Some(WsEvent::Stderr { data, offset })) => {
                        stderr_parts.push(data.clone());
                        last_stderr_offset = offset + data.len();
                        reconnect_attempts = 0;
                        let _ = chunk_tx.send(OutputChunk {
                            stream: "stderr".to_string(),
                            data,
                            offset,
                        }).await;
                    }
                    Ok(Some(WsEvent::Exit { exit_code })) => {
                        return Ok(ExecutionResult {
                            stdout: stdout_parts.join(""),
                            stderr: stderr_parts.join(""),
                            exit_code,
                        });
                    }
                    Ok(Some(WsEvent::Error { error_type, error })) => {
                        return Err(error_from_ws_msg(&error_type, &error, Some(&command_id)));
                    }
                    Ok(Some(WsEvent::Started { .. })) => {
                        // Unexpected but harmless on reconnect
                    }
                    Ok(None) | Err(SandboxError::ServerReload(_)) | Err(SandboxError::Connection(_)) => {
                        // Connection lost — try to reconnect
                        if killed {
                            return Err(SandboxError::Connection(
                                "connection lost after kill".to_string(),
                            ));
                        }

                        reconnect_attempts += 1;
                        if reconnect_attempts > MAX_AUTO_RECONNECTS {
                            return Err(SandboxError::Connection(format!(
                                "lost connection {reconnect_attempts} times, giving up"
                            )));
                        }

                        let is_reload = matches!(event, Err(SandboxError::ServerReload(_)));
                        if !is_reload {
                            let delay = std::cmp::min(
                                BACKOFF_BASE_MS * 2u64.pow(reconnect_attempts - 1),
                                BACKOFF_MAX_MS,
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        }

                        // Reconnect
                        match WsConnection::connect(&params.dataplane_url, &params.api_key).await {
                            Ok(mut new_conn) => {
                                new_conn
                                    .send_reconnect(&command_id, last_stdout_offset, last_stderr_offset)
                                    .await?;
                                conn = new_conn;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
}
