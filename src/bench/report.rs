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

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_result(duration_ms: u64, ttft_ms: Option<u64>, token_count: usize, success: bool) -> RunResult {
        RunResult {
            duration: Duration::from_millis(duration_ms),
            ttft: ttft_ms.map(Duration::from_millis),
            token_count,
            total_chars: token_count * 4, // Approximation
            success,
            error: if success { None } else { Some("error".to_string()) },
        }
    }

    #[test]
    fn percentile_empty() {
        let durations: Vec<Duration> = vec![];
        assert_eq!(percentile(&durations, 50), Duration::ZERO);
        assert_eq!(percentile(&durations, 95), Duration::ZERO);
    }

    #[test]
    fn percentile_single_value() {
        let durations = vec![Duration::from_millis(100)];
        assert_eq!(percentile(&durations, 50), Duration::from_millis(100));
        assert_eq!(percentile(&durations, 95), Duration::from_millis(100));
        assert_eq!(percentile(&durations, 99), Duration::from_millis(100));
    }

    #[test]
    fn percentile_sorted_values() {
        let durations = vec![
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(30),
            Duration::from_millis(40),
            Duration::from_millis(50),
            Duration::from_millis(60),
            Duration::from_millis(70),
            Duration::from_millis(80),
            Duration::from_millis(90),
            Duration::from_millis(100),
        ];

        // p50: (50 * 10 / 100) = 5, min(5, 9) = 5 -> index 5 = 60ms
        assert_eq!(percentile(&durations, 50), Duration::from_millis(60));

        // p95: (95 * 10 / 100) = 9, min(9, 9) = 9 -> index 9 = 100ms
        assert_eq!(percentile(&durations, 95), Duration::from_millis(100));

        // p99: (99 * 10 / 100) = 9, min(9, 9) = 9 -> index 9 = 100ms
        assert_eq!(percentile(&durations, 99), Duration::from_millis(100));
    }

    #[test]
    fn bench_report_basic_stats() {
        let results = vec![
            mock_result(100, Some(10), 50, true),
            mock_result(200, Some(20), 60, true),
            mock_result(300, Some(30), 70, true),
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        assert_eq!(report.total, 3);
        assert_eq!(report.succeeded, 3);
        assert_eq!(report.failed, 0);
        assert_eq!(report.total_duration, wall_time);
    }

    #[test]
    fn bench_report_with_failures() {
        let results = vec![
            mock_result(100, Some(10), 50, true),
            mock_result(200, Some(20), 60, false),
            mock_result(300, Some(30), 70, true),
            mock_result(400, Some(40), 80, false),
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        assert_eq!(report.total, 4);
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 2);
    }

    #[test]
    fn bench_report_latency_stats() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
            mock_result(300, None, 70, true),
            mock_result(400, None, 80, true),
            mock_result(500, None, 90, true),
        ];
        let wall_time = Duration::from_secs(5);

        let report = BenchReport::from_results(&results, wall_time);

        // Average latency: (100 + 200 + 300 + 400 + 500) / 5 = 300ms
        assert_eq!(report.avg_latency, Duration::from_millis(300));

        // p50: (50 * 5 / 100) = 2, min(2, 4) = 2 -> index 2 = 300ms
        assert_eq!(report.p50_latency, Duration::from_millis(300));

        // p95: (95 * 5 / 100) = 4, min(4, 4) = 4 -> index 4 = 500ms
        assert_eq!(report.p95_latency, Duration::from_millis(500));

        // p99: (99 * 5 / 100) = 4, min(4, 4) = 4 -> index 4 = 500ms
        assert_eq!(report.p99_latency, Duration::from_millis(500));
    }

    #[test]
    fn bench_report_ttft_stats() {
        let results = vec![
            mock_result(100, Some(10), 50, true),
            mock_result(200, Some(20), 60, true),
            mock_result(300, Some(30), 70, true),
            mock_result(400, Some(40), 80, true),
            mock_result(500, Some(50), 90, true),
        ];
        let wall_time = Duration::from_secs(5);

        let report = BenchReport::from_results(&results, wall_time);

        // Average TTFT: (10 + 20 + 30 + 40 + 50) / 5 = 30ms
        assert_eq!(report.avg_ttft, Some(Duration::from_millis(30)));

        // p50 TTFT: (50 * 5 / 100) = 2, min(2, 4) = 2 -> index 2 = 30ms
        assert_eq!(report.p50_ttft, Some(Duration::from_millis(30)));

        // p95 TTFT: (95 * 5 / 100) = 4, min(4, 4) = 4 -> index 4 = 50ms
        assert_eq!(report.p95_ttft, Some(Duration::from_millis(50)));
    }

    #[test]
    fn bench_report_no_ttft() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        assert_eq!(report.avg_ttft, None);
        assert_eq!(report.p50_ttft, None);
        assert_eq!(report.p95_ttft, None);
    }

    #[test]
    fn bench_report_requests_per_sec() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
            mock_result(300, None, 70, true),
            mock_result(400, None, 80, true),
        ];
        let wall_time = Duration::from_secs(2);

        let report = BenchReport::from_results(&results, wall_time);

        // 4 requests / 2 seconds = 2.0 req/s
        assert_eq!(report.requests_per_sec, 2.0);
    }

    #[test]
    fn bench_report_avg_tokens() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
            mock_result(300, None, 70, true),
            mock_result(400, None, 80, false), // Failed request
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        // Total tokens includes all requests: 50 + 60 + 70 + 80 = 260
        // Average tokens: 260 / 3 successful = 86.66...
        assert!((report.avg_tokens - 86.66666666666667).abs() < 0.0001);
    }

    #[test]
    fn bench_report_tokens_per_sec() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
            mock_result(300, None, 70, true),
            mock_result(400, None, 80, true),
        ];
        let wall_time = Duration::from_secs(2);

        let report = BenchReport::from_results(&results, wall_time);

        // Total tokens: 50 + 60 + 70 + 80 = 260
        // 260 tokens / 2 seconds = 130.0 tok/s
        assert_eq!(report.avg_tokens_per_sec, 130.0);
    }

    #[test]
    fn bench_report_empty_results() {
        let results: Vec<RunResult> = vec![];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        assert_eq!(report.total, 0);
        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.avg_latency, Duration::ZERO);
        assert_eq!(report.p50_latency, Duration::ZERO);
        assert_eq!(report.avg_tokens, 0.0);
        assert_eq!(report.requests_per_sec, 0.0);
    }

    #[test]
    fn bench_report_zero_wall_time() {
        let results = vec![
            mock_result(100, None, 50, true),
            mock_result(200, None, 60, true),
        ];
        let wall_time = Duration::ZERO;

        let report = BenchReport::from_results(&results, wall_time);

        // Should handle zero wall time gracefully
        assert_eq!(report.requests_per_sec, 0.0);
        assert_eq!(report.avg_tokens_per_sec, 0.0);
    }

    #[test]
    fn bench_report_all_failed() {
        let results = vec![
            mock_result(100, None, 50, false),
            mock_result(200, None, 60, false),
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        assert_eq!(report.total, 2);
        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failed, 2);
        assert_eq!(report.avg_tokens, 0.0);
    }

    #[test]
    fn bench_report_mixed_ttft() {
        // Some results with TTFT, some without
        let results = vec![
            mock_result(100, Some(10), 50, true),
            mock_result(200, None, 60, true),
            mock_result(300, Some(30), 70, true),
        ];
        let wall_time = Duration::from_secs(1);

        let report = BenchReport::from_results(&results, wall_time);

        // Should only compute stats for results with TTFT
        assert!(report.avg_ttft.is_some());
        assert!(report.p50_ttft.is_some());

        // Average of 10 and 30 = 20ms
        assert_eq!(report.avg_ttft, Some(Duration::from_millis(20)));
    }

    #[test]
    fn bench_report_percentile_calculation_accuracy() {
        // Create 100 results with predictable latencies
        let mut results = Vec::new();
        for i in 1..=100 {
            results.push(mock_result(i * 10, None, 50, true));
        }
        let wall_time = Duration::from_secs(10);

        let report = BenchReport::from_results(&results, wall_time);

        // p50: (50 * 100 / 100) = 50, min(50, 99) = 50 -> index 50 = 510ms
        assert_eq!(report.p50_latency, Duration::from_millis(510));

        // p95: (95 * 100 / 100) = 95, min(95, 99) = 95 -> index 95 = 960ms
        assert_eq!(report.p95_latency, Duration::from_millis(960));

        // p99: (99 * 100 / 100) = 99, min(99, 99) = 99 -> index 99 = 1000ms
        assert_eq!(report.p99_latency, Duration::from_millis(1000));
    }

    #[test]
    fn bench_report_large_token_counts() {
        let results = vec![
            mock_result(100, None, 1000, true),
            mock_result(200, None, 2000, true),
            mock_result(300, None, 3000, true),
        ];
        let wall_time = Duration::from_secs(3);

        let report = BenchReport::from_results(&results, wall_time);

        // Average: 2000 tokens
        assert_eq!(report.avg_tokens, 2000.0);

        // Total: 6000 tokens / 3 seconds = 2000 tok/s
        assert_eq!(report.avg_tokens_per_sec, 2000.0);
    }
}
