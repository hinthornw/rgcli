use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub assistant_id: String,
    pub input: Value,
    pub stream_mode: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_not_exists: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub thread_id: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub values: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadState {
    pub values: Value,
    #[serde(default)]
    pub next: Option<Vec<String>>,
    #[serde(default)]
    pub checkpoint: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageChunk {
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(rename = "type")]
    pub chunk_type: Option<String>,
}

pub fn new_run_request(assistant_id: &str, user_message: &str) -> RunRequest {
    RunRequest {
        assistant_id: assistant_id.to_string(),
        input: serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": user_message,
                }
            ]
        }),
        stream_mode: vec!["messages-tuple".to_string()],
        if_not_exists: Some("create".to_string()),
    }
}

pub fn parse_message_chunk(data: &str) -> anyhow::Result<Option<MessageChunk>> {
    let value: Value = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(err) => {
            return Err(err.into());
        }
    };

    let chunk_value = if value.is_array() {
        value
            .as_array()
            .and_then(|arr| arr.first().cloned())
            .unwrap_or(Value::Null)
    } else {
        value
    };

    if chunk_value.is_null() {
        return Ok(None);
    }

    let chunk: MessageChunk = serde_json::from_value(chunk_value)?;
    Ok(Some(chunk))
}

pub fn message_chunk_content(chunk: &MessageChunk) -> String {
    let Some(content) = &chunk.content else {
        return String::new();
    };

    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    if let Some(arr) = content.as_array() {
        let mut combined = String::new();
        for item in arr {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                combined.push_str(text);
            }
        }
        return combined;
    }

    String::new()
}

pub fn is_ai_chunk(chunk: &MessageChunk) -> bool {
    match chunk.chunk_type.as_deref() {
        Some("AIMessageChunk") | Some("ai") => true,
        _ => false,
    }
}

pub fn get_messages(values: &Value) -> Vec<Message> {
    let Some(messages) = values.get("messages").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for msg in messages {
        let Some(obj) = msg.as_object() else {
            continue;
        };
        let mut role = obj
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if role.is_empty() {
            if let Some(msg_type) = obj.get("type").and_then(|v| v.as_str()) {
                role = match msg_type {
                    "human" => "user",
                    "ai" => "assistant",
                    other => other,
                }
                .to_string();
            }
        }
        let content = obj
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !role.is_empty() && !content.is_empty() {
            result.push(Message { role, content });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_chunk_array() {
        let data = r#"[{"content":"hello","type":"AIMessageChunk"},{}]"#;
        let chunk = parse_message_chunk(data).unwrap().unwrap();
        assert_eq!(message_chunk_content(&chunk), "hello");
        assert!(is_ai_chunk(&chunk));
    }

    #[test]
    fn get_messages_maps_role() {
        let values = serde_json::json!({
            "messages": [
                {"type":"human","content":"hi"},
                {"role":"assistant","content":"hey"}
            ]
        });
        let messages = get_messages(&values);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hi");
        assert_eq!(messages[1].role, "assistant");
    }
}
