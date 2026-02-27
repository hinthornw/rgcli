use reqwest::multipart;

use super::error::{SandboxError, parse_http_error};
use super::handle::{CommandHandle, WsExecParams, start_streaming};
use super::models::{ExecutionResult, RunOpts, SandboxInfo};

/// A live sandbox instance with dataplane operations (run, read, write).
#[derive(Debug, Clone)]
pub struct Sandbox {
    pub info: SandboxInfo,
    http: reqwest::Client,
    api_key: String,
}

impl Sandbox {
    pub(crate) fn new(info: SandboxInfo, http: reqwest::Client, api_key: String) -> Self {
        Self {
            info,
            http,
            api_key,
        }
    }

    /// The sandbox name.
    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// The dataplane URL, if configured.
    pub fn dataplane_url(&self) -> Option<&str> {
        self.info.dataplane_url.as_deref()
    }

    fn require_dataplane_url(&self) -> Result<&str, SandboxError> {
        self.info
            .dataplane_url
            .as_deref()
            .ok_or(SandboxError::DataplaneNotConfigured)
    }

    /// Execute a command with default options (60s timeout, /bin/bash).
    pub async fn run(&self, command: &str) -> Result<ExecutionResult, SandboxError> {
        self.run_with(&RunOpts::new(command)).await
    }

    /// Execute a command with custom options.
    pub async fn run_with(&self, opts: &RunOpts) -> Result<ExecutionResult, SandboxError> {
        let base = self.require_dataplane_url()?;
        let url = format!("{}/execute", base.trim_end_matches('/'));

        let mut payload = serde_json::json!({
            "command": opts.command,
            "timeout": opts.timeout,
            "shell": opts.shell,
        });
        if !opts.env.is_empty() {
            payload["env"] = serde_json::to_value(&opts.env).unwrap_or_default();
        }
        if let Some(cwd) = &opts.cwd {
            payload["cwd"] = serde_json::Value::String(cwd.clone());
        }

        let resp = self
            .http
            .post(&url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(opts.timeout + 10))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_http_error(status.as_u16(), &body));
        }

        Ok(resp.json::<ExecutionResult>().await?)
    }

    /// Execute a command with streaming output over WebSocket.
    ///
    /// Returns a [`CommandHandle`] that yields [`OutputChunk`](super::models::OutputChunk)s
    /// as they arrive. Supports kill, stdin, and auto-reconnect on transient errors.
    pub async fn run_streaming(&self, command: &str) -> Result<CommandHandle, SandboxError> {
        self.run_streaming_with(&RunOpts::new(command)).await
    }

    /// Execute a command with streaming output and custom options.
    pub async fn run_streaming_with(&self, opts: &RunOpts) -> Result<CommandHandle, SandboxError> {
        let base = self.require_dataplane_url()?;
        start_streaming(WsExecParams {
            dataplane_url: base.to_string(),
            api_key: self.api_key.clone(),
            command: opts.command.clone(),
            timeout: opts.timeout,
            shell: opts.shell.clone(),
            env: opts.env.clone(),
            cwd: opts.cwd.clone(),
        })
        .await
    }

    /// Write content to a file in the sandbox.
    pub async fn write(&self, path: &str, content: &[u8]) -> Result<(), SandboxError> {
        let base = self.require_dataplane_url()?;
        let url = format!("{}/upload", base.trim_end_matches('/'));

        let part = multipart::Part::bytes(content.to_vec()).file_name("file");
        let form = multipart::Form::new().part("file", part);

        let resp = self
            .http
            .post(&url)
            .query(&[("path", path)])
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_http_error(status.as_u16(), &body));
        }
        Ok(())
    }

    /// Read a file from the sandbox.
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, SandboxError> {
        let base = self.require_dataplane_url()?;
        let url = format!("{}/download", base.trim_end_matches('/'));

        let resp = self.http.get(&url).query(&[("path", path)]).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(SandboxError::NotFound {
                    resource_type: "file".to_string(),
                    name: path.to_string(),
                });
            }
            return Err(parse_http_error(status.as_u16(), &body));
        }

        Ok(resp.bytes().await?.to_vec())
    }
}
