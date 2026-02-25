use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::api::Client;
use crate::bench::report::BenchReport;
use crate::bench::runner::{BenchConfig, BenchEvent, RunResult, run_bench};

pub async fn run(
    client: &Client,
    assistant_id: &str,
    concurrent: usize,
    requests: usize,
    inputs: Vec<String>,
) -> Result<()> {
    println!();
    println!(
        "  \x1b[1;35m⚡ Benchmarking\x1b[0m {} requests @ {} concurrent",
        requests, concurrent
    );
    println!(
        "  \x1b[2mAssistant: {} | {} unique inputs\x1b[0m",
        assistant_id,
        inputs.len()
    );
    println!();

    let config = BenchConfig {
        concurrent,
        total_requests: requests,
        assistant_id: assistant_id.to_string(),
        inputs,
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let client = client.clone();

    let start = Instant::now();

    tokio::spawn(async move {
        run_bench(client, config, tx).await;
    });

    let mut results: Vec<RunResult> = Vec::new();
    let mut completed = 0;

    while let Some(event) = rx.recv().await {
        match event {
            BenchEvent::Completed(result) => {
                completed += 1;
                let status = if result.success {
                    "\x1b[32m✓\x1b[0m"
                } else {
                    "\x1b[31m✗\x1b[0m"
                };
                // Progress line
                print!(
                    "\r  {status} {completed}/{requests}  {:.0}ms  ",
                    result.duration.as_millis()
                );
                if let Some(ttft) = result.ttft {
                    print!("ttft={:.0}ms  ", ttft.as_millis());
                }
                print!("{}tok", result.token_count);
                if let Some(ref err) = result.error {
                    let short: String = err.chars().take(40).collect();
                    print!("  \x1b[31m{short}\x1b[0m");
                }
                results.push(result);
            }
            BenchEvent::Done => break,
        }
    }
    println!();

    let wall_time = start.elapsed();
    let report = BenchReport::from_results(&results, wall_time);

    // Build histogram from results
    let mut latencies_ms: Vec<u128> = results.iter().map(|r| r.duration.as_millis()).collect();
    latencies_ms.sort();

    if !latencies_ms.is_empty() {
        let min = latencies_ms[0];
        let max = latencies_ms[latencies_ms.len() - 1];
        let num_buckets = 8usize;
        let step = ((max - min) / num_buckets as u128).max(1);
        let mut buckets: Vec<(String, usize)> = Vec::new();
        for i in 0..num_buckets {
            let lo = min + i as u128 * step;
            let hi = lo + step;
            let count = latencies_ms.iter().filter(|&&v| v >= lo && v < hi).count();
            buckets.push((format!("{lo}ms"), count));
        }
        // Add overflow bucket
        let last_hi = min + num_buckets as u128 * step;
        let overflow = latencies_ms.iter().filter(|&&v| v >= last_hi).count();
        if overflow > 0 {
            buckets.push((format!("{last_hi}ms+"), overflow));
        }

        report.print();
        crate::bench::report::print_histogram("Latency Distribution", &buckets);
    } else {
        report.print();
    }

    Ok(())
}
