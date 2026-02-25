use std::time::Duration;

use super::runner::RunResult;

/// Aggregated benchmark statistics.
pub struct BenchReport {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_duration: Duration,
    pub p50_latency: Duration,
    pub p95_latency: Duration,
    pub p99_latency: Duration,
    pub avg_latency: Duration,
    pub p50_ttft: Option<Duration>,
    pub p95_ttft: Option<Duration>,
    pub avg_ttft: Option<Duration>,
    pub requests_per_sec: f64,
    pub avg_tokens: f64,
    pub avg_tokens_per_sec: f64,
}

impl BenchReport {
    pub fn from_results(results: &[RunResult], wall_time: Duration) -> Self {
        let total = results.len();
        let succeeded = results.iter().filter(|r| r.success).count();
        let failed = total - succeeded;

        let mut latencies: Vec<Duration> = results.iter().map(|r| r.duration).collect();
        latencies.sort();

        let mut ttfts: Vec<Duration> = results.iter().filter_map(|r| r.ttft).collect();
        ttfts.sort();

        let avg_latency = if !latencies.is_empty() {
            let sum: Duration = latencies.iter().sum();
            sum / latencies.len() as u32
        } else {
            Duration::ZERO
        };

        let p50_latency = percentile(&latencies, 50);
        let p95_latency = percentile(&latencies, 95);
        let p99_latency = percentile(&latencies, 99);

        let avg_ttft = if !ttfts.is_empty() {
            let sum: Duration = ttfts.iter().sum();
            Some(sum / ttfts.len() as u32)
        } else {
            None
        };
        let p50_ttft = if !ttfts.is_empty() { Some(percentile(&ttfts, 50)) } else { None };
        let p95_ttft = if !ttfts.is_empty() { Some(percentile(&ttfts, 95)) } else { None };

        let requests_per_sec = if wall_time.as_secs_f64() > 0.0 {
            total as f64 / wall_time.as_secs_f64()
        } else {
            0.0
        };

        let total_tokens: usize = results.iter().map(|r| r.token_count).sum();
        let avg_tokens = if succeeded > 0 {
            total_tokens as f64 / succeeded as f64
        } else {
            0.0
        };

        let avg_tokens_per_sec = if wall_time.as_secs_f64() > 0.0 {
            total_tokens as f64 / wall_time.as_secs_f64()
        } else {
            0.0
        };

        Self {
            total,
            succeeded,
            failed,
            total_duration: wall_time,
            p50_latency,
            p95_latency,
            p99_latency,
            avg_latency,
            p50_ttft,
            p95_ttft,
            avg_ttft,
            requests_per_sec,
            avg_tokens,
            avg_tokens_per_sec,
        }
    }

    pub fn print(&self) {
        println!();
        println!("  \x1b[1;36m╭─────────────────────────────────╮\x1b[0m");
        println!("  \x1b[1;36m│      Benchmark Results          │\x1b[0m");
        println!("  \x1b[1;36m╰─────────────────────────────────╯\x1b[0m");
        println!();
        println!(
            "  \x1b[1mRequests:\x1b[0m  {} total, \x1b[32m{} ok\x1b[0m, \x1b[31m{} failed\x1b[0m",
            self.total, self.succeeded, self.failed
        );
        println!(
            "  \x1b[1mDuration:\x1b[0m  {:.2}s",
            self.total_duration.as_secs_f64()
        );
        println!(
            "  \x1b[1mThroughput:\x1b[0m {:.2} req/s",
            self.requests_per_sec
        );
        println!();
        println!("  \x1b[1mLatency:\x1b[0m");
        println!("    avg:  {:>8.0}ms", self.avg_latency.as_millis());
        println!("    p50:  {:>8.0}ms", self.p50_latency.as_millis());
        println!("    p95:  {:>8.0}ms", self.p95_latency.as_millis());
        println!("    p99:  {:>8.0}ms", self.p99_latency.as_millis());
        if let Some(avg) = self.avg_ttft {
            println!();
            println!("  \x1b[1mTime to First Token:\x1b[0m");
            println!("    avg:  {:>8.0}ms", avg.as_millis());
            if let Some(p50) = self.p50_ttft {
                println!("    p50:  {:>8.0}ms", p50.as_millis());
            }
            if let Some(p95) = self.p95_ttft {
                println!("    p95:  {:>8.0}ms", p95.as_millis());
            }
        }
        println!();
        println!(
            "  \x1b[1mTokens:\x1b[0m    {:.1} avg/req, {:.1} tok/s total",
            self.avg_tokens, self.avg_tokens_per_sec
        );
        println!();

        // ASCII histogram of latencies
        print_histogram(
            "Latency Distribution",
            &self.latency_histogram(),
        );
    }

    fn latency_histogram(&self) -> Vec<(String, usize)> {
        // Not great to reconstruct — we'll keep this simple
        Vec::new() // Will be filled from outside
    }
}

pub fn print_histogram(title: &str, buckets: &[(String, usize)]) {
    if buckets.is_empty() {
        return;
    }
    let max_count = buckets.iter().map(|(_, c)| *c).max().unwrap_or(1);
    let bar_width = 30;

    println!("  \x1b[1m{title}:\x1b[0m");
    for (label, count) in buckets {
        let bar_len = if max_count > 0 {
            (*count * bar_width) / max_count
        } else {
            0
        };
        let bar: String = "█".repeat(bar_len);
        println!("    {label:>8} [{bar:<30}] {count}");
    }
    println!();
}

fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = (pct * sorted.len() / 100).min(sorted.len() - 1);
    sorted[idx]
}
