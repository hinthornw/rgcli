use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub id: String,
    /// Tool calls made by an AI message (for history display).
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// Tool name for tool result messages.
    #[serde(default)]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    #[allow(dead_code)]
    pub filename: String,
    pub mime_type: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub assistant_id: String,
    pub input: Value,
    pub stream_mode: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_not_exists: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multitask_strategy: Option<String>,
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
    pub id: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(rename = "type")]
    pub chunk_type: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(default)]
    pub tool_call_chunks: Option<Vec<Value>>,
    /// For ToolMessage chunks, the tool name.
    #[serde(default)]
    pub name: Option<String>,
}

pub fn new_run_request(
    assistant_id: &str,
    user_message: &str,
    multitask_strategy: Option<&str>,
    stream_mode: Option<&str>,
) -> RunRequest {
    let mode = stream_mode.unwrap_or("messages-tuple");
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
        stream_mode: vec![mode.to_string()],
        if_not_exists: Some("create".to_string()),
        multitask_strategy: multitask_strategy.map(String::from),
    }
}

/// Create a run request with multimodal content (text + image attachments).
pub fn new_run_request_with_attachments(
    assistant_id: &str,
    user_message: &str,
    attachments: &[Attachment],
    multitask_strategy: Option<&str>,
    stream_mode: Option<&str>,
) -> RunRequest {
    let mode = stream_mode.unwrap_or("messages-tuple");
    let mut content_parts = Vec::new();
    content_parts.push(serde_json::json!({"type": "text", "text": user_message}));
    for att in attachments {
        let data_url = format!("data:{};base64,{}", att.mime_type, att.base64_data);
        content_parts.push(serde_json::json!({
            "type": "image_url",
            "image_url": {"url": data_url}
        }));
    }
    RunRequest {
        assistant_id: assistant_id.to_string(),
        input: serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": content_parts,
                }
            ]
        }),
        stream_mode: vec![mode.to_string()],
        if_not_exists: Some("create".to_string()),
        multitask_strategy: multitask_strategy.map(String::from),
    }
}

/// Create a resume request (no new user message, continues interrupted graph).
pub fn new_resume_request(assistant_id: &str, input: Option<Value>, stream_mode: Option<&str>) -> RunRequest {
    let mode = stream_mode.unwrap_or("messages-tuple");
    RunRequest {
        assistant_id: assistant_id.to_string(),
        input: input.unwrap_or(Value::Null),
        stream_mode: vec![mode.to_string()],
        if_not_exists: None,
        multitask_strategy: None,
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

/// Extract tool calls from an AI message chunk.
pub fn extract_tool_calls(chunk: &MessageChunk) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Check tool_calls field
    if let Some(tc) = &chunk.tool_calls {
        for call in tc {
            let name = call
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = call
                .get("args")
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string(v).unwrap_or_default()
                    }
                })
                .unwrap_or_default();
            if !name.is_empty() {
                calls.push(ToolCall { name, args });
            }
        }
    }

    // Check tool_call_chunks (streaming partial tool calls)
    if calls.is_empty() {
        if let Some(tc) = &chunk.tool_call_chunks {
            for call in tc {
                let name = call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = call
                    .get("args")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    calls.push(ToolCall { name, args });
                }
            }
        }
    }

    calls
}

pub fn is_ai_chunk(chunk: &MessageChunk) -> bool {
    matches!(
        chunk.chunk_type.as_deref(),
        Some("AIMessageChunk") | Some("ai")
    )
}

pub fn is_tool_chunk(chunk: &MessageChunk) -> bool {
    matches!(
        chunk.chunk_type.as_deref(),
        Some("ToolMessage") | Some("ToolMessageChunk") | Some("tool")
    )
}

pub fn get_messages(values: &Value) -> Vec<Message> {
    let Some(messages) = values.get("messages").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut result: Vec<Message> = Vec::new();
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
                    "tool" => "tool",
                    other => other,
                }
                .to_string();
            }
        }

        // Extract tool calls from AI messages
        let mut tool_calls = Vec::new();
        if let Some(tc_arr) = obj.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tc_arr {
                let name = tc
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = tc
                    .get("args")
                    .map(|v| {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string(v).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();
                if !name.is_empty() {
                    tool_calls.push(ToolCall { name, args });
                }
            }
        }

        // Tool name for tool result messages
        let tool_name = obj.get("name").and_then(|v| v.as_str()).map(String::from);

        let content = match obj.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };

        // Skip empty messages (but keep tool messages even if content is empty, if they have tool_calls)
        if role.is_empty() || (content.is_empty() && tool_calls.is_empty() && role != "tool") {
            continue;
        }

        let msg_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        // Merge with previous message if same non-empty ID
        if !msg_id.is_empty() {
            if let Some(last) = result.last_mut() {
                if last.id == msg_id {
                    if !content.is_empty() {
                        last.content.push_str("\n\n");
                        last.content.push_str(&content);
                    }
                    last.tool_calls.extend(tool_calls);
                    continue;
                }
            }
        }
        result.push(Message {
            role,
            content,
            id: msg_id.to_string(),
            tool_calls,
            tool_name,
        });
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

    #[test]
    fn get_messages_with_tool_calls() {
        let values = serde_json::json!({
            "messages": [
                {"type":"human","content":"search for rust"},
                {"type":"ai","content":"","tool_calls":[{"name":"search","args":{"q":"rust"}}],"id":"ai1"},
                {"type":"tool","name":"search","content":"Rust is a language","id":"tool1"},
                {"type":"ai","content":"Rust is a systems programming language.","id":"ai2"}
            ]
        });
        let messages = get_messages(&values);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].tool_calls.len(), 1);
        assert_eq!(messages[1].tool_calls[0].name, "search");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_name, Some("search".to_string()));
    }
}
