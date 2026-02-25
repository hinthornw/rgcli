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
        let updated = a.get("updated_at").and_then(|v| v.as_str()).unwrap_or("-");
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

pub async fn graph(client: &Client, assistant_id: &str, ascii: bool) -> Result<()> {
    let url = format!("{}/assistants/{}/graph", client.endpoint(), assistant_id);
    let resp = client.get_json(&url).await?;

    if ascii {
        render_ascii_graph(&resp)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    }
    Ok(())
}

fn render_ascii_graph(graph: &serde_json::Value) -> Result<()> {
    use std::collections::HashMap;

    let nodes = graph
        .get("nodes")
        .and_then(|v| v.as_array())
        .context("missing 'nodes' array")?;

    let edges = graph
        .get("edges")
        .and_then(|v| v.as_array())
        .context("missing 'edges' array")?;

    // Build adjacency list
    let mut outgoing: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    for edge in edges {
        let source = edge
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let target = edge
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let conditional = edge
            .get("conditional")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        outgoing
            .entry(source)
            .or_default()
            .push((target, conditional));
    }

    // Collect node IDs in order
    let mut node_ids: Vec<String> = nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // Sort to ensure __start__ is first and __end__ is last
    node_ids.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("__start__", _) => std::cmp::Ordering::Less,
        (_, "__start__") => std::cmp::Ordering::Greater,
        ("__end__", _) => std::cmp::Ordering::Greater,
        (_, "__end__") => std::cmp::Ordering::Less,
        _ => a.cmp(b),
    });

    // Render each node and its outgoing edges
    for (i, node_id) in node_ids.iter().enumerate() {
        // Draw the node box
        println!("  [{}]", node_id);

        // Draw edges to targets
        if let Some(targets) = outgoing.get(node_id) {
            match targets.len().cmp(&1) {
                std::cmp::Ordering::Equal => {
                    // Single edge - simple vertical arrow
                    let (_target, conditional) = &targets[0];
                    println!("       │");
                    if *conditional {
                        println!("       ▼ (conditional)");
                    } else {
                        println!("       ▼");
                    }
                }
                std::cmp::Ordering::Greater => {
                    // Multiple edges - show side by side
                    println!("       │");
                    let mut line = String::from("      ");
                    for (j, (_, conditional)) in targets.iter().enumerate() {
                        if j > 0 {
                            line.push_str("  ");
                        }
                        if *conditional {
                            line.push_str("▼(c)");
                        } else {
                            line.push('▼');
                        }
                    }
                    println!("{}", line);

                    let mut names_line = String::from("     ");
                    for (j, (target, _)) in targets.iter().enumerate() {
                        if j > 0 {
                            names_line.push_str("  ");
                        }
                        names_line.push_str(&format!("[{}]", target));
                    }
                    println!("{}", names_line);
                }
                std::cmp::Ordering::Less => {}
            }
        }

        // Add spacing between nodes if not last
        if i < node_ids.len() - 1 && outgoing.contains_key(node_id) {
            // Skip extra spacing as we already drew the connection
        } else if i < node_ids.len() - 1 {
            println!();
        }
    }

    Ok(())
}

pub async fn schemas(client: &Client, assistant_id: &str) -> Result<()> {
    let url = format!("{}/assistants/{}/schemas", client.endpoint(), assistant_id);
    let resp = client.get_json(&url).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn versions(client: &Client, assistant_id: &str) -> Result<()> {
    let url = format!("{}/assistants/{}/versions", client.endpoint(), assistant_id);
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
            let created = v.get("created_at").and_then(|v| v.as_str()).unwrap_or("-");
            table.add_row(vec![version, created.to_string()]);
        }
        println!("{table}");
    } else {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    }
    Ok(())
}
