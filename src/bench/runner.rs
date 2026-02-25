use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::api::Client;

/// Result of a single benchmark run.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RunResult {
    pub duration: Duration,
    pub ttft: Option<Duration>,
    pub token_count: usize,
    pub total_chars: usize,
    pub success: bool,
    pub error: Option<String>,
}

/// A completed benchmark run event.
#[derive(Debug)]
pub enum BenchEvent {
    /// A single run completed.
    Completed(RunResult),
    /// All runs finished.
    Done,
}

/// Configuration for a benchmark run.
pub struct BenchConfig {
    pub concurrent: usize,
    pub total_requests: usize,
    pub assistant_id: String,
    pub inputs: Vec<String>,
}

/// Execute the benchmark, sending results through the channel.
pub async fn run_bench(client: Client, config: BenchConfig, tx: mpsc::UnboundedSender<BenchEvent>) {
    let client = Arc::new(client);
    let inputs = Arc::new(config.inputs);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(config.concurrent));
    let mut handles = Vec::new();

    for i in 0..config.total_requests {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let client = client.clone();
        let inputs = inputs.clone();
        let assistant_id = config.assistant_id.clone();
        let tx = tx.clone();

        let handle = tokio::spawn(async move {
            let input = &inputs[i % inputs.len()];
            let result = run_single(&client, &assistant_id, input).await;
            let _ = tx.send(BenchEvent::Completed(result));
            drop(permit);
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }
    let _ = tx.send(BenchEvent::Done);
}

async fn run_single(client: &Client, assistant_id: &str, input: &str) -> RunResult {
    let start = Instant::now();

    // Create a fresh thread for each request
    let thread = match client.create_thread().await {
        Ok(t) => t,
        Err(e) => {
            return RunResult {
                duration: start.elapsed(),
                ttft: None,
                token_count: 0,
                total_chars: 0,
                success: false,
                error: Some(e.to_string()),
            };
        }
    };

    // Use streaming to measure TTFT
    let (tx, mut rx) = mpsc::unbounded_channel();
    let client_clone = client.clone();
    let thread_id = thread.thread_id.clone();
    let assistant_id = assistant_id.to_string();
    let input = input.to_string();

    tokio::spawn(async move {
        client_clone
            .stream_run(&thread_id, &assistant_id, &input, None, None, &tx)
            .await;
    });

    let mut ttft: Option<Duration> = None;
    let mut token_count = 0usize;
    let mut total_chars = 0usize;
    let mut error = None;

    while let Some(event) = rx.recv().await {
        match event {
            crate::api::StreamEvent::Token(text) => {
                if ttft.is_none() {
                    ttft = Some(start.elapsed());
                }
                token_count += 1;
                total_chars += text.len();
            }
            crate::api::StreamEvent::Done(result) => {
                if let Err(e) = result {
                    error = Some(e.to_string());
                }
                break;
            }
            _ => {}
        }
    }

    RunResult {
        duration: start.elapsed(),
        ttft,
        token_count,
        total_chars,
        success: error.is_none(),
        error,
    }
}
