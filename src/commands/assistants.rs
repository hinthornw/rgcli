use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table};

use crate::api::Client;

pub async fn list(client: &Client) -> Result<()> {
    let assistants = client
        .list_assistants()
        .await
        .context("failed to list assistants")?;

    if assistants.is_empty() {
        println!("No assistants found.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["ID", "Name", "Graph", "Updated"]);

    for a in &assistants {
        let id = a
            .get("assistant_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let graph = a.get("graph_id").and_then(|v| v.as_str()).unwrap_or("-");
        let updated = a
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        table.add_row(vec![
            Cell::new(id).fg(Color::Cyan),
            Cell::new(name),
            Cell::new(graph),
            Cell::new(updated),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub async fn get(client: &Client, assistant_id: &str) -> Result<()> {
    let assistants = client.list_assistants().await?;
    let assistant = assistants
        .iter()
        .find(|a| {
            a.get("assistant_id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| id == assistant_id)
        })
        .context("assistant not found")?;

    println!("{}", serde_json::to_string_pretty(assistant)?);
    Ok(())
}

pub async fn graph(client: &Client, assistant_id: &str) -> Result<()> {
    let url = format!("{}/assistants/{}/graph", client.endpoint(), assistant_id);
    let resp = client.get_json(&url).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn schemas(client: &Client, assistant_id: &str) -> Result<()> {
    let url = format!("{}/assistants/{}/schemas", client.endpoint(), assistant_id);
    let resp = client.get_json(&url).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn versions(client: &Client, assistant_id: &str) -> Result<()> {
    let url = format!(
        "{}/assistants/{}/versions",
        client.endpoint(),
        assistant_id
    );
    let resp = client.post_json(&url, &serde_json::json!({})).await?;
    if let Some(arr) = resp.as_array() {
        let mut table = Table::new();
        table.set_header(vec!["Version", "Created"]);
        for v in arr {
            let version = v
                .get("version")
                .and_then(|v| v.as_i64())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string());
            let created = v
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            table.add_row(vec![version, created.to_string()]);
        }
        println!("{table}");
    } else {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    }
    Ok(())
}
