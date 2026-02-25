use anyhow::{Context, Result};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::api::sse::{
    SseEvent, extract_run_id, is_end_event, is_message_event, is_metadata_event, parse_sse,
};
use crate::api::types::{
    Attachment, Thread, ThreadState, extract_tool_calls, is_ai_chunk, is_tool_chunk,
    message_chunk_content, new_resume_request, new_run_request, new_run_request_with_attachments,
    parse_message_chunk,
};
use crate::config::Config;

/// Events emitted by a streaming run.
#[derive(Debug)]
pub enum StreamEvent {
    /// Run ID extracted from metadata event.
    RunStarted(String),
    /// A new AI message started (different message ID).
    NewMessage(String),
    /// A text token from the assistant.
    Token(String),
    /// Tool call detected (name, args).
    ToolUse(String, String),
    /// Tool result received (tool name, content).
    ToolResult(String, String),
    /// Stream completed (Ok or Err).
    Done(Result<()>),
}

#[derive(Clone)]
pub struct Client {
    endpoint: String,
    headers: HeaderMap,
    http: reqwest::Client,
}

impl Client {
    pub fn new(cfg: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        for (key, value) in cfg.headers() {
            let name = HeaderName::from_bytes(key.as_bytes())?;
            let value = HeaderValue::from_str(&value)?;
            headers.insert(name, value);
        }
        Ok(Self {
            endpoint: cfg.endpoint.trim_end_matches('/').to_string(),
            headers,
            http: reqwest::Client::new(),
        })
    }

    pub async fn create_thread(&self) -> Result<Thread> {
        let url = format!("{}/threads", self.endpoint);
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .body("{}")
            .send()
            .await?;

        if resp.status() != StatusCode::OK && resp.status() != StatusCode::CREATED {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to create thread: {} - {}", status, body);
        }

        Ok(resp.json::<Thread>().await?)
    }

    pub async fn search_threads(&self, limit: usize) -> Result<Vec<Thread>> {
        let url = format!("{}/threads/search", self.endpoint);
        let body = serde_json::json!({ "limit": limit });
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .json(&body)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to search threads: {} - {}", status, body);
        }

        Ok(resp.json::<Vec<Thread>>().await?)
    }

    pub async fn get_thread_state(&self, thread_id: &str) -> Result<ThreadState> {
        let url = format!("{}/threads/{}/state", self.endpoint, thread_id);
        let resp = self
            .http
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to get thread state: {} - {}", status, body);
        }

        Ok(resp.json::<ThreadState>().await?)
    }

    #[allow(dead_code)]
    pub async fn get_thread(&self, thread_id: &str, select_fields: &[&str]) -> Result<Thread> {
        let mut url = format!("{}/threads/{}", self.endpoint, thread_id);
        if !select_fields.is_empty() {
            url.push_str("?select=");
            url.push_str(&select_fields.join(","));
        }
        let resp = self
            .http
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to get thread: {} - {}", status, body);
        }

        Ok(resp.json::<Thread>().await?)
    }

    /// Start a streaming run, sending events through the provided channel.
    pub async fn stream_run(
        &self,
        thread_id: &str,
        assistant_id: &str,
        user_message: &str,
        multitask_strategy: Option<&str>,
        tx: &mpsc::UnboundedSender<StreamEvent>,
    ) {
        let result = self
            .stream_run_inner(
                thread_id,
                assistant_id,
                user_message,
                multitask_strategy,
                tx,
            )
            .await;
        let _ = tx.send(StreamEvent::Done(result));
    }

    /// Start a streaming run with file attachments.
    pub async fn stream_run_with_attachments(
        &self,
        thread_id: &str,
        assistant_id: &str,
        user_message: &str,
        attachments: &[Attachment],
        tx: &mpsc::UnboundedSender<StreamEvent>,
    ) {
        let url = format!("{}/threads/{}/runs/stream", self.endpoint, thread_id);
        let run_req =
            new_run_request_with_attachments(assistant_id, user_message, attachments, None);
        let result = self.do_stream(&url, &run_req, tx).await;
        let _ = tx.send(StreamEvent::Done(result));
    }

    /// Resume an interrupted run (human-in-the-loop).
    pub async fn resume_run(
        &self,
        thread_id: &str,
        assistant_id: &str,
        input: Option<Value>,
        tx: &mpsc::UnboundedSender<StreamEvent>,
    ) {
        let url = format!("{}/threads/{}/runs/stream", self.endpoint, thread_id);
        let run_req = new_resume_request(assistant_id, input);
        let result = self.do_stream(&url, &run_req, tx).await;
        let _ = tx.send(StreamEvent::Done(result));
    }

    async fn stream_run_inner(
        &self,
        thread_id: &str,
        assistant_id: &str,
        user_message: &str,
        multitask_strategy: Option<&str>,
        tx: &mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<()> {
        let url = format!("{}/threads/{}/runs/stream", self.endpoint, thread_id);
        let run_req = new_run_request(assistant_id, user_message, multitask_strategy);
        self.do_stream(&url, &run_req, tx).await
    }

    async fn do_stream(
        &self,
        url: &str,
        run_req: &crate::api::types::RunRequest,
        tx: &mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<()> {
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .header(ACCEPT, "text/event-stream")
            .json(run_req)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to create run: {} - {}", status, body);
        }

        let stream = resp.bytes_stream();
        let mut current_msg_id: Option<String> = None;
        parse_sse(stream, |event| handle_sse(event, tx, &mut current_msg_id))
            .await
            .context("failed to parse SSE stream")?;

        Ok(())
    }

    /// Run and wait for the final result (non-streaming). Returns raw JSON response.
    pub async fn wait_run(
        &self,
        thread_id: &str,
        assistant_id: &str,
        user_message: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/threads/{}/runs/wait", self.endpoint, thread_id);
        let run_req = new_run_request(assistant_id, user_message, None);
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .json(&run_req)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("run failed: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }

    /// Get deployment info (version, etc.)
    pub async fn get_info(&self) -> Result<serde_json::Value> {
        let url = format!("{}/info", self.endpoint);
        let resp = self
            .http
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to get info: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }

    /// Get project details from LangSmith API (for hosted deployments).
    pub async fn get_project_details(
        &self,
        project_id: &str,
        tenant_id: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("https://api.smith.langchain.com/v1/projects/{}", project_id);
        let resp = self
            .http
            .get(url)
            .headers(self.headers.clone())
            .header("X-Tenant-Id", tenant_id)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to get project details: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }

    /// List available assistants.
    pub async fn list_assistants(&self) -> Result<Vec<serde_json::Value>> {
        let url = format!("{}/assistants/search", self.endpoint);
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .body("{}")
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to list assistants: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }

    /// Cancel a running run.
    pub async fn cancel_run(&self, thread_id: &str, run_id: &str) -> Result<()> {
        let url = format!(
            "{}/threads/{}/runs/{}/cancel",
            self.endpoint, thread_id, run_id
        );
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .body("{}")
            .send()
            .await?;

        let status = resp.status();
        if status != StatusCode::OK && status != StatusCode::ACCEPTED {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to cancel run: {} - {}", status, body);
        }
        Ok(())
    }
}

fn handle_sse(
    event: SseEvent,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    current_msg_id: &mut Option<String>,
) {
    if is_end_event(&event) {
        return;
    }
    if is_metadata_event(&event) {
        if let Some(run_id) = extract_run_id(&event) {
            let _ = tx.send(StreamEvent::RunStarted(run_id));
        }
        return;
    }
    if !is_message_event(&event) {
        return;
    }
    let Ok(chunk) = parse_message_chunk(&event.data) else {
        return;
    };
    let Some(chunk) = chunk else {
        return;
    };

    // Handle tool result messages
    if is_tool_chunk(&chunk) {
        let tool_name = chunk.name.clone().unwrap_or_else(|| "tool".to_string());
        let content = message_chunk_content(&chunk);
        if !content.is_empty() {
            let _ = tx.send(StreamEvent::ToolResult(tool_name, content));
        }
        return;
    }

    if !is_ai_chunk(&chunk) {
        return;
    }

    // Detect new message by ID change
    if let Some(id) = &chunk.id {
        if current_msg_id.as_ref() != Some(id) {
            if current_msg_id.is_some() {
                let _ = tx.send(StreamEvent::NewMessage(id.clone()));
            }
            *current_msg_id = Some(id.clone());
        }
    }

    // Check for tool calls in AI chunk
    let tool_calls = extract_tool_calls(&chunk);
    for tc in &tool_calls {
        let _ = tx.send(StreamEvent::ToolUse(tc.name.clone(), tc.args.clone()));
    }

    // Also emit text content if present
    let content = message_chunk_content(&chunk);
    if !content.is_empty() {
        let _ = tx.send(StreamEvent::Token(content));
    }
}
