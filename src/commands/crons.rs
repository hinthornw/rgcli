use anyhow::Result;
use comfy_table::{Cell, Color, Table};

use crate::api::Client;

pub async fn list(client: &Client, assistant_id: Option<&str>) -> Result<()> {
    let url = format!("{}/crons/search", client.endpoint());
    let mut body = serde_json::json!({});
    if let Some(aid) = assistant_id {
        body["assistant_id"] = serde_json::Value::String(aid.to_string());
    }
    let resp = client.post_json(&url, &body).await?;

    let crons = resp.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    if crons.is_empty() {
        println!("No cron jobs found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["ID", "Schedule", "Assistant", "Created"]);

    for c in crons {
        let id = c.get("cron_id").and_then(|v| v.as_str()).unwrap_or("-");
        let id_short: String = id.chars().take(12).collect();
        let schedule = c.get("schedule").and_then(|v| v.as_str()).unwrap_or("-");
        let assistant = c
            .get("assistant_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let created = c.get("created_at").and_then(|v| v.as_str()).unwrap_or("-");
        table.add_row(vec![
            Cell::new(id_short).fg(Color::Cyan),
            Cell::new(schedule),
            Cell::new(assistant),
            Cell::new(created),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub async fn create(client: &Client, assistant_id: &str, schedule: &str) -> Result<()> {
    let url = format!("{}/crons", client.endpoint());
    let body = serde_json::json!({
        "assistant_id": assistant_id,
        "schedule": schedule,
    });
    let resp = client.post_json(&url, &body).await?;
    let id = resp
        .get("cron_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    println!("Created cron job: {id}");
    Ok(())
}

pub async fn delete(client: &Client, cron_id: &str) -> Result<()> {
    let url = format!("{}/crons/{}", client.endpoint(), cron_id);
    client.delete_url(&url).await?;
    println!("Deleted cron job: {cron_id}");
    Ok(())
}
