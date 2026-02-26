use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;

use super::{ChatMessage, ChatState};
use crate::api::{Client, StreamEvent};

#[derive(Clone)]
pub(crate) struct ToolExec {
    pub(crate) name: String,
    pub(crate) started_at: Instant,
    pub(crate) completed_at: Option<Instant>,
    pub(crate) duration_ms: Option<u128>,
}

#[derive(Default, Clone)]
pub(crate) struct RunMetrics {
    pub(crate) run_started_at: Option<Instant>,
    pub(crate) first_token_at: Option<Instant>,
    pub(crate) last_token_at: Option<Instant>,
    pub(crate) token_count: usize,
    pub(crate) total_chars: usize,
    pub(crate) run_id: Option<String>,
    pub(crate) last_ttft_ms: Option<u128>,
    pub(crate) last_tokens_per_sec: Option<f64>,
    pub(crate) last_total_ms: Option<u128>,
    pub(crate) last_token_count: Option<usize>,
    pub(crate) last_run_id: Option<String>,
    pub(crate) tool_timeline: Vec<ToolExec>,
    pub(crate) last_tool_timeline: Vec<ToolExec>,
    pub(crate) current_node: Option<String>,
    pub(crate) node_history: Vec<String>,
    pub(crate) last_node_history: Vec<String>,
    pub(crate) feedback_url: Option<String>,
    pub(crate) last_feedback_url: Option<String>,
}

pub(super) async fn check_for_updates_loop(tx: mpsc::UnboundedSender<String>) -> Result<()> {
    use crate::update;
    tokio::time::sleep(Duration::from_secs(5)).await;
    loop {
        let _ = update::force_check().await;
        if let Some(notice) = update::pending_update_notice() {
            let _ = tx.send(notice);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

pub(super) async fn recv_stream(
    rx: &mut Option<mpsc::UnboundedReceiver<StreamEvent>>,
) -> Option<StreamEvent> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

pub(super) async fn handle_stream_event(
    app: &mut ChatState,
    event: StreamEvent,
    client: &Client,
    thread_id: &str,
) {
    match event {
        StreamEvent::RunStarted(id) => {
            crate::debug_log::log("stream", &format!("run started: {id}"));
            app.active_run_id = Some(id.clone());
            app.metrics.run_id = Some(id);
        }
        StreamEvent::NewMessage(_id) => {
            if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
        }
        StreamEvent::Token(token) => {
            if app.metrics.first_token_at.is_none() {
                app.metrics.first_token_at = Some(Instant::now());
            }
            app.metrics.last_token_at = Some(Instant::now());
            app.metrics.token_count += 1;
            app.metrics.total_chars += token.len();
            app.is_waiting = false;
            app.streaming_text.push_str(&token);
            app.auto_scroll = true;
        }
        StreamEvent::ToolUse(name, args) => {
            if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
            app.metrics.tool_timeline.push(ToolExec {
                name: name.clone(),
                started_at: Instant::now(),
                completed_at: None,
                duration_ms: None,
            });
            app.messages.push(ChatMessage::ToolUse(name, args));
            app.is_waiting = false;
            app.auto_scroll = true;
        }
        StreamEvent::ToolResult(name, content) => {
            let now = Instant::now();
            if let Some(tool) = app
                .metrics
                .tool_timeline
                .iter_mut()
                .rev()
                .find(|t| t.name == name && t.completed_at.is_none())
            {
                tool.completed_at = Some(now);
                tool.duration_ms = Some(now.duration_since(tool.started_at).as_millis());
            }
            app.messages.push(ChatMessage::ToolResult(name, content));
            app.auto_scroll = true;
        }
        StreamEvent::FeedbackUrls(urls) => {
            if let Some(url) = urls.get("user_score") {
                app.metrics.feedback_url = Some(url.clone());
            }
        }
        StreamEvent::Done(result) => {
            if let Err(ref err) = result {
                crate::debug_log::log("stream", &format!("run error: {err}"));
                app.messages
                    .push(ChatMessage::Error(format!("Error: {}", err)));
            } else if !app.streaming_text.is_empty() {
                let text = std::mem::take(&mut app.streaming_text);
                app.messages.push(ChatMessage::Assistant(text));
            }
            app.streaming_text.clear();
            app.stream_rx = None;
            app.active_run_id = None;
            app.is_waiting = false;

            // Snapshot metrics
            if let Some(started) = app.metrics.run_started_at {
                app.metrics.last_total_ms = Some(started.elapsed().as_millis());
                app.metrics.last_ttft_ms = app
                    .metrics
                    .first_token_at
                    .map(|t| t.duration_since(started).as_millis());
                app.metrics.last_token_count = Some(app.metrics.token_count);
                app.metrics.last_run_id = app.metrics.run_id.clone();
                if let (Some(first), Some(last)) =
                    (app.metrics.first_token_at, app.metrics.last_token_at)
                {
                    let dur = last.duration_since(first).as_secs_f64();
                    if dur > 0.0 && app.metrics.token_count > 1 {
                        app.metrics.last_tokens_per_sec =
                            Some((app.metrics.token_count - 1) as f64 / dur);
                    }
                }
            }
            app.metrics.last_tool_timeline = std::mem::take(&mut app.metrics.tool_timeline);
            app.metrics.last_feedback_url = app.metrics.feedback_url.take();
            app.metrics.last_node_history = std::mem::take(&mut app.metrics.node_history);
            app.metrics.current_node = None;

            // Trace link (devtools only)
            if app.devtools {
                if let (Some(run_id), Some(tid)) = (&app.metrics.run_id, &app.tenant_id) {
                    let url = if let Some(sid) = &app.tracer_session_id {
                        format!("https://smith.langchain.com/o/{tid}/projects/p/{sid}/r/{run_id}?trace_id={run_id}")
                    } else {
                        format!("https://smith.langchain.com/o/{tid}/r/{run_id}")
                    };
                    app.messages
                        .push(ChatMessage::System(format!("trace: {url}")));
                }
            }

            // Check for human-in-the-loop interrupt
            if result.is_ok() {
                let assistant_id = app.assistant_id.clone();
                if let Ok(state) = client.get_thread_state(thread_id).await {
                    if let Some(next) = &state.next {
                        if !next.is_empty() {
                            let nodes = next.join(", ");
                            app.messages.push(ChatMessage::System(format!(
                                "⏸ Graph paused at: {nodes}. Press Enter to continue or type a response."
                            )));
                            app.interrupted = true;
                            return;
                        }
                    }
                }
                let _ = assistant_id;
            }

            // Drain queue
            if let Some(msg) = app.pending_messages.pop_front() {
                if let Some(ChatMessage::System(s)) = app.messages.last() {
                    if s == "(queued)" {
                        app.messages.pop();
                    }
                }
                let aid = app.assistant_id.clone();
                app.messages.push(ChatMessage::User(msg.clone()));
                start_run(client, thread_id, &aid, &msg, Some("enqueue"), app);
            }
        }
    }
}

/// Prepare a new run: reset state and return the channel sender for the spawned task.
fn prepare_run(app: &mut ChatState) -> mpsc::UnboundedSender<StreamEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    app.stream_rx = Some(rx);
    app.is_waiting = true;
    app.spinner_idx = 0;
    app.active_run_id = None;
    app.streaming_text.clear();
    app.metrics.run_started_at = Some(Instant::now());
    app.metrics.first_token_at = None;
    app.metrics.last_token_at = None;
    app.metrics.token_count = 0;
    app.metrics.total_chars = 0;
    app.metrics.run_id = None;
    app.metrics.tool_timeline.clear();
    app.metrics.node_history.clear();
    app.metrics.current_node = None;
    app.metrics.feedback_url = None;
    app.feedback_submitted = None;
    tx
}

pub(super) fn start_run(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    message: &str,
    multitask_strategy: Option<&str>,
    app: &mut ChatState,
) {
    let tx = prepare_run(app);
    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let message = message.to_string();
    let strategy = multitask_strategy.map(String::from);
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .stream_run(
                &thread_id,
                &assistant_id,
                &message,
                strategy.as_deref(),
                Some(&stream_mode),
                &tx,
            )
            .await;
    });
}

pub(super) fn start_run_with_attachments(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    message: &str,
    attachments: &[crate::api::types::Attachment],
    app: &mut ChatState,
) {
    let tx = prepare_run(app);
    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let message = message.to_string();
    let attachments = attachments.to_vec();
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .stream_run_with_attachments(
                &thread_id,
                &assistant_id,
                &message,
                &attachments,
                Some(&stream_mode),
                &tx,
            )
            .await;
    });
}

pub(super) fn start_resume(
    client: &Client,
    thread_id: &str,
    assistant_id: &str,
    input: Option<serde_json::Value>,
    app: &mut ChatState,
) {
    let tx = prepare_run(app);
    let client = client.clone();
    let thread_id = thread_id.to_string();
    let assistant_id = assistant_id.to_string();
    let stream_mode = app.stream_mode.clone();

    tokio::spawn(async move {
        client
            .resume_run(&thread_id, &assistant_id, input, Some(&stream_mode), &tx)
            .await;
    });
}
