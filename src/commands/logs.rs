use anyhow::Result;

use crate::api::Client;

pub async fn show(
    client: &Client,
    thread_id: Option<&str>,
    run_id: Option<&str>,
    last_n: usize,
) -> Result<()> {
    // If a specific run is given, show its details
    if let (Some(tid), Some(rid)) = (thread_id, run_id) {
        let url = format!("{}/threads/{}/runs/{}", client.endpoint(), tid, rid);
        let resp = client.get_json(&url).await?;
        print_run_detail(&resp);
        return Ok(());
    }

    // If a thread is given, show its recent runs
    if let Some(tid) = thread_id {
        let runs = client.search_runs(tid, last_n).await?;
        if runs.is_empty() {
            println!("No runs found.");
            return Ok(());
        }
        for run in &runs {
            print_run_summary(run);
            println!();
        }
        return Ok(());
    }

    // No thread — search recent threads and show their last runs
    let threads = client.search_threads(5).await?;
    if threads.is_empty() {
        println!("No threads found.");
        return Ok(());
    }

    for thread in &threads {
        let tid_short: String = thread.thread_id.chars().take(8).collect();
        println!("\x1b[1;36mThread {tid_short}\x1b[0m");

        match client.search_runs(&thread.thread_id, 3).await {
            Ok(runs) => {
                for run in &runs {
                    print_run_summary(run);
                }
            }
            Err(e) => println!("  \x1b[31mError: {e}\x1b[0m"),
        }
        println!();
    }
    Ok(())
}

fn print_run_summary(run: &serde_json::Value) {
    let id = run
        .get("run_id")
        .or_else(|| run.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let id_short: String = id.chars().take(8).collect();
    let status = run.get("status").and_then(|v| v.as_str()).unwrap_or("-");
    let created = run
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("-");

    let status_colored = match status {
        "success" => format!("\x1b[32m{status}\x1b[0m"),
        "error" => format!("\x1b[31m{status}\x1b[0m"),
        "running" | "pending" => format!("\x1b[33m{status}\x1b[0m"),
        "interrupted" => format!("\x1b[35m{status}\x1b[0m"),
        _ => status.to_string(),
    };

    println!("  {id_short}  {status_colored:>20}  {created}");
}

fn print_run_detail(run: &serde_json::Value) {
    let id = run
        .get("run_id")
        .or_else(|| run.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let status = run.get("status").and_then(|v| v.as_str()).unwrap_or("-");
    let created = run
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let updated = run
        .get("updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("-");

    println!("\x1b[1mRun:\x1b[0m {id}");
    println!("\x1b[1mStatus:\x1b[0m {status}");
    println!("\x1b[1mCreated:\x1b[0m {created}");
    println!("\x1b[1mUpdated:\x1b[0m {updated}");

    if let Some(metadata) = run.get("metadata") {
        if !metadata.is_null() {
            println!(
                "\x1b[1mMetadata:\x1b[0m {}",
                serde_json::to_string_pretty(metadata).unwrap_or_default()
            );
        }
    }
}
