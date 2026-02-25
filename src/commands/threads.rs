use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table};

use crate::api::Client;

pub async fn list(client: &Client, limit: usize) -> Result<()> {
    let threads = client
        .search_threads(limit)
        .await
        .context("failed to search threads")?;

    if threads.is_empty() {
        println!("No threads found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Thread ID", "Created", "Updated"]);

    for t in &threads {
        let id_short: String = t.thread_id.chars().take(12).collect();
        let created = t.created_at.as_deref().unwrap_or("-");
        let updated = t.updated_at.as_deref().unwrap_or("-");
        table.add_row(vec![
            Cell::new(id_short).fg(Color::Cyan),
            Cell::new(created),
            Cell::new(updated),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub async fn get(client: &Client, thread_id: &str) -> Result<()> {
    let thread = client.get_thread(thread_id, &[]).await?;
    println!("{}", serde_json::to_string_pretty(&thread)?);
    Ok(())
}

pub async fn create(client: &Client) -> Result<()> {
    let thread = client.create_thread().await?;
    println!("Created thread: {}", thread.thread_id);
    Ok(())
}

pub async fn delete(client: &Client, thread_id: &str) -> Result<()> {
    let url = format!("{}/threads/{}", client.endpoint(), thread_id);
    client.delete_url(&url).await?;
    println!("Deleted thread: {}", thread_id);
    Ok(())
}

pub async fn state(client: &Client, thread_id: &str) -> Result<()> {
    let state = client.get_thread_state(thread_id).await?;
    println!("{}", serde_json::to_string_pretty(&state)?);
    Ok(())
}

pub async fn history(client: &Client, thread_id: &str, limit: usize) -> Result<()> {
    let url = format!("{}/threads/{}/history", client.endpoint(), thread_id);
    let body = serde_json::json!({ "limit": limit });
    let resp = client.post_json(&url, &body).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn copy(client: &Client, thread_id: &str) -> Result<()> {
    let url = format!("{}/threads/{}/copy", client.endpoint(), thread_id);
    let resp = client.post_json(&url, &serde_json::json!({})).await?;
    if let Some(new_id) = resp.get("thread_id").and_then(|v| v.as_str()) {
        println!("Copied thread to: {}", new_id);
    } else {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    }
    Ok(())
}

pub async fn prune(client: &Client, thread_id: &str) -> Result<()> {
    let url = format!("{}/threads/{}/prune", client.endpoint(), thread_id);
    client.post_json(&url, &serde_json::json!({})).await?;
    println!("Pruned old checkpoints from thread: {}", thread_id);
    Ok(())
}
