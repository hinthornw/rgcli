use anyhow::Result;
use comfy_table::{Cell, Color, Table};

use crate::api::Client;

pub async fn list(client: &Client, thread_id: &str, limit: usize) -> Result<()> {
    let url = format!("{}/threads/{}/runs", client.endpoint(), thread_id);
    let body = serde_json::json!({ "limit": limit });
    let resp = client.post_json(&url, &body).await?;

    let runs = resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    if runs.is_empty() {
        println!("No runs found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Run ID", "Status", "Created"]);

    for r in runs {
        let id = r.get("run_id").and_then(|v| v.as_str()).unwrap_or("-");
        let id_short: String = id.chars().take(12).collect();
        let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        let created = r
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let status_cell = match status {
            "success" => Cell::new(status).fg(Color::Green),
            "error" => Cell::new(status).fg(Color::Red),
            "running" | "pending" => Cell::new(status).fg(Color::Yellow),
            _ => Cell::new(status),
        };
        table.add_row(vec![Cell::new(id_short).fg(Color::Cyan), status_cell, Cell::new(created)]);
    }

    println!("{table}");
    Ok(())
}

pub async fn get(client: &Client, thread_id: &str, run_id: &str) -> Result<()> {
    let url = format!(
        "{}/threads/{}/runs/{}",
        client.endpoint(),
        thread_id,
        run_id
    );
    let resp = client.get_json(&url).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn cancel(client: &Client, thread_id: &str, run_id: &str) -> Result<()> {
    client.cancel_run(thread_id, run_id).await?;
    println!("Cancelled run: {}", run_id);
    Ok(())
}
