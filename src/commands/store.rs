use anyhow::Result;

use crate::api::Client;

fn parse_namespace(namespace: &str) -> Vec<&str> {
    namespace.split('.').collect()
}

pub async fn get_item(client: &Client, namespace: &str, key: &str) -> Result<()> {
    let ns_parts = parse_namespace(namespace);
    let url = format!("{}/store/items", client.endpoint());
    let body = serde_json::json!({
        "namespace": ns_parts,
        "key": key,
    });
    let resp = client.post_json(&url, &body).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn put_item(client: &Client, namespace: &str, key: &str, value: &str) -> Result<()> {
    let ns_parts = parse_namespace(namespace);
    let val: serde_json::Value = serde_json::from_str(value)?;
    let url = format!("{}/store/items", client.endpoint());
    let body = serde_json::json!({
        "namespace": ns_parts,
        "key": key,
        "value": val,
    });
    client.put_json(&url, &body).await?;
    println!("Stored item: {namespace}/{key}");
    Ok(())
}

pub async fn delete_item(client: &Client, namespace: &str, key: &str) -> Result<()> {
    let ns_parts = parse_namespace(namespace);
    let url = format!("{}/store/items", client.endpoint());
    let body = serde_json::json!({
        "namespace": ns_parts,
        "key": key,
    });
    client.delete_json(&url, &body).await?;
    println!("Deleted item: {namespace}/{key}");
    Ok(())
}

pub async fn search(
    client: &Client,
    namespace: &str,
    query: Option<&str>,
    limit: usize,
) -> Result<()> {
    let ns_parts = parse_namespace(namespace);
    let url = format!("{}/store/items/search", client.endpoint());
    let mut body = serde_json::json!({
        "namespace_prefix": ns_parts,
        "limit": limit,
    });
    if let Some(q) = query {
        body["query"] = serde_json::Value::String(q.to_string());
    }
    let resp = client.post_json(&url, &body).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

pub async fn namespaces(client: &Client) -> Result<()> {
    let url = format!("{}/store/namespaces", client.endpoint());
    let resp = client.post_json(&url, &serde_json::json!({})).await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}
