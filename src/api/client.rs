use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT};
use reqwest::StatusCode;

use crate::api::sse::{is_end_event, is_message_event, parse_sse, SseEvent};
use crate::api::types::{
    is_ai_chunk, message_chunk_content, new_run_request, parse_message_chunk, Thread, ThreadState,
};
use crate::config::Config;

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

    pub async fn stream_run<F>(
        &self,
        thread_id: &str,
        assistant_id: &str,
        user_message: &str,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(String),
    {
        let url = format!("{}/threads/{}/runs/stream", self.endpoint, thread_id);
        let run_req = new_run_request(assistant_id, user_message);
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .header(ACCEPT, "text/event-stream")
            .json(&run_req)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("failed to create run: {} - {}", status, body);
        }

        let stream = resp.bytes_stream();
        parse_sse(stream, |event| handle_sse(event, &mut on_token))
            .await
            .context("failed to parse SSE stream")?;

        Ok(())
    }
}

fn handle_sse<F>(event: SseEvent, on_token: &mut F)
where
    F: FnMut(String),
{
    if is_end_event(&event) {
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
    if !is_ai_chunk(&chunk) {
        return;
    }
    let content = message_chunk_content(&chunk);
    if !content.is_empty() {
        on_token(content);
    }
}
